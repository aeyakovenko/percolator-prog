//! INV-073 - No permanent user lock.
//!
//! Normative obligation: Every publicly reachable funded state has a finite public path to capital or terminal disposition.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_permissionless_asset_expired_close_matrix_discovers_global_recovery`, `v16_program_fragmented_recovery_pair_matrix_discovers_force_close_lock`, `v16_program_fractional_social_loss_exit_matrix_discovers_dust_lock`, `v16_program_recovery_residue_matrix_discovers_abandoned_owner_lock`, `v16_program_expired_partial_close_matrix_discovers_global_terminal_lock`, `v16_program_crossed_adl_effective_exit_matrix_discovers_zero_oi_residue_lock`, `v16_program_partial_adl_effective_exit_matrix_discovers_zero_oi_residue_lock`, `v16_attack_source_backed_force_close_preserves_bounded_resolved_exits`, `v16_probe_liquidation_then_shutdown_preserves_bounded_owner_exit`, `v16_attack_permissionless_close_resolved_survives_drained_owner_system_account`, `v16_attack_permissionless_asset_epoch_grief_has_atomic_max_leg_exit`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_permissionless_asset_expired_close_matrix_discovers_global_recovery() {
    for base_deposit in [1_000u128, 1_001] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 1,
            max_bankrupt_close_lifetime_slots: 2,
            public_b_chunk_atoms: 1,
            ..V16CuMarketParams::default()
        });
        env.configure_auth_mark_with_cu(0, 100);
        env.configure_permissionless_resolve_with_cu(100, 5);
        env.update_market_init_fee_policy_with_cu(1);

        let base_long_owner = Keypair::new();
        let base_short_owner = Keypair::new();
        let base_long = env.create_portfolio(&base_long_owner);
        let base_short = env.create_portfolio(&base_short_owner);
        env.deposit(&base_long_owner, base_long, base_deposit);
        env.deposit(&base_short_owner, base_short, base_deposit);
        env.trade_asset_with_cu(
            0,
            &base_long_owner,
            base_long,
            &base_short_owner,
            base_short,
            POS_SCALE as i128,
            100,
            0,
        );

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
        let attack_long_owner = Keypair::new();
        let attack_short_owner = Keypair::new();
        let attack_long = env.create_portfolio(&attack_long_owner);
        let attack_short = env.create_portfolio(&attack_short_owner);
        env.deposit(&attack_long_owner, attack_long, 10);
        env.deposit(&attack_short_owner, attack_short, 2);
        env.trade_asset_with_cu(
            1,
            &attack_long_owner,
            attack_long,
            &attack_short_owner,
            attack_short,
            (POS_SCALE / 50) as i128,
            100,
            0,
        );

        for (slot, mark) in [(2u64, 200u64), (3, 300)] {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_with_authority(1, &creator, slot, mark);
            env.crank(
                attack_long,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
            );
        }
        env.crank(
            attack_short,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations(1),
            },
        );
        assert_eq!(env.portfolio_state(attack_short).capital.get(), 0);
        assert!(env.portfolio_state(attack_short).pnl.get() < 0);

        env.svm.warp_to_slot(4);
        env.try_shutdown_asset_with_authority(&creator, 1, 4)
            .expect("permissionless creator shuts down only its own asset");
        env.forfeit_recovery_leg_with_cu(&attack_short_owner, attack_short, 1, 1);
        let ledger = close_progress(&env.portfolio_state(attack_short));
        assert!(ledger.active && ledger.residual_remaining > 0);
        assert_eq!(env.market_state().1.mode, MarketModeV16::Live);

        let expired_slot = ledger.max_close_slot + 1;
        env.svm.warp_to_slot(expired_slot);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(attack_short, false),
            ],
            &[],
        )
        .expect("expired local close chooses a public continuation");
        assert_eq!(env.market_state().1.mode, MarketModeV16::Recovery);

        let market_before = env.svm.get_account(&env.market).unwrap();
        let base_long_before = env.svm.get_account(&base_long).unwrap();
        let base_short_before = env.svm.get_account(&base_short).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let base_exit = env.try_trade_asset_with_cu(
            0,
            &base_long_owner,
            base_long,
            &base_short_owner,
            base_short,
            -(POS_SCALE as i128),
            100,
            0,
        );
        assert!(
            base_exit.is_err(),
            "global Recovery did not freeze the base exit"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&base_long).unwrap(), base_long_before);
        assert_eq!(env.svm.get_account(&base_short).unwrap(), base_short_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        assert!(has_active_leg_for_asset(&env.portfolio_state(base_long), 0));
        assert!(has_active_leg_for_asset(
            &env.portfolio_state(base_short),
            0
        ));
    }
}

