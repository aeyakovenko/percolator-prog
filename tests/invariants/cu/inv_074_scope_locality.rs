//! INV-074 - Scope locality.
//!
//! Normative obligation: Scoped state affects only its own asset, side, portfolio, domain, close, or receipt.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public
//! bankruptcy-scope, permissionless-oracle, base-stale, and non-base-route
//! regressions exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: these bounded public worlds certify the covered scope
//! relationships, not every possible cross-domain composition. The audit matrix
//! records the remaining side, receipt, and lifecycle cross-products.

use super::*;

fn configure_scope_probe_asset(env: &mut V16CuEnv, creator: &Keypair, start_slot: u64) {
    let creator_key = creator.pubkey();
    env.activate_permissionless_asset_with_fee(
        creator,
        1,
        start_slot,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, creator, start_slot, 100);
}

fn trigger_scope_probe_bankruptcy(
    env: &mut V16CuEnv,
    creator: &Keypair,
    start_slot: u64,
) -> (Pubkey, Pubkey) {
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 250);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );

    for (slot, mark) in [
        (start_slot + 1, 200u64),
        (start_slot + 2, 400),
        (start_slot + 3, 800),
    ] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_with_authority(1, creator, slot, mark);
        for portfolio in [long, short] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.crank_steps(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: start_slot + 3,
            observations: crank_observations(1),
        },
        4,
    );
    let (_, group) = env.market_state();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert!(group.bankruptcy_hlock_active);
    assert_eq!(env.portfolio_state(short).capital.get(), 0);
    (long, short)
}

#[test]
fn v16_program_unrelated_bankruptcy_preserves_backed_claim_and_owner_exit() {
    const DEPOSIT: u128 = 1_000_000;
    const CLAIM: u128 = 100;

    for cohort_count in [2usize, 3] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_with_cu(1, 100);
        env.update_market_init_fee_policy_with_cu(1);
        let creator = Keypair::new();
        configure_scope_probe_asset(&mut env, &creator, 1);

        let mut cohorts = Vec::new();
        for _ in 0..cohort_count {
            let winner_owner = Keypair::new();
            let loser_owner = Keypair::new();
            let winner = env.create_portfolio(&winner_owner);
            let loser = env.create_portfolio(&loser_owner);
            env.deposit(&winner_owner, winner, DEPOSIT);
            env.deposit(&loser_owner, loser, DEPOSIT);
            env.trade_asset_with_cu(
                0,
                &winner_owner,
                winner,
                &loser_owner,
                loser,
                POS_SCALE as i128,
                100,
                0,
            );
            cohorts.push((winner_owner, winner, loser_owner, loser));
        }

        env.svm.warp_to_slot(2);
        env.push_auth_mark_with_cu(2, 200);
        for (_, winner, _, loser) in &cohorts {
            for portfolio in [*loser, *winner] {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                );
            }
        }
        for (winner_owner, winner, loser_owner, loser) in &cohorts {
            env.trade_asset_with_cu(
                0,
                winner_owner,
                *winner,
                loser_owner,
                *loser,
                -(POS_SCALE as i128),
                200,
                0,
            );
            assert_eq!(env.portfolio_state(*winner).pnl.get(), CLAIM as i128);
        }

        env.convert_released_pnl_with_cu(&cohorts[0].0, cohorts[0].1, CLAIM);
        assert_eq!(
            env.portfolio_state(cohorts[0].1).capital.get(),
            DEPOSIT + CLAIM
        );

        let (failed_long, failed_short) = trigger_scope_probe_bankruptcy(&mut env, &creator, 3);
        let target_owner = &cohorts[1].0;
        let target = cohorts[1].1;
        env.crank(
            target,
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
                observations: crank_observations(0),
            },
        );
        assert_eq!(env.portfolio_state(target).pnl.get(), CLAIM as i128);

        env.svm.warp_to_slot(6);
        for portfolio in [failed_short, failed_long, target] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 6,
                    observations: crank_observations(1),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }

        let before_conversion = env.market_state().1;
        let failed_short_before = env.svm.get_account(&failed_short).unwrap();
        let failed_long_before = env.svm.get_account(&failed_long).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        assert!(before_conversion.bankruptcy_hlock_active);

        let conversion_cu = env.convert_released_pnl_with_cu(target_owner, target, CLAIM);
        assert_cu_within(
            "INV-074 unrelated source-backed conversion",
            conversion_cu,
            CUSTODY_CU_LIMIT,
        );
        let converted = env.portfolio_state(target);
        let after_conversion = env.market_state().1;
        assert_eq!(after_conversion.mode, MarketModeV16::Live);
        assert!(
            after_conversion.bankruptcy_hlock_active,
            "conversion must not erase the unrelated bankruptcy history"
        );
        assert_eq!(converted.pnl.get(), 0);
        assert_eq!(converted.capital.get(), DEPOSIT + CLAIM);
        assert!(
            converted
                .source_domains
                .iter()
                .all(|source| source.source_claim_bound_num.get() == 0),
            "the realized claim must not remain reusable"
        );
        assert_eq!(after_conversion.c_tot, before_conversion.c_tot + CLAIM);
        assert_eq!(after_conversion.vault, before_conversion.vault);
        assert_eq!(
            env.svm.get_account(&failed_short).unwrap(),
            failed_short_before
        );
        assert_eq!(
            env.svm.get_account(&failed_long).unwrap(),
            failed_long_before
        );
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let vault_atoms_before_exit = env.token_amount(env.vault);
        let (destination, withdraw_cu) =
            env.withdraw_with_cu(target_owner, target, DEPOSIT + CLAIM);
        assert_cu_within(
            "INV-074 unrelated backed claimant full exit",
            withdraw_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(env.token_amount(destination), (DEPOSIT + CLAIM) as u64);
        assert_eq!(
            env.token_amount(env.vault),
            vault_atoms_before_exit - (DEPOSIT + CLAIM) as u64
        );
        let materialized_before_close = env.market_state().1.materialized_portfolio_count;
        env.close_portfolio_with_cu(target_owner, target);
        assert_eq!(
            env.market_state().1.materialized_portfolio_count,
            materialized_before_close - 1
        );
        if let Some(closed) = env.svm.get_account(&target) {
            assert_eq!(closed.lamports, 0);
            assert!(closed.data.is_empty() || !state::is_initialized(&closed.data));
        }
        assert!(env.market_state().1.bankruptcy_hlock_active);
    }
}