#[test]
fn v16_program_fragmented_recovery_pair_matrix_discovers_force_close_lock() {
    const ASSET: u16 = 1;
    const OPEN_MARK: u64 = 100;
    const SHUTDOWN_MARK: u64 = 110;
    const SHUTDOWN_SLOT: u64 = 10;
    const FORCE_CLOSE_DELAY: u64 = 5;
    const FRAGMENTS: usize = 10;
    const FRAGMENT_Q: u128 = POS_SCALE;
    const LARGE_Q: u128 = FRAGMENTS as u128 * FRAGMENT_Q;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 5_000, 10_000, 1_000);
    env.configure_permissionless_resolve_with_cu(1_000, FORCE_CLOSE_DELAY);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 1, OPEN_MARK);

    let large_owner = Keypair::new();
    let large = env.create_portfolio(&large_owner);
    env.deposit(&large_owner, large, 1_000);
    let mut smalls = Vec::new();
    for _ in 0..FRAGMENTS {
        let owner = Keypair::new();
        let small = env.create_portfolio(&owner);
        env.deposit(&owner, small, 100);
        env.trade_asset_with_cu(
            ASSET,
            &large_owner,
            large,
            &owner,
            small,
            -(FRAGMENT_Q as i128),
            OPEN_MARK,
            0,
        );
        smalls.push(small);
    }

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.push_auth_mark_for_asset_as_admin(ASSET, SHUTDOWN_SLOT, SHUTDOWN_MARK);
    for portfolio in core::iter::once(large).chain(smalls.iter().copied()) {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: SHUTDOWN_SLOT,
                observations: crank_observations(ASSET),
            },
        );
    }
    let cert = health_cert(&env.portfolio_state(large));
    assert_eq!(cert.certified_liq_deficit, 0);
    assert!(
        cert.certified_equity >= cert.certified_maintenance_req as i128
            && cert.certified_equity < cert.certified_initial_req as i128
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(large), ASSET as usize).basis_pos_q,
        -(LARGE_Q as i128)
    );

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        ASSET,
        SHUTDOWN_SLOT,
        0,
    );
    let close_slot = SHUTDOWN_SLOT + FORCE_CLOSE_DELAY + 1;
    env.svm.warp_to_slot(close_slot);
    let cranker = Keypair::new();
    let mut successful_pairs = 0usize;
    for small in smalls.iter().copied() {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let large_before = env.svm.get_account(&large).unwrap();
        let small_before = env.svm.get_account(&small).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        match env.try_force_close_abandoned_asset_with_cu(
            &cranker, large, small, ASSET, close_slot, FRAGMENT_Q,
        ) {
            Ok(cu) => {
                assert_cu_within("fragmented Recovery pair close", cu, TRADE_CU_LIMIT);
                successful_pairs += 1;
            }
            Err(_) => {
                assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                assert_eq!(env.svm.get_account(&large).unwrap(), large_before);
                assert_eq!(env.svm.get_account(&small).unwrap(), small_before);
                assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                break;
            }
        }
    }
    assert!(successful_pairs < FRAGMENTS);
    assert!(has_active_leg_for_asset(
        &env.portfolio_state(large),
        ASSET as usize
    ));
    assert!(
        env.portfolio_state(large).capital.get() != 0 || env.portfolio_state(large).pnl.get() != 0
    );

    let remaining: Vec<_> = smalls
        .iter()
        .copied()
        .filter(|small| has_active_leg_for_asset(&env.portfolio_state(*small), ASSET as usize))
        .collect();
    assert!(!remaining.is_empty());
    for small in remaining {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let large_before = env.svm.get_account(&large).unwrap();
        let small_before = env.svm.get_account(&small).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let retry = env.try_force_close_abandoned_asset_with_cu(
            &cranker, large, small, ASSET, close_slot, FRAGMENT_Q,
        );
        assert!(
            retry.is_err(),
            "an alternate fragment unexpectedly progressed"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&large).unwrap(), large_before);
        assert_eq!(env.svm.get_account(&small).unwrap(), small_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }
    let group = env.market_state().1;
    assert!(group.assets[ASSET as usize].oi_eff_long_q != 0);
    assert_eq!(
        group.assets[ASSET as usize].oi_eff_long_q,
        group.assets[ASSET as usize].oi_eff_short_q
    );
}

#[test]
fn v16_program_fractional_social_loss_exit_matrix_discovers_dust_lock() {
    let mut params = V16CuMarketParams::default();
    params.initial_price = 1;
    params.max_price_move_bps_per_slot = 10_000;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_auth_mark_with_cu(0, 1);

    let l1o = Keypair::new();
    let l2o = Keypair::new();
    let l3o = Keypair::new();
    let l4o = Keypair::new();
    let s1o = Keypair::new();
    let s2o = Keypair::new();
    let s3o = Keypair::new();
    let s4o = Keypair::new();
    let l1 = env.create_portfolio(&l1o);
    let l2 = env.create_portfolio(&l2o);
    let l3 = env.create_portfolio(&l3o);
    let l4 = env.create_portfolio(&l4o);
    let s1 = env.create_portfolio(&s1o);
    let s2 = env.create_portfolio(&s2o);
    let s3 = env.create_portfolio(&s3o);
    let s4 = env.create_portfolio(&s4o);

    for (owner, portfolio, deposit) in [
        (&l1o, l1, 1_000),
        (&l2o, l2, 1_000),
        (&l3o, l3, 1_000),
        (&l4o, l4, 1_000),
        (&s1o, s1, 2),
        (&s2o, s2, 1_000),
        (&s3o, s3, 1_000),
        (&s4o, s4, 1_000),
    ] {
        env.deposit(owner, portfolio, deposit);
    }
    for (long_owner, long, short_owner, short) in [
        (&l1o, l1, &s1o, s1),
        (&l2o, l2, &s2o, s2),
        (&l3o, l3, &s3o, s3),
        (&l4o, l4, &s4o, s4),
    ] {
        env.trade_asset_with_cu(
            0,
            long_owner,
            long,
            short_owner,
            short,
            POS_SCALE as i128,
            1,
            0,
        );
    }

    for (slot, mark) in [(1u64, 2u64), (2, 3), (3, 4), (4, 5)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        env.crank(
            s2,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for portfolio in [s1, l1, l2, l3, l4, s2, s3, s4] {
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        );
    }
    for _ in 0..4 {
        if !has_active_leg_for_asset(&env.portfolio_state(s1), 0) {
            break;
        }
        env.crank(
            s1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: crank_observations(0),
            },
        );
    }
    let bankruptcy = env.market_state().1;
    assert_eq!(bankruptcy.mode, MarketModeV16::Live);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(s1), 0));
    assert_ne!(bankruptcy.assets[0].b_long_num, 0);

    for _ in 0..4 {
        let leg = active_leg_for_asset(&env.portfolio_state(l1), 0);
        if !leg.b_stale && leg.b_snap == env.market_state().1.assets[0].b_long_num {
            break;
        }
        env.crank(
            l1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(l1), 0).b_rem,
        percolator::SOCIAL_LOSS_DEN / 2
    );
    env.rebalance_reduce_with_cu(&l1o, l1, 0, POS_SCALE);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(l1), 0));
    assert_eq!(
        env.market_state().1.assets[0].social_loss_dust_long_num,
        percolator::SOCIAL_LOSS_DEN / 2
    );

    for _ in 0..4 {
        let leg = active_leg_for_asset(&env.portfolio_state(l2), 0);
        if !leg.b_stale && leg.b_snap == env.market_state().1.assets[0].b_long_num {
            break;
        }
        env.crank(
            l2,
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(l2), 0).b_rem,
        percolator::SOCIAL_LOSS_DEN / 2
    );

    let fixed_market = env.svm.get_account(&env.market).unwrap();
    let fixed_long = env.svm.get_account(&l2).unwrap();
    let fixed_short = env.svm.get_account(&s2).unwrap();
    let fixed_vault = env.svm.get_account(&env.vault).unwrap();
    for route in [0u8, 1] {
        env.svm.expire_blockhash();
        let result = if route == 0 {
            env.send(
                ProgInstruction::RebalanceReduce {
                    asset_index: 0,
                    reduce_q: POS_SCALE,
                },
                vec![
                    AccountMeta::new(l2o.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(l2, false),
                ],
                &[&l2o],
            )
        } else {
            env.try_trade_asset_with_cu(0, &l2o, l2, &s2o, s2, -(POS_SCALE as i128), 5, 0)
        };
        assert!(
            result.is_err(),
            "fractional carry exit route {route} progressed"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&l2).unwrap(), fixed_long);
        assert_eq!(env.svm.get_account(&s2).unwrap(), fixed_short);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
    }
    assert!(has_active_leg_for_asset(&env.portfolio_state(l2), 0));
    assert!(env.portfolio_state(l2).capital.get() != 0 || env.portfolio_state(l2).pnl.get() != 0);
}

#[test]
fn v16_program_recovery_residue_matrix_discovers_abandoned_owner_lock() {
    for loser_capital in [900u128, 901] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_permissionless_resolve_with_cu(10_000, 1);
        env.configure_auth_mark_with_cu(0, 100);
        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 1_000);
        env.deposit(&short_owner, short, loser_capital);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            (2 * POS_SCALE) as i128,
            100,
            0,
        );

        env.svm.warp_to_slot(6);
        env.push_auth_mark_with_cu(6, 500);
        for portfolio in [short, long] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 6,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
        env.crank_steps(
            short,
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
                observations: crank_observations(0),
            },
            2,
        );
        let matched_oi = env.market_state().1.assets[0].oi_eff_long_q;
        assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, matched_oi);
        assert!(matched_oi > 0 && matched_oi < 2 * POS_SCALE);
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(long), 0).basis_pos_q,
            (2 * POS_SCALE) as i128
        );

        env.svm.warp_to_slot(7);
        let admin = env.admin.insecure_clone();
        env.svm.expire_blockhash();
        env.try_shutdown_asset_with_authority(&admin, 0, 7)
            .expect("authenticated shutdown enters asset Recovery");
        env.svm.warp_to_slot(8);
        let cranker = Keypair::new();
        env.svm.expire_blockhash();
        env.try_force_close_abandoned_asset_with_cu(&cranker, long, short, 0, 8, 2 * POS_SCALE)
            .expect("first permissionless close consumes the matched quantity");

        let group = env.market_state().1;
        let long_state = env.portfolio_state(long);
        assert_eq!(group.assets[0].oi_eff_long_q, 0);
        assert_eq!(group.assets[0].oi_eff_short_q, 0);
        assert_eq!(
            active_leg_for_asset(&long_state, 0).basis_pos_q,
            (2 * POS_SCALE - matched_oi) as i128
        );
        assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
        assert!(long_state.capital.get() != 0 || long_state.pnl.get() != 0);

        for _ in 0..3 {
            let market_before = env.svm.get_account(&env.market).unwrap();
            let long_before = env.svm.get_account(&long).unwrap();
            let short_before = env.svm.get_account(&short).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            env.svm.expire_blockhash();
            let cleanup = env.try_force_close_abandoned_asset_with_cu(
                &cranker,
                long,
                short,
                0,
                8,
                percolator::MAX_VAULT_TVL,
            );
            assert!(
                cleanup.is_err(),
                "zero-OI Recovery residue unexpectedly detached"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&long).unwrap(), long_before);
            assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        }

        env.svm.expire_blockhash();
        let finalize_cu = env
            .send(
                ProgInstruction::FinalizeResetSide {
                    asset_index: 0,
                    side: 0,
                },
                vec![AccountMeta::new(env.market, false)],
                &[],
            )
            .expect("zero-OI side finalization is independently permissionless");
        assert_cu_within(
            "partial-ADL residue side finalization",
            finalize_cu,
            CUSTODY_CU_LIMIT,
        );
        assert!(has_active_leg_for_asset(&env.portfolio_state(long), 0));

        let market_after_finalize = env.svm.get_account(&env.market).unwrap();
        let long_after_finalize = env.svm.get_account(&long).unwrap();
        env.svm.expire_blockhash();
        let post_finalize_cleanup = env.try_force_close_abandoned_asset_with_cu(
            &cranker,
            long,
            short,
            0,
            8,
            percolator::MAX_VAULT_TVL,
        );
        assert!(post_finalize_cleanup.is_err());
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_after_finalize
        );
        assert_eq!(env.svm.get_account(&long).unwrap(), long_after_finalize);
        assert!(has_active_leg_for_asset(&env.portfolio_state(long), 0));
    }
}