#[test]
fn v16_attack_permissionless_asset_bankruptcy_does_not_freeze_base_trading() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.update_market_init_fee_policy_with_cu(1);

    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let attacker_long = Keypair::new();
    let attacker_short = Keypair::new();
    let long_account = env.create_portfolio(&attacker_long);
    let short_account = env.create_portfolio(&attacker_short);
    env.deposit(&attacker_long, long_account, 1_000_000);
    env.deposit(&attacker_short, short_account, 250);
    env.trade_asset_with_cu(
        1,
        &attacker_long,
        long_account,
        &attacker_short,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    for (slot, mark) in [(2u64, 200u64), (3, 400), (4, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_with_authority(1, &creator, slot, mark);
        for portfolio in [long_account, short_account] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.crank_steps(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(1),
        },
        4,
    );

    let (_, failed_asset_group) = env.market_state();
    assert_eq!(failed_asset_group.mode, MarketModeV16::Live);
    assert!(
        failed_asset_group.bankruptcy_hlock_active,
        "probe must activate the cross-market bankruptcy flag"
    );
    assert_eq!(env.portfolio_state(short_account).capital.get(), 0);
    assert_eq!(env.portfolio_state(short_account).pnl.get(), 0);

    let base_long = Keypair::new();
    let base_short = Keypair::new();
    let base_long_account = env.create_portfolio(&base_long);
    let base_short_account = env.create_portfolio(&base_short);
    env.deposit(&base_long, base_long_account, 1_000_000);
    env.deposit(&base_short, base_short_account, 1_000_000);
    env.svm.expire_blockhash();
    let base_trade = env.try_trade_asset_with_cu(
        0,
        &base_long,
        base_long_account,
        &base_short,
        base_short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        base_trade.is_ok(),
        "a permissionless asset's self-bankruptcy must not freeze unrelated base trading: {base_trade:?}"
    );
    let (_, final_group) = env.market_state();
    assert_eq!(final_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(final_group.assets[0].oi_eff_short_q, POS_SCALE);
}

#[test]
fn v16_program_public_asset_close_does_not_global_lock_unrelated_base_users() {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        live_counterparty_owner: base_long_owner,
        live_counterparty: base_long,
        live_peer_owner: base_short_owner,
        live_peer: base_short,
        ..
    } = public_asset1_bankrupt_close_fixture();
    assert!(close_progress(&env.portfolio_state(loss)).active);

    let idle_owner = Keypair::new();
    let idle = env.create_portfolio(&idle_owner);
    env.deposit(&idle_owner, idle, 100);
    let (idle_destination, withdraw_cu) = env.withdraw_with_cu(&idle_owner, idle, 100);
    assert_cu_within(
        "INV-074 unrelated flat withdrawal during asset close",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(idle_destination),
        100,
        "unrelated flat user can still withdraw while another asset has an active close"
    );

    let (_, before_exit) = env.market_state();
    assert_eq!(before_exit.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(before_exit.assets[0].oi_eff_short_q, POS_SCALE);
    let exit_cu = env
        .try_trade_asset_with_cu(
            0,
            &base_long_owner,
            base_long,
            &base_short_owner,
            base_short,
            -(POS_SCALE as i128),
            100,
            0,
        )
        .expect("asset-1 close ledger must not globally lock unrelated asset-0 exits");
    assert_cu_within(
        "INV-074 unrelated base exit during asset close",
        exit_cu,
        TRADE_CU_LIMIT,
    );
    let (_, after_exit) = env.market_state();
    assert_eq!(after_exit.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_exit.assets[0].oi_eff_short_q, 0);
    assert!(
        close_progress(&env.portfolio_state(loss)).active,
        "unrelated base operations must not cancel or rewrite the scoped close ledger"
    );
}

#[test]
fn v16_attack_permissionless_oracle_reconfiguration_preserves_unrelated_fee_and_exit_liveness() {
    let mut env =
        V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 1_000, 1_000, 500, 100);
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let creator = Keypair::new();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

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
        (100 * POS_SCALE) as i128,
        100,
        0,
    );
    let before = env.market_state().1;
    assert_eq!(before.assets[0].slot_last, 1);
    assert_eq!(before.slot_last, 1);
    assert_eq!(before.assets[1].oi_eff_long_q, 0);
    assert_eq!(before.pnl_pos_tot, 0);

    env.svm.warp_to_slot(100);
    let configure_cu = env.configure_auth_mark_for_asset_with_authority(1, &creator, 100, 200);
    let after_configure = env.market_state().1;
    assert_eq!(after_configure.current_slot, 100);
    assert_eq!(after_configure.slot_last, 100);
    assert_eq!(after_configure.assets[0].slot_last, 1);
    assert_cu_within(
        "permissionless cross-asset oracle reconfiguration",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let cap_before = env.portfolio_state(long).capital.get();
    let fee_slot_before = env.portfolio_state(long).last_fee_slot.get();
    let insurance_before = env.market_state().1.insurance;
    let sync_cu = env
        .try_sync_maintenance_fee_with_cu(long, None, 100)
        .expect("cross-asset fee sync remains live");
    let long_after = env.portfolio_state(long);
    let cap_after = long_after.capital.get();
    let insurance_after = env.market_state().1.insurance;
    assert_cu_within(
        "cross-asset loss-safe maintenance sync",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(cap_after, cap_before);
    assert_eq!(long_after.last_fee_slot.get(), fee_slot_before);
    assert_eq!(insurance_after, insurance_before);

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            -(100 * POS_SCALE as i128),
            100,
            0,
        )
        .expect("unrelated oracle reconfiguration must not block a signed risk-reducing exit");
    assert_cu_within("cross-asset stale-position exit", close_cu, TRADE_CU_LIMIT);
    let after_close = env.market_state().1;
    assert_eq!(after_close.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_close.assets[0].oi_eff_short_q, 0);
}

fn setup_permissionless_asset_base_stale_probe() -> (V16CuEnv, Keypair) {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.update_market_init_fee_policy_with_cu(1);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);
    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    (env, creator)
}