#[test]
fn v16_program_expired_partial_close_matrix_discovers_global_terminal_lock() {
    for idle_capital in [100u128, 101] {
        let mut params = production_risk_params();
        params.max_bankrupt_close_lifetime_slots = 1;
        params.public_b_chunk_atoms = 8_000;
        let mut env = V16CuEnv::new_with_init_params(params);
        env.configure_permissionless_resolve_with_cu(100, 5);
        env.configure_auth_mark_with_cu(0, 1_000_000);

        let loss_owner = Keypair::new();
        let counterparty_owner = Keypair::new();
        let idle_owner = Keypair::new();
        let loss = env.create_portfolio(&loss_owner);
        let counterparty = env.create_portfolio(&counterparty_owner);
        let idle = env.create_portfolio(&idle_owner);
        env.deposit(&loss_owner, loss, 51_000);
        env.deposit(&counterparty_owner, counterparty, 100_000);
        env.deposit(&idle_owner, idle, idle_capital);
        env.trade_asset_with_cu(
            0,
            &loss_owner,
            loss,
            &counterparty_owner,
            counterparty,
            POS_SCALE as i128,
            1_000_000,
            0,
        );

        for (slot, mark) in [(20u64, 952_000u64), (25, 940_000), (30, 940_576)] {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_with_cu(slot, mark);
            env.crank(
                counterparty,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        env.update_asset_lifecycle_as_admin_with_cu(
            percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            0,
            30,
            0,
        );

        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::ForfeitRecoveryLeg {
                asset_index: 0,
                b_delta_budget: percolator::MAX_VAULT_TVL,
            },
            vec![
                AccountMeta::new(loss_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[&loss_owner],
        )
        .expect("one public recovery-forfeit chunk");
        let ledger = close_progress(&env.portfolio_state(loss));
        assert!(ledger.active && ledger.residual_remaining > 0);
        assert_eq!(env.market_state().1.mode, MarketModeV16::Live);

        env.svm.warp_to_slot(ledger.max_close_slot + 1);
        env.svm.expire_blockhash();
        env.send(
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
        .expect("expired close ledger chooses a terminal continuation");
        let (_, recovery) = env.market_state();
        assert_eq!(recovery.mode, MarketModeV16::Recovery);
        assert_eq!(recovery.vault as u64, env.token_amount(env.vault));

        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_idle = env.svm.get_account(&idle).unwrap();
        let fixed_vault = env.svm.get_account(&env.vault).unwrap();
        for _ in 0..3 {
            env.svm.expire_blockhash();
            let continuation = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(loss, false),
                ],
                &[],
            );
            if let Ok(cu) = continuation {
                assert_cu_within("Recovery fixed-point crank", cu, CRANK_CU_LIMIT);
            }
            assert_eq!(env.market_state().1.mode, MarketModeV16::Recovery);
            assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
            assert_eq!(env.svm.get_account(&idle).unwrap(), fixed_idle);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
        }

        let admin = env.admin.insecure_clone();
        env.svm.expire_blockhash();
        let authorized_resolve = env.send(
            ProgInstruction::ResolveMarket,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        );
        assert!(authorized_resolve.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&idle).unwrap(), fixed_idle);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);

        env.svm.warp_to_slot(200);
        env.svm.expire_blockhash();
        let stale_resolve = env.send(
            ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
            vec![AccountMeta::new(env.market, false)],
            &[],
        );
        assert!(stale_resolve.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&idle).unwrap(), fixed_idle);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);

        let idle_dest = env.token_account(idle_owner.pubkey(), 0);
        let idle_dest_before = env.svm.get_account(&idle_dest).unwrap();
        env.svm.expire_blockhash();
        let live_withdraw = env.send(
            ProgInstruction::Withdraw {
                amount: idle_capital,
            },
            vec![
                AccountMeta::new(idle_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(idle, false),
                AccountMeta::new(idle_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&idle_owner],
        );
        assert!(live_withdraw.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&idle).unwrap(), fixed_idle);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
        assert_eq!(env.svm.get_account(&idle_dest).unwrap(), idle_dest_before);

        env.svm.expire_blockhash();
        let resolved_close = env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(idle_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(idle, false),
                AccountMeta::new(idle_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&idle_owner],
        );
        assert!(resolved_close.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&idle).unwrap(), fixed_idle);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
        assert_eq!(env.svm.get_account(&idle_dest).unwrap(), idle_dest_before);
        assert_eq!(env.portfolio_state(idle).capital.get(), idle_capital);
    }
}

#[test]
fn v16_program_crossed_adl_effective_exit_matrix_discovers_zero_oi_residue_lock() {
    const PRICE: u64 = 1;
    const OPEN_Q: i128 = 13_000 * POS_SCALE as i128;

    for maintenance_fee_per_slot in [27u128, 28] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            h_max: 6_480_000,
            initial_price: PRICE,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 20,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 10_000_000,
            liquidation_fee_bps: 0,
            maintenance_fee_per_slot,
            ..V16CuMarketParams::default()
        });
        let survivor_owner = Keypair::new();
        let counterparty_owner = Keypair::new();
        let survivor = env.create_portfolio(&survivor_owner);
        let counterparty = env.create_portfolio(&counterparty_owner);
        env.deposit(&survivor_owner, survivor, 1_000);
        env.deposit(&counterparty_owner, counterparty, 1_189);

        env.svm.warp_to_slot(8);
        env.trade_asset_with_cu(
            0,
            &survivor_owner,
            survivor,
            &counterparty_owner,
            counterparty,
            OPEN_Q,
            PRICE,
            0,
        );
        env.svm.warp_to_slot(27);
        env.crank(
            survivor,
            ProgInstruction::PermissionlessCrank {
                now_slot: 27,
                observations: crank_observations(0),
            },
        );
        env.svm.warp_to_slot(35);
        env.sync_maintenance_fee_with_cu(counterparty, None, 35);
        env.crank_steps(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: 35,
                observations: crank_observations(0),
            },
            2,
        );

        let before = env.market_state().1;
        let effective_q = before.assets[0].oi_eff_long_q;
        let stored_q = active_leg_for_asset(&env.portfolio_state(survivor), 0)
            .basis_pos_q
            .unsigned_abs();
        assert_eq!(before.assets[0].oi_eff_short_q, effective_q);
        assert!(effective_q > 0 && stored_q > effective_q);

        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &counterparty_owner,
            counterparty,
            &survivor_owner,
            survivor,
            effective_q as i128,
            PRICE,
            0,
        )
        .expect("exact-effective crossed reduction");

        let crossed = env.market_state().1;
        let residual = active_leg_for_asset(&env.portfolio_state(survivor), 0)
            .basis_pos_q
            .unsigned_abs();
        assert_eq!(crossed.assets[0].oi_eff_long_q, 0);
        assert_eq!(crossed.assets[0].oi_eff_short_q, 0);
        assert_eq!(crossed.assets[0].mode_long, SideModeV16::Normal);
        assert!(residual > 0);
        assert!(env.portfolio_state(survivor).capital.get() > 0);

        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_survivor = env.svm.get_account(&survivor).unwrap();
        for _ in 0..4 {
            env.svm.expire_blockhash();
            let cu = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 35,
                        observations: vec![],
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(survivor, false),
                    ],
                    &[],
                )
                .expect("crossed zero-OI residue crank");
            assert_cu_within("crossed zero-OI residue crank", cu, CRANK_CU_LIMIT);
            assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
            assert_eq!(env.svm.get_account(&survivor).unwrap(), fixed_survivor);
        }

        env.svm.expire_blockhash();
        let owner_exit = env.send(
            ProgInstruction::RebalanceReduce {
                asset_index: 0,
                reduce_q: residual,
            },
            vec![
                AccountMeta::new(survivor_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(survivor, false),
            ],
            &[&survivor_owner],
        );
        assert!(owner_exit.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&survivor).unwrap(), fixed_survivor);

        let counterparty_before = env.svm.get_account(&counterparty).unwrap();
        env.svm.expire_blockhash();
        let matched_exit = env.try_trade_asset_with_cu(
            0,
            &counterparty_owner,
            counterparty,
            &survivor_owner,
            survivor,
            residual as i128,
            PRICE,
            0,
        );
        assert!(matched_exit.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&survivor).unwrap(), fixed_survivor);
        assert_eq!(
            env.svm.get_account(&counterparty).unwrap(),
            counterparty_before
        );
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
    }
}