#[test]
fn v16_program_stale_permissionless_asset_cannot_global_resolve_market() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.update_market_init_fee_policy_with_cu(1);

    let creator = Keypair::new();
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let stale_long_account = env.create_portfolio(&stale_long_owner);
    let stale_short_account = env.create_portfolio(&stale_short_owner);
    env.deposit(&stale_long_owner, stale_long_account, 1_000_000);
    env.deposit(&stale_short_owner, stale_short_account, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &stale_long_owner,
        stale_long_account,
        &stale_short_owner,
        stale_short_account,
        (5 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    env.configure_permissionless_resolve_with_cu(5, 5);

    env.svm.warp_to_slot(6);
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 6 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_err(),
        "a stale permissionless asset must not globally resolve a fresh base market"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    let (_, group_after_reject) = env.market_state();
    assert_eq!(group_after_reject.mode, MarketModeV16::Live);
    assert!(group_after_reject.assets[1].slot_last < 6);

    let base_long_owner = Keypair::new();
    let base_short_owner = Keypair::new();
    let base_long_account = env.create_portfolio(&base_long_owner);
    let base_short_account = env.create_portfolio(&base_short_owner);
    env.deposit(&base_long_owner, base_long_account, 1_000_000);
    env.deposit(&base_short_owner, base_short_account, 1_000_000);
    let trade = env.try_trade_asset_with_cu(
        0,
        &base_long_owner,
        base_long_account,
        &base_short_owner,
        base_short_account,
        (5 * POS_SCALE) as i128,
        100,
        0,
    );
    assert!(
        trade.is_ok(),
        "unrelated base trade must remain live despite a stale permissionless asset: {trade:?}"
    );
}

#[test]
fn v16_program_non_base_slot_zero_profile_stale_rejects_trade() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(5, 5);

    env.activate_asset(1, 0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    assert_eq!(profile.last_good_oracle_slot, 0);

    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);

    env.svm.warp_to_slot(4);
    env.push_auth_mark_with_cu(4, 100);
    let fresh_base_resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(fresh_base_resolve.is_err());

    env.svm.warp_to_slot(6);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    env.svm.expire_blockhash();
    let stale_trade = env.send(
        env.trade_no_cpi_ix(taker_portfolio, lp_portfolio, 1, POS_SCALE as i128, 100, 0),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(lp_portfolio, false),
        ],
        &[&taker, &lp],
    );
    assert!(stale_trade.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_portfolio).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_portfolio).unwrap(), lp_before);

    env.svm.expire_blockhash();
    let base_trade = env.try_trade_asset_with_cu(
        0,
        &taker,
        taker_portfolio,
        &lp,
        lp_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        base_trade.is_ok(),
        "unrelated base trades remain live while only asset 1's local profile is stale: {base_trade:?}"
    );
}