#[test]
fn v16_program_partial_adl_effective_exit_matrix_discovers_zero_oi_residue_lock() {
    for long_capital in [1_000u128, 1_001] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_auth_mark_with_cu(0, 100);
        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, long_capital);
        env.deposit(&short_owner, short, 900);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            (2 * POS_SCALE) as i128,
            100,
            0,
        );

        env.svm.warp_to_slot(6);
        env.push_auth_mark_with_cu(6, 500);
        for portfolio in [short, long] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 6,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
        let _ = env.crank_steps(
            short,
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
                observations: crank_observations(0),
            },
            2,
        );

        let (_, adl) = env.market_state();
        let effective_exit_q = adl.assets[0].oi_eff_long_q;
        assert!(effective_exit_q > 0 && effective_exit_q < 2 * POS_SCALE);
        assert_eq!(adl.assets[0].oi_eff_short_q, effective_exit_q);
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(long), 0).basis_pos_q,
            (2 * POS_SCALE) as i128,
            "ADL must leave stored basis larger than current effective OI"
        );

        let reduction_cu = env.rebalance_reduce_with_cu(&long_owner, long, 0, effective_exit_q);
        assert_cu_within(
            "partial-ADL exact-effective owner reduction",
            reduction_cu,
            CUSTODY_CU_LIMIT,
        );
        let (_, reduced) = env.market_state();
        assert_eq!(reduced.assets[0].oi_eff_long_q, 0);
        assert_eq!(reduced.assets[0].oi_eff_short_q, 0);

        let residual_before = active_leg_for_asset(&env.portfolio_state(long), 0).basis_pos_q;
        assert!(residual_before > 0);
        assert_eq!(reduced.assets[0].mode_long, SideModeV16::Normal);
        assert_eq!(reduced.assets[0].stored_pos_count_long, 1);
        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_long = env.svm.get_account(&long).unwrap();
        for _ in 0..4 {
            env.svm.expire_blockhash();
            let crank = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 6,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(long, false),
                    ],
                    &[],
                )
                .expect("honest crank reaches the zero-OI residue fixed point");
            assert_cu_within("zero-OI residue crank", crank, CRANK_CU_LIMIT);
            assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
            assert_eq!(env.svm.get_account(&long).unwrap(), fixed_long);
        }

        env.svm.expire_blockhash();
        let owner_retry = env.send(
            ProgInstruction::RebalanceReduce {
                asset_index: 0,
                reduce_q: residual_before.unsigned_abs(),
            },
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[&long_owner],
        );
        assert!(
            owner_retry.is_err(),
            "zero-OI owner retry unexpectedly detached"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&long).unwrap(), fixed_long);

        let fresh_owner = Keypair::new();
        let fresh = env.create_portfolio(&fresh_owner);
        env.deposit(&fresh_owner, fresh, 10_000);
        let trade_market = env.svm.get_account(&env.market).unwrap();
        let trade_long = env.svm.get_account(&long).unwrap();
        let trade_fresh = env.svm.get_account(&fresh).unwrap();
        env.svm.expire_blockhash();
        let matched_exit = env.try_trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &fresh_owner,
            fresh,
            -residual_before,
            500,
            0,
        );
        assert!(
            matched_exit.is_err(),
            "fresh willing counterparty unexpectedly detached zero-OI residue"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), trade_market);
        assert_eq!(env.svm.get_account(&long).unwrap(), trade_long);
        assert_eq!(env.svm.get_account(&fresh).unwrap(), trade_fresh);

        let locked = env.portfolio_state(long);
        assert!(locked.capital.get() != 0 || locked.pnl.get() != 0);
        assert_eq!(
            active_leg_for_asset(&locked, 0).basis_pos_q,
            residual_before
        );
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
    }
}

#[test]
fn v16_attack_source_backed_force_close_preserves_bounded_resolved_exits() {
    const PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const TOO_LARGE_INCREASE_Q: i128 = 30 * POS_SCALE as i128;
    const SHUTDOWN_SLOT: u64 = 4;
    const FORCE_CLOSE_DELAY: u64 = 5;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);

    let winner_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&winner_owner, winner, 313);
    env.deposit(&counterparty_owner, counterparty, 1_000);
    env.top_up_backing_bucket(1, 150, 10);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        ASSET0_SIZE_Q,
        PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        ASSET1_SIZE_Q,
        PRICE,
        0,
    );

    // Create source-backed positive PnL on asset 0, then use it as initial
    // margin for a public risk increase on the losing asset-1 leg.
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [(counterparty, 0), (winner, 0), (counterparty, 1)] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
    }

    let watermark_dest = env.token_account(env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(watermark_dest, 1, 50);
    assert!(
        env.try_trade_asset_with_cu(
            1,
            &winner_owner,
            winner,
            &counterparty_owner,
            counterparty,
            TOO_LARGE_INCREASE_Q,
            95,
            0,
        )
        .is_err(),
        "source-credit watermark control must reject the oversized increase"
    );
    env.svm.warp_to_slot(3);
    env.trade_asset_with_cu(
        1,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        SAFE_INCREASE_Q,
        95,
        0,
    );

    let winner_before_shutdown = env.portfolio_state(winner);
    let lien_before = winner_before_shutdown
        .source_domains
        .iter()
        .find(|source| source.source_lien_counterparty_backing_num.get() != 0);
    assert!(
        lien_before.is_some(),
        "winner must hold a counterparty-backed source lien; pnl={} capital={}",
        winner_before_shutdown.pnl.get(),
        winner_before_shutdown.capital.get(),
    );
    let lien_before = lien_before.unwrap();
    assert!(winner_before_shutdown.pnl.get() > 0);
    assert!(lien_before.source_claim_liened_num.get() > 0);
    assert!(lien_before.source_lien_counterparty_backing_num.get() > 0);
    assert_eq!(
        env.market_state().1.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    env.configure_permissionless_resolve_with_cu(100, FORCE_CLOSE_DELAY);

    // A post-init asset admin may shut down the market, but that action must
    // preserve bounded owner/keeper exits for users already holding claims.
    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );
    let (_, recovery) = env.market_state();
    assert_eq!(recovery.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(recovery.assets[1].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(
        recovery.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );

    let cranker = Keypair::new();
    let force_slot = SHUTDOWN_SLOT + FORCE_CLOSE_DELAY + 1;
    env.svm.warp_to_slot(force_slot);
    env.force_close_abandoned_asset_with_cu(
        &cranker,
        winner,
        counterparty,
        0,
        force_slot,
        ASSET0_SIZE_Q.unsigned_abs(),
    );
    env.force_close_abandoned_asset_with_cu(
        &cranker,
        winner,
        counterparty,
        1,
        force_slot,
        (ASSET1_SIZE_Q + SAFE_INCREASE_Q).unsigned_abs(),
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(winner)
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(counterparty)
    )));
    let winner_after_force = env.portfolio_state(winner);
    let forced_lien = winner_after_force
        .source_domains
        .iter()
        .find(|source| source.source_lien_counterparty_backing_num.get() != 0)
        .expect("force-close must preserve the winner's account-local lien until settlement");
    let (_, forced_market) = env.market_state();
    let forced_domain = forced_lien.domain.get() as usize;
    assert_eq!(
        forced_market.source_backing_buckets[forced_domain].valid_liened_backing_num,
        forced_lien.source_lien_counterparty_backing_num.get(),
        "the public force-close setup must retain a real aggregate-backed source lien",
    );

    env.resolve();
    let winner_dest = env.token_account(winner_owner.pubkey(), 0);
    let counterparty_dest = env.token_account(counterparty_owner.pubkey(), 0);
    let winner_tokens_before = env.token_amount(winner_dest);
    let try_close = |env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey, dest: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    };

    let is_terminal = |state: &PortfolioAccountV16| {
        state.capital.get() == 0
            && state.pnl.get() == 0
            && percolator::active_bitmap_is_empty(active_bitmap(state))
            && state
                .source_domains
                .iter()
                .all(|source| source.source_claim_bound_num.get() == 0)
            && state.stale_state == 0
            && state.b_stale_state == 0
    };
    let mut max_close_cu = 0;
    let mut close_rounds = 0;
    for round in 0..8 {
        let counterparty_cu = try_close(
            &mut env,
            &counterparty_owner,
            counterparty,
            counterparty_dest,
        )
        .expect("counterparty resolved close must make bounded progress");
        let winner_cu = try_close(&mut env, &winner_owner, winner, winner_dest)
            .expect("source-backed winner resolved close must make bounded progress");
        max_close_cu = max_close_cu.max(counterparty_cu).max(winner_cu);
        close_rounds = round + 1;
        if is_terminal(&env.portfolio_state(winner))
            && is_terminal(&env.portfolio_state(counterparty))
        {
            break;
        }
    }

    let winner_after = env.portfolio_state(winner);
    let counterparty_after = env.portfolio_state(counterparty);
    let winner_tokens_after = env.token_amount(winner_dest);
    assert!(
        is_terminal(&winner_after),
        "source-backed shutdown must not strand the independent winner; \
         capital={} pnl={} active={:?} rounds={close_rounds}",
        winner_after.capital.get(),
        winner_after.pnl.get(),
        active_bitmap(&winner_after),
    );
    assert!(
        is_terminal(&counterparty_after),
        "source-backed shutdown must not strand the counterparty; \
         capital={} pnl={} active={:?} rounds={close_rounds}",
        counterparty_after.capital.get(),
        counterparty_after.pnl.get(),
        active_bitmap(&counterparty_after),
    );
    assert!(
        winner_tokens_after > winner_tokens_before,
        "resolved close must transfer the winner's withdrawable value"
    );
    assert_cu_within(
        "source-backed force-close resolved wind-down",
        max_close_cu,
        CUSTODY_CU_LIMIT,
    );
}

#[test]
fn v16_probe_liquidation_then_shutdown_preserves_bounded_owner_exit() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.configure_permissionless_resolve_with_cu(100, 2);

    let winner = Keypair::new();
    let winner_portfolio = env.create_portfolio(&winner);
    let loser = Keypair::new();
    let loser_portfolio = env.create_portfolio(&loser);
    let idle = Keypair::new();
    let idle_portfolio = env.create_portfolio(&idle);
    env.deposit(&winner, winner_portfolio, 5_000_000);
    env.deposit(&loser, loser_portfolio, 250);
    env.deposit(&idle, idle_portfolio, 5_000_000);
    env.trade_with_cu(
        &winner,
        winner_portfolio,
        &loser,
        loser_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loser_portfolio, false),
            ],
            &[],
        );
    }
    env.crank(
        loser_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        3,
        0,
    );

    for (owner, portfolio) in [
        (&winner, winner_portfolio),
        (&loser, loser_portfolio),
        (&idle, idle_portfolio),
    ] {
        if has_active_leg_for_asset(&env.portfolio_state(portfolio), 0) {
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::ForfeitRecoveryLeg {
                    asset_index: 0,
                    b_delta_budget: u128::MAX,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            )
            .expect("every Recovery survivor has a bounded owner forfeit path");
        }
    }

    for portfolio in [winner_portfolio, loser_portfolio, idle_portfolio] {
        assert!(
            percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(portfolio))),
            "shutdown exit must clear every remaining leg"
        );
    }

    env.resolve();
    env.svm.warp_to_slot(6);
    let mut total_payout = 0u128;
    for (owner, portfolio) in [
        (&winner, winner_portfolio),
        (&loser, loser_portfolio),
        (&idle, idle_portfolio),
    ] {
        for _ in 0..8 {
            let state = env.portfolio_state(portfolio);
            let receipt = resolved_receipt(&state);
            if state.capital.get() == 0
                && state.pnl.get() == 0
                && state.reserved_pnl.get() == 0
                && (!receipt.present || receipt.finalized)
            {
                break;
            }
            let (dest, _) = env.close_resolved_with_cu(owner, portfolio);
            total_payout += env.token_amount(dest) as u128;
        }
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        assert_eq!(state.capital.get(), 0, "resolved capital must be payable");
        assert_eq!(state.pnl.get(), 0, "resolved PnL must settle");
        assert_eq!(state.reserved_pnl.get(), 0, "no payout may remain reserved");
        assert!(
            !receipt.present || receipt.finalized,
            "resolved payout receipt must finish in bounded calls"
        );
    }
    let (_, group) = env.market_state();
    assert_eq!(
        total_payout + group.vault,
        10_000_250,
        "all deposited collateral must remain in payouts or the engine vault"
    );
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "engine and SPL custody must remain synchronized"
    );
}