#[test]
fn v16_program_permissionless_asset_oracle_cannot_block_base_resolve_matured() {
    let (mut control_env, _) = setup_permissionless_asset_base_stale_probe();
    control_env.svm.warp_to_slot(40);
    control_env.svm.expire_blockhash();
    let control_resolve = control_env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(control_env.market, false)],
        &[],
    );
    assert!(
        control_resolve.is_ok(),
        "base market is resolve-matured without permissionless-asset interference"
    );

    let (mut env, creator) = setup_permissionless_asset_base_stale_probe();
    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_asset_push = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 40,
            mark_e6: 102,
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&creator],
    );
    assert!(stale_asset_push.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(resolve.is_ok());
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_program_permissionless_asset_crank_rejects_after_base_resolve_matured() {
    let (mut env, creator) = setup_permissionless_asset_base_stale_probe();
    let cranked = env.create_portfolio(&Keypair::new());

    env.svm.warp_to_slot(7);
    env.push_auth_mark_for_asset_with_authority(1, &creator, 7, 101);
    env.svm.expire_blockhash();
    let fresh_crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 7,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cranked, false),
        ],
        &[],
    );
    assert!(fresh_crank.is_ok());

    env.svm.warp_to_slot(8);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&cranked).unwrap();
    env.svm.expire_blockhash();
    let stale_crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 8,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cranked, false),
        ],
        &[],
    );
    assert!(stale_crank.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&cranked).unwrap(), portfolio_before);

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(resolve.is_ok());
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_program_non_base_trade_rejects_after_base_resolve_matured() {
    let (mut env, creator) = setup_permissionless_asset_base_stale_probe();

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);

    env.svm.warp_to_slot(7);
    env.push_auth_mark_for_asset_with_authority(1, &creator, 7, 101);
    for portfolio in [taker_portfolio, lp_portfolio] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 7,
                observations: crank_observations(1),
            },
        );
    }
    let fresh_trade = env.try_trade_asset_with_cu(
        1,
        &taker,
        taker_portfolio,
        &lp,
        lp_portfolio,
        POS_SCALE as i128,
        101,
        0,
    );
    assert!(fresh_trade.is_ok());

    env.svm.warp_to_slot(8);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    env.svm.expire_blockhash();
    let stale_trade = env.send(
        env.trade_no_cpi_ix(taker_portfolio, lp_portfolio, 1, POS_SCALE as i128, 101, 0),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(lp_portfolio, false),
        ],
        &[&taker, &lp],
    );
    assert!(stale_trade.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_portfolio).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_portfolio).unwrap(), lp_before);

    env.resolve();
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_program_non_base_tradecpi_rejects_before_matcher_after_base_resolve_matured() {
    let (mut env, creator) = setup_permissionless_asset_base_stale_probe();
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp_portfolio,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, lp_portfolio, ctx, delegate, 1);

    let accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(lp_portfolio, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };
    let set_hostile_mode = |env: &mut V16CuEnv, mode: u8| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = mode;
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    };

    env.svm.warp_to_slot(7);
    env.push_auth_mark_for_asset_with_authority(1, &creator, 7, 101);
    set_hostile_mode(&mut env, 0);
    env.svm.expire_blockhash();
    let fresh_err = env
        .send(
            env.trade_cpi_ix(taker_portfolio, lp_portfolio, 1, POS_SCALE as i128, 100, 0),
            accounts(&env),
            &[&taker],
        )
        .expect_err("fresh hostile non-base TradeCpi must reach matcher-return validation");
    assert!(fresh_err.contains("InvalidAccountData"));
    assert!(!fresh_err.contains("Custom(27)") && !fresh_err.contains("0x1b"));

    env.svm.warp_to_slot(8);
    set_hostile_mode(&mut env, 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale_err = env
        .send(
            env.trade_cpi_ix(taker_portfolio, lp_portfolio, 1, POS_SCALE as i128, 100, 0),
            accounts(&env),
            &[&taker],
        )
        .expect_err("base-stale non-base TradeCpi must reject before matcher CPI");
    assert!(stale_err.contains("Custom(27)") || stale_err.contains("0x1b"));
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_portfolio).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_portfolio).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(resolve.is_ok());
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// security.md sweep — per-asset funding isolation (#33/#22): funding accruing on one asset (its mark
// premium) must NOT alter another asset's funding ledger. Asset 0's funding must leave asset 1's
// f_long_num/f_short_num unchanged.
#[test]
fn v16_attack_per_asset_funding_isolation() {
    const IP: u64 = 1_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: IP,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, IP, 1, 0);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: IP,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg ewma asset1");
    let lo = Keypair::new();
    let plo = env.create_portfolio(&lo);
    let sh = Keypair::new();
    let psh = env.create_portfolio(&sh);
    env.deposit(&lo, plo, 100_000_000);
    env.deposit(&sh, psh, 100_000_000);
    // balanced positions on BOTH assets.
    env.trade_with_cu(&lo, plo, &sh, psh, POS_SCALE as i128, IP, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &lo, plo, &sh, psh, POS_SCALE as i128, IP, 0);
    let a1_flong0 = env.market_state().1.assets[1].f_long_num;
    let a1_fshort0 = env.market_state().1.assets[1].f_short_num;
    // induce a mark premium and accrue funding on asset 0 ONLY.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, IP * 2); // asset 0 premium
    for slot in 1..=4u64 {
        env.svm.warp_to_slot(slot);
        for p in [plo, psh] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    let (_, g) = env.market_state();
    // asset 0 funding accrued; asset 1's funding ledger UNCHANGED.
    assert!(
        g.assets[0].f_long_num != 0,
        "asset 0 funding accrued (non-vacuous)"
    );
    assert_eq!(
        g.assets[1].f_long_num, a1_flong0,
        "asset 1 f_long_num UNCHANGED by asset-0 funding"
    );
    assert_eq!(
        g.assets[1].f_short_num, a1_fshort0,
        "asset 1 f_short_num unchanged"
    );
    assert_eq!(g.vault, 200_000_000, "vault conserved");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

#[test]
fn v16_bpf_stale_asset_does_not_block_current_unrelated_trade() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);

    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let stale_long_account = env.create_portfolio(&stale_long_owner);
    let stale_short_account = env.create_portfolio(&stale_short_owner);
    env.deposit(&stale_long_owner, stale_long_account, 1_000_000_000);
    env.deposit(&stale_short_owner, stale_short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &stale_long_owner,
        stale_long_account,
        &stale_short_owner,
        stale_short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.svm.warp_to_slot(3);

    for nonce in 0..3 {
        env.crank(
            cranker_portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3 + nonce,
                observations: crank_observations(0),
            },
        );
    }

    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(1),
        },
    );

    let (_, group) = env.market_state();
    assert_eq!(group.current_slot, 3);
    assert_eq!(group.assets[0].slot_last, 3);
    assert!(group.assets[1].slot_last < group.current_slot);
    assert!(
        group.loss_stale_active,
        "asset[1] partial catch-up must leave the market loss-stale bit set"
    );

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);

    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi current asset[0] with stale asset[1] CU: {trade_cu}");
    assert_cu_within(
        "TradeNoCpi current asset[0] with unrelated stale asset[1]",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].slot_last, 3);
    assert!(group_after.assets[1].slot_last < group_after.current_slot);
    assert!(
        group_after.loss_stale_active,
        "unrelated trade must not hide the stale asset state"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    assert!(has_active_leg_for_asset(&long, 0));
    assert!(has_active_leg_for_asset(&short, 0));
    assert!(!has_active_leg_for_asset(&long, 1));
    assert!(!has_active_leg_for_asset(&short, 1));
}