#[test]
fn v16_attack_permissionless_close_resolved_survives_drained_owner_system_account() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let owner_dest = env.token_account(owner.pubkey(), 0);
    env.resolve();
    env.svm.warp_to_slot(EXIT_DELAY + 1);

    let owner_lamports = env.svm.get_account(&owner.pubkey()).unwrap().lamports;
    env.svm.expire_blockhash();
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![system_instruction::transfer(
            &owner.pubkey(),
            &env.payer.pubkey(),
            owner_lamports,
        )],
        &[&owner],
    )
    .expect("owner can publicly drain its system-account lamports");
    assert_eq!(
        env.svm
            .get_account(&owner.pubkey())
            .map(|account| account.lamports)
            .unwrap_or(0),
        0,
        "probe starts after the owner system account is no longer funded"
    );

    env.svm.expire_blockhash();
    let permissionless = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(owner_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("post-timeout CloseResolved should not depend on owner lamports");
    assert_cu_within(
        "post-timeout CloseResolved drained owner account",
        permissionless,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(owner_dest),
        1_000,
        "permissionless close still pays the portfolio owner's token account"
    );
    assert_eq!(env.token_amount(env.vault), 0);
}

#[test]
fn v16_attack_permissionless_asset_epoch_grief_has_atomic_max_leg_exit() {
    const LEGS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const ATTACK_ASSET: u16 = LEGS;
    const PRICE: u64 = 100;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: LEGS,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
        LEGS as usize + 1,
    );
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(1);
    for asset_index in 0..LEGS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, PRICE);
    }

    let attacker = Keypair::new();
    env.svm.warp_to_slot(2);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        ATTACK_ASSET,
        2,
        PRICE,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(ATTACK_ASSET, &attacker, 2, PRICE);

    let attack_long_owner = Keypair::new();
    let attack_short_owner = Keypair::new();
    let attack_long = env.create_portfolio(&attack_long_owner);
    let attack_short = env.create_portfolio(&attack_short_owner);
    env.deposit(&attack_long_owner, attack_long, 10_000);
    env.deposit(&attack_short_owner, attack_short, 10_000);
    env.trade_asset_with_cu(
        ATTACK_ASSET,
        &attack_long_owner,
        attack_long,
        &attack_short_owner,
        attack_short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    let open_legs: Vec<BatchTradeLeg> = (0..LEGS)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            size_q: POS_SCALE as i128,
            exec_price: PRICE,
            fee_bps: 0,
        })
        .collect();
    env.send(
        ProgInstruction::BatchTradeNoCpi { legs: open_legs },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("public 14-leg setup trade");
    let cert_epoch = health_cert(&env.portfolio_state(long)).cert_oracle_epoch;
    assert_eq!(cert_epoch, env.market_state().1.oracle_epoch);

    env.svm.warp_to_slot(3);
    env.send(
        ProgInstruction::PushAuthMark {
            asset_index: ATTACK_ASSET,
            now_slot: 3,
            mark_e6: 200,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&attacker],
    )
    .expect("attacker updates its permissionless mark");
    env.crank(
        attack_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(ATTACK_ASSET),
        },
    );
    assert!(env.market_state().1.oracle_epoch > cert_epoch);
    let custody_before_exit = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let stale_exit = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        PRICE,
        0,
    );
    let stale_err = stale_exit.expect_err("unrelated target update invalidates max-leg exit certs");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "expected EngineStale, got {stale_err}"
    );

    let program_id = env.program_id;
    let market = env.market;
    let payer = env.payer.pubkey();
    let crank_ix = |portfolio| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        }
        .encode(),
    };
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_context, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &short_owner, short);
    let exit_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(matcher_context, false),
            AccountMeta::new_readonly(matcher_delegate, false),
        ],
        data: ProgInstruction::TradeCpi {
            asset_index: 0,
            size_q: -(POS_SCALE as i128),
            fee_bps: 0,
            limit_price: PRICE,
        }
        .encode(),
    };
    env.svm.expire_blockhash();
    let atomic_cu = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), crank_ix(long), crank_ix(short), exit_ix],
        &[&long_owner],
    )
    .expect("atomic recovery path must fit one transaction");
    assert!(
        atomic_cu < 1_400_000,
        "max-leg atomic refresh+exit consumed {atomic_cu} CU"
    );

    let after = env.market_state().1;
    assert_eq!(after.assets[0].oi_eff_long_q, 0);
    assert_eq!(after.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    assert_eq!(env.token_amount(env.vault), custody_before_exit);
    assert_eq!(after.vault as u64, custody_before_exit);
}
