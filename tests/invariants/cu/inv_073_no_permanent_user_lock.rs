//! INV-073 - No permanent user lock.
//!
//! Normative obligation: Every publicly reachable funded state has a finite public path to capital or terminal disposition.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): recovery and terminal-exit
//! matrices (`v16_program_asset0_recovery_matrix_discovers_provider_and_restart_lock`,
//! `v16_program_winner_first_recovery_matrix_discovers_provider_lien_lock`,
//! `v16_program_permissionless_asset_expired_close_matrix_discovers_global_recovery`,
//! `v16_program_fragmented_recovery_pair_matrix_clears_every_fragment`,
//! `v16_program_fractional_social_loss_exit_matrix_discovers_dust_lock`,
//! `v16_program_recovery_residue_matrix_clears_abandoned_owner_residue`,
//! `v16_program_expired_partial_close_matrix_resolves_and_preserves_idle_exit`,
//! `v16_program_funding_disabled_round_trip_mark_preserves_stale_terminal_progress`), and
//! resolve-matured freeze regressions for policy changes, value-in/value-out, released-PnL
//! conversion, maintenance fee sync, recovery forfeit, and rebalance-reduce. The two ADL zero-OI
//! helpers are executed and owned by INV-051. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_funding_disabled_round_trip_mark_preserves_stale_terminal_progress() {
    const PRICE: u64 = 100;
    const HIGH_MARK: u64 = 101;
    const RESOLVE_SLOT: u64 = 103;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);

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
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, HIGH_MARK);
    env.svm.warp_to_slot(3);
    env.push_auth_mark_for_asset_as_admin(0, 3, PRICE);

    // Advance the engine's global cursor through an unrelated asset. The first asset now retains
    // a historical pending checkpoint that cannot be replayed at its original slot, but its latest
    // authenticated endpoint equals the official price and funding is disabled, so net K/F is zero.
    env.svm.warp_to_slot(10);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(1),
        },
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, before) = state::read_market(&market_data).unwrap();
    let profile = state::read_asset_oracle_profile(&market_data, 0).unwrap();
    assert_eq!(before.config.max_abs_funding_e9_per_slot, 0);
    assert_eq!(before.assets[0].effective_price, PRICE);
    assert_eq!(profile.mark_ewma_e6, PRICE);
    assert_ne!(profile.funding_mark_pending_e6, 0);
    assert!(before.current_slot > profile.funding_mark_pending_slot);

    env.svm.warp_to_slot(RESOLVE_SLOT);
    env.svm.expire_blockhash();
    let resolve_cu = env
        .send(
            ProgInstruction::ResolveStalePermissionless {
                now_slot: RESOLVE_SLOT,
            },
            vec![AccountMeta::new(env.market, false)],
            &[],
        )
        .expect("net-zero obsolete checkpoint must not lock stale resolution");
    assert!(resolve_cu < 1_400_000);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    assert_eq!(env.market_state().1.assets[0].effective_price, PRICE);

    env.svm.warp_to_slot(RESOLVE_SLOT + 1);
    let long_destination = env.close_resolved(&long_owner, long);
    let short_destination = env.close_resolved(&short_owner, short);
    assert_eq!(env.token_amount(long_destination), 1_000_000);
    assert_eq!(env.token_amount(short_destination), 1_000_000);
}

#[test]
fn v16_program_asset0_recovery_matrix_discovers_provider_and_restart_lock() {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: 1_000,
        min_nonzero_mm_req: 599,
        min_nonzero_im_req: 600,
        maintenance_margin_bps: 5_000,
        initial_margin_bps: 5_000,
        max_price_move_bps_per_slot: 4_900,
        max_bankrupt_close_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    let marketauth = env.admin.insecure_clone();
    let backing_provider = Keypair::new();
    env.configure_permissionless_resolve_with_cu(1_000, 1);
    env.configure_auth_mark_with_cu(0, 1_000);
    env.try_update_per_asset_authority_with_cu(
        &marketauth,
        Some(&backing_provider),
        0,
        processor::ASSET_AUTH_BACKING_BUCKET,
        backing_provider.pubkey().to_bytes(),
    )
    .expect("record external backing provider");
    env.top_up_insurance(2);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&long_owner, long, 600);
    env.deposit(&short_owner, short, 600);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 399);
    for slot in [1u64, 2] {
        env.svm.warp_to_slot(slot);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for _ in 0..5 {
        env.crank(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
    }
    let bankrupt = env.market_state().1;
    assert!(bankrupt.bankruptcy_hlock_active);
    assert_eq!(bankrupt.insurance_domain_budget_remaining_total, 1);
    let recovered_principal =
        bankrupt.source_backing_buckets[0].fresh_unliened_backing_num / BOUND_SCALE;
    assert!(recovered_principal > 0);

    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 2, 0);
    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
        for _ in 0..8 {
            if percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(portfolio))) {
                break;
            }
            env.forfeit_recovery_leg_with_cu(owner, portfolio, 0, percolator::MAX_VAULT_TVL);
        }
    }
    for side in [0u8, 1] {
        env.finalize_reset_side_with_cu(0, side);
    }
    let recovered = env.market_state().1;
    assert_eq!(recovered.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(recovered.assets[0].oi_eff_long_q, 0);
    assert_eq!(recovered.assets[0].oi_eff_short_q, 0);
    assert_eq!(recovered.assets[0].stored_pos_count_long, 0);
    assert_eq!(recovered.assets[0].stored_pos_count_short, 0);
    let principal = recovered.source_backing_buckets[0].fresh_unliened_backing_num / BOUND_SCALE;
    assert_eq!(principal, recovered_principal);

    env.svm.warp_to_slot(3);
    let provider_destination = env.token_account(backing_provider.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let destination_before = env.svm.get_account(&provider_destination).unwrap();
    let market_id = recovered.assets[0].market_id;
    for _ in 0..16 {
        env.svm.expire_blockhash();
        let withdrawal = env.send(
            ProgInstruction::WithdrawBackingBucket {
                domain: 0,
                market_id,
                amount: principal,
            },
            vec![
                AccountMeta::new(backing_provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(provider_destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&backing_provider],
        );
        assert!(withdrawal.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        assert_eq!(
            env.svm.get_account(&provider_destination).unwrap(),
            destination_before
        );
    }
    assert_eq!(env.token_amount(provider_destination), 0);

    env.svm.expire_blockhash();
    let restart = env.send(
        ProgInstruction::RestartAssetOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 3,
            initial_price: 1_000,
            observation_sequence: u64::MAX,
        },
        vec![
            AccountMeta::new(marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&marketauth],
    );
    assert!(restart.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num / BOUND_SCALE,
        principal
    );
}

#[test]
fn v16_program_winner_first_recovery_matrix_discovers_provider_lien_lock() {
    const PRICE: u64 = 1_000_000;
    const ASSET: u16 = 1;
    const SOURCE_DOMAIN: u16 = ASSET * 2;
    const BACKING: u128 = 100_000;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    params.public_b_chunk_atoms = 8_000;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 0, PRICE);

    let loser_owner = Keypair::new();
    let winner_owner = Keypair::new();
    let base_peer_owner = Keypair::new();
    let loser = env.create_portfolio(&loser_owner);
    let winner = env.create_portfolio(&winner_owner);
    let base_peer = env.create_portfolio(&base_peer_owner);
    env.deposit(&loser_owner, loser, 51_000);
    env.deposit(&winner_owner, winner, 100_000);
    env.deposit(&base_peer_owner, base_peer, 100_000);
    env.top_up_backing_bucket(SOURCE_DOMAIN, BACKING, 10_000);
    env.trade_asset_with_cu(
        ASSET,
        &loser_owner,
        loser,
        &winner_owner,
        winner,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(20);
    env.push_auth_mark_for_asset_as_admin(ASSET, 20, 952_000);
    env.crank(
        winner,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations(ASSET),
        },
    );
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &base_peer_owner,
        base_peer,
        POS_SCALE as i128 * 3 / 2,
        PRICE,
        0,
    );
    let lien_before =
        state::portfolio_source_domain(&env.portfolio_state(winner), SOURCE_DOMAIN as usize);
    assert!(lien_before.source_lien_counterparty_backing_num.get() > 0);

    for (slot, mark) in [(25, 940_000), (30, 940_576)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(ASSET, slot, mark);
        env.crank(
            winner,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(ASSET),
            },
        );
    }
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, ASSET, 30, 0);
    env.forfeit_recovery_leg_with_cu(&winner_owner, winner, ASSET, u128::MAX);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(winner),
        ASSET as usize
    ));

    let market_before = env.svm.get_account(&env.market).unwrap();
    let loser_before = env.svm.get_account(&loser).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    for _ in 0..16 {
        env.svm.expire_blockhash();
        let forfeit = env.send(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: env.portfolio_id(loser),
                position_epoch: env.portfolio_position_epoch(loser),
                asset_index: ASSET,
                b_delta_budget: u128::MAX,
            },
            vec![
                AccountMeta::new(loser_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loser, false),
            ],
            &[&loser_owner],
        );
        assert!(
            forfeit.is_err(),
            "the loser unexpectedly escaped the fixed point"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&loser).unwrap(), loser_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }

    let loser_after = env.portfolio_state(loser);
    assert!(has_active_leg_for_asset(&loser_after, ASSET as usize));
    let market_after = env.market_state().1;
    let bucket = &market_after.source_backing_buckets[SOURCE_DOMAIN as usize];
    assert!(
        bucket.valid_liened_backing_num != 0 || bucket.impaired_liened_backing_num != 0,
        "the failed loser continuation retained no provider encumbrance"
    );
}

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
fn v16_program_fragmented_recovery_pair_matrix_clears_every_fragment() {
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
    env.crank_steps_after_market_catchup(
        large,
        ProgInstruction::PermissionlessCrank {
            now_slot: SHUTDOWN_SLOT,
            observations: crank_observations(ASSET),
        },
        1,
    );
    for portfolio in smalls.iter().copied() {
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
    assert_eq!(successful_pairs, FRAGMENTS);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(large),
        ASSET as usize
    ));
    assert!(smalls
        .iter()
        .all(|small| !has_active_leg_for_asset(&env.portfolio_state(*small), ASSET as usize)));
    let group = env.market_state().1;
    assert_eq!(group.assets[ASSET as usize].oi_eff_long_q, 0);
    assert_eq!(group.assets[ASSET as usize].oi_eff_short_q, 0);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
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
                    portfolio_id: env.portfolio_id(l2),
                    position_epoch: env.portfolio_position_epoch(l2),
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
fn v16_program_recovery_residue_matrix_clears_abandoned_owner_residue() {
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
        env.crank_steps_after_market_catchup(
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

        let long_capital_before_cleanup = long_state.capital.get();
        let long_pnl_before_cleanup = long_state.pnl.get();
        let long_epoch_before_cleanup = env.portfolio_position_epoch(long);
        let short_epoch_before_cleanup = env.portfolio_position_epoch(short);
        let vault_before_cleanup = env.market_state().1.vault;

        env.svm.expire_blockhash();
        let cleanup_cu = env
            .try_force_close_abandoned_asset_with_cu(
                &cranker,
                long,
                short,
                0,
                8,
                percolator::MAX_VAULT_TVL,
            )
            .expect("zero-OI Recovery residue must detach without its owner");
        assert_cu_within(
            "partial-ADL Recovery singleton cleanup",
            cleanup_cu,
            CUSTODY_CU_LIMIT,
        );
        assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
        assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
        assert_eq!(
            env.portfolio_position_epoch(long),
            long_epoch_before_cleanup + 1,
            "the changed position episode advances exactly once"
        );
        assert_eq!(
            env.portfolio_position_epoch(short),
            short_epoch_before_cleanup,
            "the already-flat counterparty episode is unchanged"
        );

        let finalize_cu = env.finalize_reset_side_with_cu(0, 0);
        assert_cu_within(
            "partial-ADL Recovery side finalization",
            finalize_cu,
            CUSTODY_CU_LIMIT,
        );
        let done = env.market_state().1;
        assert_eq!(
            done.assets[0].lifecycle,
            percolator::AssetLifecycleV16::Recovery
        );
        assert_eq!(done.assets[0].mode_long, percolator::SideModeV16::Normal);
        assert_eq!(done.assets[0].mode_short, percolator::SideModeV16::Normal);
        assert_eq!(done.assets[0].stored_pos_count_long, 0);
        assert_eq!(done.assets[0].stored_pos_count_short, 0);
        assert_eq!(done.assets[0].oi_eff_long_q, 0);
        assert_eq!(done.assets[0].oi_eff_short_q, 0);
        assert_eq!(done.vault, vault_before_cleanup);
        assert_eq!(done.vault as u64, env.token_amount(env.vault));
        assert_eq!(
            env.portfolio_state(long).capital.get(),
            long_capital_before_cleanup
        );
        assert_eq!(env.portfolio_state(long).pnl.get(), long_pnl_before_cleanup);
    }
}

#[test]
fn v16_program_expired_partial_close_matrix_resolves_and_preserves_idle_exit() {
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
                portfolio_id: env.portfolio_id(loss),
                position_epoch: env.portfolio_position_epoch(loss),
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
        for _ in 0..3 {
            if env.market_state().1.mode == MarketModeV16::Resolved {
                break;
            }
            assert_eq!(env.market_state().1.mode, MarketModeV16::Recovery);
            env.svm.expire_blockhash();
            let cu = env
                .send(
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
                )
                .expect("Recovery has a permissionless terminal continuation");
            assert_cu_within("expired-close terminal continuation", cu, CRANK_CU_LIMIT);
        }
        let (_, resolved) = env.market_state();
        assert_eq!(resolved.mode, MarketModeV16::Resolved);
        assert_eq!(resolved.vault as u64, env.token_amount(env.vault));
        let vault_before_idle_exit = resolved.vault;

        let idle_dest = env.token_account(idle_owner.pubkey(), 0);
        env.svm.expire_blockhash();
        let close_cu = env
            .send(
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
            )
            .expect("unrelated owner can close immediately during the anti-front-run delay");
        assert_cu_within(
            "expired-close unrelated user resolved exit",
            close_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(env.token_amount(idle_dest) as u128, idle_capital);
        assert_eq!(env.portfolio_state(idle).capital.get(), 0);
        let (_, after_idle_exit) = env.market_state();
        assert_eq!(after_idle_exit.vault, vault_before_idle_exit - idle_capital);
        assert_eq!(after_idle_exit.vault as u64, env.token_amount(env.vault));
        assert!(after_idle_exit.vault >= after_idle_exit.c_tot + after_idle_exit.insurance);
        env.close_portfolio_with_cu(&idle_owner, idle);
    }
}

pub(super) fn assert_inv_051_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup() {
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
        let keeper_owner = Keypair::new();
        env.svm.warp_to_slot(8);
        let survivor = env.create_portfolio(&survivor_owner);
        let counterparty = env.create_portfolio(&counterparty_owner);
        let keeper = env.create_portfolio(&keeper_owner);
        env.deposit(&survivor_owner, survivor, 1_000);
        // The fixed crank collects through slot 35 before sizing liquidation; the historical
        // fixture collected only through the slot-20 bounded accrual frontier. Add the exact
        // seven-slot net delta so post-fee capital, and therefore this ADL boundary, is unchanged.
        env.deposit(
            &counterparty_owner,
            counterparty,
            1_189 + maintenance_fee_per_slot * 7,
        );

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
            keeper,
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
        assert_eq!(crossed.assets[0].mode_long, SideModeV16::ResetPending);
        assert!(residual > 0);
        assert!(env.portfolio_state(survivor).capital.get() > 0);

        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_survivor = env.svm.get_account(&survivor).unwrap();
        env.svm.expire_blockhash();
        let owner_exit = env.send(
            ProgInstruction::RebalanceReduce {
                portfolio_id: env.portfolio_id(survivor),
                position_epoch: env.portfolio_position_epoch(survivor),
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

        env.svm.expire_blockhash();
        let cleanup_cu = env
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
            .expect("one public crank must clear the crossed zero-OI residue");
        assert_cu_within(
            "crossed zero-OI residue cleanup",
            cleanup_cu,
            CRANK_CU_LIMIT,
        );
        let cleaned = env.portfolio_state(survivor);
        assert!(!has_active_leg_for_asset(&cleaned, 0));
        assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
        let withdrawable = cleaned.capital.get();
        if withdrawable > 0 {
            let destination = env.withdraw(&survivor_owner, survivor, withdrawable);
            assert_eq!(env.token_amount(destination), withdrawable as u64);
        }
        env.close_portfolio_with_cu(&survivor_owner, survivor);
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
    }
}

pub(super) fn assert_inv_051_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup() {
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
        let _ = env.crank_steps_after_market_catchup(
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
        assert_eq!(reduced.assets[0].mode_long, SideModeV16::ResetPending);
        assert_eq!(reduced.assets[0].stored_pos_count_long, 1);
        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_long = env.svm.get_account(&long).unwrap();
        env.svm.expire_blockhash();
        let owner_retry = env.send(
            ProgInstruction::RebalanceReduce {
                portfolio_id: env.portfolio_id(long),
                position_epoch: env.portfolio_position_epoch(long),
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

        env.svm.expire_blockhash();
        let cleanup_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 6,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[],
            )
            .expect("one honest crank must clear the unilateral zero-OI residue");
        assert_cu_within(
            "unilateral zero-OI residue cleanup",
            cleanup_cu,
            CRANK_CU_LIMIT,
        );
        let cleaned = env.portfolio_state(long);
        assert!(!has_active_leg_for_asset(&cleaned, 0));
        assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
        let withdrawable = cleaned.capital.get();
        assert!(withdrawable > 0);
        let destination = env.withdraw(&long_owner, long, withdrawable);
        assert_eq!(env.token_amount(destination), withdrawable as u64);
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
    }
}

pub(super) fn assert_inv_051_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup() {
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
        let liquidated_owner = Keypair::new();
        let keeper_owner = Keypair::new();
        env.svm.warp_to_slot(8);
        let survivor = env.create_portfolio(&survivor_owner);
        let liquidated = env.create_portfolio(&liquidated_owner);
        let keeper = env.create_portfolio(&keeper_owner);
        env.deposit(&survivor_owner, survivor, 1_000);
        env.deposit(
            &liquidated_owner,
            liquidated,
            1_189 + maintenance_fee_per_slot * 7,
        );

        env.trade_asset_with_cu(
            0,
            &survivor_owner,
            survivor,
            &liquidated_owner,
            liquidated,
            OPEN_Q,
            PRICE,
            0,
        );
        env.svm.warp_to_slot(27);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: 27,
                observations: crank_observations(0),
            },
        );
        env.svm.warp_to_slot(35);
        env.sync_maintenance_fee_with_cu(liquidated, None, 35);
        env.crank_steps(
            liquidated,
            ProgInstruction::PermissionlessCrank {
                now_slot: 35,
                observations: crank_observations(0),
            },
            2,
        );

        let partial = env.market_state().1;
        let effective_q = partial.assets[0].oi_eff_long_q;
        let stored_q = active_leg_for_asset(&env.portfolio_state(survivor), 0)
            .basis_pos_q
            .unsigned_abs();
        assert_eq!(partial.assets[0].oi_eff_short_q, effective_q);
        assert!(
            effective_q > 0 && stored_q > effective_q,
            "public partial liquidation must leave a real ADL-scaled survivor: \
             maintenance_fee={maintenance_fee_per_slot}, effective={effective_q}, stored={stored_q}"
        );

        env.svm.warp_to_slot(60);
        env.sync_maintenance_fee_with_cu(liquidated, None, 60);
        env.crank_steps_after_market_catchup(
            liquidated,
            ProgInstruction::PermissionlessCrank {
                now_slot: 60,
                observations: crank_observations(0),
            },
            4,
        );

        let liquidated_group = env.market_state().1;
        let survivor_state = env.portfolio_state(survivor);
        let residual = active_leg_for_asset(&survivor_state, 0)
            .basis_pos_q
            .unsigned_abs();
        assert_eq!(liquidated_group.assets[0].oi_eff_long_q, 0);
        assert_eq!(liquidated_group.assets[0].oi_eff_short_q, 0);
        assert_eq!(
            liquidated_group.assets[0].mode_long,
            SideModeV16::ResetPending
        );
        assert!(
            residual > 0,
            "liquidation must leave raw winner residue to be cleaned by the reset path"
        );
        assert!(survivor_state.capital.get() > 0);
        assert_eq!(
            liquidated_group.vault as u64,
            env.token_amount(env.vault),
            "liquidation path preserves SPL vault/accounting parity"
        );

        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_survivor = env.svm.get_account(&survivor).unwrap();
        env.svm.expire_blockhash();
        let owner_retry = env.send(
            ProgInstruction::RebalanceReduce {
                portfolio_id: env.portfolio_id(survivor),
                position_epoch: env.portfolio_position_epoch(survivor),
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
        assert!(
            owner_retry.is_err(),
            "owner retry must not double-subtract zero effective OI"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&survivor).unwrap(), fixed_survivor);

        let fresh_owner = Keypair::new();
        let fresh = env.create_portfolio(&fresh_owner);
        env.deposit(&fresh_owner, fresh, 10_000);
        let trade_market = env.svm.get_account(&env.market).unwrap();
        let trade_survivor = env.svm.get_account(&survivor).unwrap();
        let trade_fresh = env.svm.get_account(&fresh).unwrap();
        env.svm.expire_blockhash();
        let matched_retry = env.try_trade_asset_with_cu(
            0,
            &survivor_owner,
            survivor,
            &fresh_owner,
            fresh,
            -(residual as i128),
            PRICE,
            0,
        );
        assert!(
            matched_retry.is_err(),
            "matched trade must not double-subtract zero effective OI"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), trade_market);
        assert_eq!(env.svm.get_account(&survivor).unwrap(), trade_survivor);
        assert_eq!(env.svm.get_account(&fresh).unwrap(), trade_fresh);

        env.svm.expire_blockhash();
        let cleanup_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 60,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(survivor, false),
                ],
                &[],
            )
            .expect("one public crank must clear the liquidation zero-OI residue");
        assert_cu_within(
            "liquidation zero-OI residue cleanup",
            cleanup_cu,
            CRANK_CU_LIMIT,
        );
        let cleaned = env.portfolio_state(survivor);
        assert!(!has_active_leg_for_asset(&cleaned, 0));
        assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
        let withdrawable = cleaned.capital.get();
        if withdrawable > 0 {
            let destination = env.withdraw(&survivor_owner, survivor, withdrawable);
            assert_eq!(env.token_amount(destination), withdrawable as u64);
        }
        env.close_portfolio_with_cu(&survivor_owner, survivor);
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
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
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
                    portfolio_id: env.portfolio_id(portfolio),
                    position_epoch: env.portfolio_position_epoch(portfolio),
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
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: PRICE,
            fee_bps: 0,
        })
        .collect();
    env.send(
        env.batch_trade_no_cpi_ix(long, short, open_legs),
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
            market_id: 0,
            observation_sequence: u64::MAX,
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
        data: env
            .trade_cpi_ix(long, short, 0, -(POS_SCALE as i128), 0, PRICE)
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

// permissionless resolver captures the terminal snapshot.
#[test]
fn v16_program_configure_permissionless_resolve_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous fresh control: before the stale boundary, marketauth can still tune the policy.
    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let fresh = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 6,
            force_close_delay_slots: 6,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        fresh.is_ok(),
        "fresh ConfigurePermissionlessResolve remains reachable: {fresh:?}"
    );
    assert_eq!(env.market_state().0.permissionless_resolve_stale_slots, 6);

    env.svm.warp_to_slot(40);
    let (stale_cfg, stale_group) = env.market_state();
    assert_eq!(stale_group.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&stale_cfg, 40),
        "test setup must be beyond the configured stale boundary"
    );
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.expire_blockhash();
    let stale = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 1_000,
            force_close_delay_slots: 1_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        stale.is_err(),
        "ConfigurePermissionlessResolve must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale reconfiguration leaves resolve policy unchanged"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve remains live after rejected stale reconfiguration: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// terminal payout snapshot can be influenced.
#[test]
fn v16_program_convert_released_pnl_rejects_when_resolve_matured() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.top_up_backing_bucket(1, RELEASED * 2, 10_000);

    let fresh_owner = Keypair::new();
    let fresh = env.create_portfolio(&fresh_owner);
    let stale_owner = Keypair::new();
    let stale = env.create_portfolio(&stale_owner);
    env.add_source_positive_pnl(fresh, 1, RELEASED);
    env.add_source_positive_pnl(stale, 1, RELEASED);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.crank_steps_after_market_catchup(
        fresh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        stale,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert!(
        env.portfolio_state(stale).pnl.get() > 0,
        "stale-path setup must have real released PnL to convert"
    );

    env.svm.warp_to_slot(4);
    let fresh_cu = env.convert_released_pnl_with_cu(&fresh_owner, fresh, RELEASED);
    assert_cu_within(
        "fresh ConvertReleasedPnl before resolve maturity",
        fresh_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.portfolio_state(fresh).capital.get(),
        RELEASED,
        "fresh conversion proves the staged PnL is actually convertible"
    );

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_before = env.svm.get_account(&stale).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let stale_state_before = env.portfolio_state(stale);

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.convert_released_pnl_ix(stale, RELEASED),
        vec![
            AccountMeta::new(stale_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale, false),
        ],
        &[&stale_owner],
    );
    assert!(
        rejected.is_err(),
        "ConvertReleasedPnl must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale ConvertReleasedPnl leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale).unwrap(),
        stale_before,
        "rejected stale ConvertReleasedPnl leaves the owner portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale ConvertReleasedPnl moves no custody"
    );
    let stale_state_after = env.portfolio_state(stale);
    assert_eq!(
        stale_state_after.capital.get(),
        stale_state_before.capital.get()
    );
    assert_eq!(stale_state_after.pnl.get(), stale_state_before.pnl.get());

    env.resolve();
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// asserted. Without it a user withdraws against stale equity before the market settles (solvency envelope).
#[test]
fn v16_program_withdraw_rejected_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5); // resolve-stale threshold = 5 slots
    env.configure_auth_mark_with_cu(0, 100);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);

    // Mark the oracle fresh at slot 3.
    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[],
    );

    // NON-VACUOUS: while the oracle is fresh (slot 4, 1 < 5 stale) the same withdraw SUCCEEDS.
    env.svm.warp_to_slot(4);
    let (d, _) = env.withdraw_with_cu(&owner, p, 100_000);
    assert_eq!(
        env.token_amount(d),
        100_000,
        "fresh-oracle withdraw succeeds (non-vacuous)"
    );

    // Warp far past the stale threshold -> market is resolve-matured.
    env.svm.warp_to_slot(40);
    env.svm.expire_blockhash();
    let dest = env.token_account(owner.pubkey(), 0);
    let r = env.send(
        env.withdraw_ix(p, 100_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw must reject once the market is resolve-matured (#66 solvency gate)"
    );
}

// support, positions, or settlement state in the stale window before the terminal snapshot is captured.
#[test]
fn v16_program_live_value_paths_reject_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let taker = Keypair::new();
    let maker = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let maker_portfolio = env.create_portfolio(&maker);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&maker, maker_portfolio, 1_000_000);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous controls: while the oracle is fresh, the same classes of operations work.
    env.svm.warp_to_slot(4);
    let fresh_deposit_source = env.token_account_for_mint(env.mint, taker.pubkey(), 50_000);
    env.svm.expire_blockhash();
    let fresh_deposit = env.send(
        env.deposit_ix(taker_portfolio, 50_000),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(fresh_deposit_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&taker],
    );
    assert!(
        fresh_deposit.is_ok(),
        "fresh-oracle Deposit should still work: {fresh_deposit:?}"
    );
    env.top_up_insurance(10);
    env.top_up_insurance_domain_with_authority(&admin, 0, 10);
    env.top_up_backing_bucket_with_authority(&admin, 1, 10, 100);
    env.trade_asset_with_cu(
        0,
        &taker,
        taker_portfolio,
        &maker,
        maker_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(40);
    let stale_deposit_source = env.token_account_for_mint(env.mint, taker.pubkey(), 75_000);
    let stale_global_insurance_source = env.token_account_for_mint(env.mint, admin.pubkey(), 20);
    let stale_insurance_source = env.token_account_for_mint(env.mint, admin.pubkey(), 25);
    let stale_backing_source = env.token_account_for_mint(env.mint, admin.pubkey(), 30);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let maker_before = env.svm.get_account(&maker_portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let deposit_source_before = env.svm.get_account(&stale_deposit_source).unwrap();
    let global_insurance_source_before =
        env.svm.get_account(&stale_global_insurance_source).unwrap();
    let insurance_source_before = env.svm.get_account(&stale_insurance_source).unwrap();
    let backing_source_before = env.svm.get_account(&stale_backing_source).unwrap();

    env.svm.expire_blockhash();
    let stale_deposit = env.send(
        env.deposit_ix(taker_portfolio, 75_000),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(stale_deposit_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&taker],
    );
    assert!(
        stale_deposit.is_err(),
        "Deposit must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_trade = env.send(
        env.trade_no_cpi_ix(
            taker_portfolio,
            maker_portfolio,
            0,
            POS_SCALE as i128,
            100,
            0,
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(maker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(maker_portfolio, false),
        ],
        &[&taker, &maker],
    );
    assert!(
        stale_trade.is_err(),
        "TradeNoCpi must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_global_insurance = env.send(
        ProgInstruction::TopUpInsurance {
            market_id: 0,
            amount: 20,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_global_insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_global_insurance.is_err(),
        "TopUpInsurance must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_insurance = env.send(
        ProgInstruction::TopUpInsuranceDomain {
            market_id: 0,
            domain: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_insurance.is_err(),
        "TopUpInsuranceDomain must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_backing = env.send(
        ProgInstruction::TopUpBackingBucket {
            market_id: 0,
            domain: 1,
            amount: 30,
            expiry_slot: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_backing_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_backing.is_err(),
        "TopUpBackingBucket must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
        ],
        &[],
    );
    assert!(
        stale_crank.is_err(),
        "PermissionlessCrank refresh must reject once the market is resolve-matured"
    );

    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale-window live ops leave market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&taker_portfolio).unwrap(),
        taker_before,
        "stale-window live ops leave taker portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&maker_portfolio).unwrap(),
        maker_before,
        "stale-window live ops leave maker portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale-window live ops move no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&stale_deposit_source).unwrap(),
        deposit_source_before,
        "rejected stale Deposit pulls no source tokens"
    );
    assert_eq!(
        env.svm.get_account(&stale_global_insurance_source).unwrap(),
        global_insurance_source_before,
        "rejected stale global insurance top-up pulls no source tokens"
    );
    assert_eq!(
        env.svm.get_account(&stale_insurance_source).unwrap(),
        insurance_source_before,
        "rejected stale insurance top-up pulls no source tokens"
    );
    assert_eq!(
        env.svm.get_account(&stale_backing_source).unwrap(),
        backing_source_before,
        "rejected stale backing top-up pulls no source tokens"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "the stale boundary should still be resolvable permissionlessly: {resolve:?}"
    );
    let (_, group) = env.market_state();
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(
        group.resolved_slot, 40,
        "resolved_slot uses the authenticated clock slot"
    );
}

// must freeze so a cranker cannot alter the terminal insurance/capital split before resolution.
#[test]
fn v16_program_sync_maintenance_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100_000);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while fresh, maintenance fee sync charges the account.
    env.svm.warp_to_slot(4);
    env.sync_maintenance_fee_with_cu(portfolio, None, 4);
    assert_eq!(
        env.portfolio_state(portfolio).last_fee_slot.get(),
        4,
        "fresh maintenance sync advanced the fee slot"
    );
    let fresh_capital = env.portfolio_state(portfolio).capital.get();
    assert!(
        fresh_capital < 100_000,
        "fresh maintenance sync charged capital"
    );

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::SyncMaintenanceFee { now_slot: 0 },
        vec![
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "SyncMaintenanceFee must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale maintenance sync leaves market insurance unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected stale maintenance sync does not debit user capital"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale maintenance sync moves no custody"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale maintenance sync: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// freeze before the terminal snapshot.
#[test]
fn v16_program_forfeit_recovery_leg_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let fresh_long_owner = Keypair::new();
    let fresh_short_owner = Keypair::new();
    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let fresh_long = env.create_portfolio(&fresh_long_owner);
    let fresh_short = env.create_portfolio(&fresh_short_owner);
    let stale_long = env.create_portfolio(&stale_long_owner);
    let stale_short = env.create_portfolio(&stale_short_owner);
    for (owner, portfolio) in [
        (&fresh_long_owner, fresh_long),
        (&fresh_short_owner, fresh_short),
        (&stale_long_owner, stale_long),
        (&stale_short_owner, stale_short),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }
    env.trade_asset_with_cu(
        0,
        &fresh_long_owner,
        fresh_long,
        &fresh_short_owner,
        fresh_short,
        POS_SCALE as i128,
        100,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &stale_long_owner,
        stale_long,
        &stale_short_owner,
        stale_short,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.mutate_market(|_, group| {
        assert_eq!(group.mode, MarketModeV16::Live);
        group.assets[0].lifecycle = AssetLifecycleV16::Recovery;
    });
    let (_, recovery_group) = env.market_state();
    assert_eq!(recovery_group.mode, MarketModeV16::Live);
    assert_eq!(
        recovery_group.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(4);
    let fresh_cu = env.forfeit_recovery_leg_with_cu(&fresh_long_owner, fresh_long, 0, 1);
    assert_cu_within(
        "fresh ForfeitRecoveryLeg before resolve maturity",
        fresh_cu,
        CUSTODY_CU_LIMIT,
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(fresh_long), 0),
        "fresh asset-recovery forfeit proves the live path is reachable before stale maturity"
    );
    assert!(
        has_active_leg_for_asset(&env.portfolio_state(stale_long), 0),
        "stale-path target must still have a leg to forfeit"
    );

    env.svm.warp_to_slot(40);
    let (stale_cfg, stale_group) = env.market_state();
    assert_eq!(stale_group.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&stale_cfg, 40),
        "test setup must be beyond the permissionless resolve stale boundary"
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_long_before = env.svm.get_account(&stale_long).unwrap();
    let stale_short_before = env.svm.get_account(&stale_short).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(stale_long),
            position_epoch: env.portfolio_position_epoch(stale_long),
            asset_index: 0,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(stale_long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_long, false),
        ],
        &[&stale_long_owner],
    );
    assert!(
        rejected.is_err(),
        "ForfeitRecoveryLeg must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale ForfeitRecoveryLeg leaves market exposure unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_long).unwrap(),
        stale_long_before,
        "rejected stale ForfeitRecoveryLeg leaves the owner portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_short).unwrap(),
        stale_short_before,
        "rejected stale ForfeitRecoveryLeg leaves the counterparty unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale ForfeitRecoveryLeg moves no custody"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale ForfeitRecoveryLeg: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// before the terminal snapshot.
#[test]
fn v16_program_rebalance_reduce_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

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
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while fresh, the same owner can reduce part of their position.
    env.svm.warp_to_slot(4);
    env.rebalance_reduce_with_cu(&long_owner, long, 0, POS_SCALE / 4);
    let remaining = env.portfolio_state(long).legs[0]
        .basis_pos_q
        .get()
        .unsigned_abs();
    assert!(
        remaining > 0 && remaining < POS_SCALE,
        "fresh RebalanceReduce partially reduced the position"
    );

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long).unwrap();
    let short_before = env.svm.get_account(&short).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(long),
            position_epoch: env.portfolio_position_epoch(long),
            asset_index: 0,
            reduce_q: remaining,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ],
        &[&long_owner],
    );
    assert!(
        rejected.is_err(),
        "RebalanceReduce must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale RebalanceReduce leaves market exposure unchanged"
    );
    assert_eq!(
        env.svm.get_account(&long).unwrap(),
        long_before,
        "rejected stale RebalanceReduce leaves the owner portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&short).unwrap(),
        short_before,
        "rejected stale RebalanceReduce leaves the counterparty unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale RebalanceReduce moves no custody"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale RebalanceReduce: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// side addition to fund the other account's reduction of pre-existing open interest.
#[test]
fn v16_attack_crossed_trade_cannot_turn_partial_liquidation_survivors_same_side() {
    const PRICE: u64 = 1;
    const OPEN_Q: i128 = 13_000 * POS_SCALE as i128;

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
        maintenance_fee_per_slot: 27,
        ..V16CuMarketParams::default()
    });
    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let keeper_owner = Keypair::new();
    env.svm.warp_to_slot(8);
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&owner_a, account_a, 1_000);
    env.deposit(&owner_b, account_b, 1_189 + 7 * 27);

    let open_cu = env
        .try_trade_asset_with_cu(
            0, &owner_a, account_a, &owner_b, account_b, OPEN_Q, PRICE, 0,
        )
        .expect("open trade");
    assert_cu_within("issue 103 open", open_cu, TRADE_CU_LIMIT);

    env.svm.warp_to_slot(27);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: 27,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(35);
    env.sync_maintenance_fee_with_cu(account_b, None, 35);
    let liquidation_cu = env.crank_steps(
        account_b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 35,
            observations: crank_observations(0),
        },
        2,
    );
    assert_cu_within(
        "issue 103 partial liquidation",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );

    let (_, group) = env.market_state();
    let asset = group.assets[0];
    let account_a_state = env.portfolio_state(account_a);
    let account_b_state = env.portfolio_state(account_b);
    let leg_a = active_leg_for_asset(&account_a_state, 0);
    let leg_b = active_leg_for_asset(&account_b_state, 0);
    assert_eq!(asset.lifecycle, AssetLifecycleV16::Active);
    assert_eq!(asset.mode_long, SideModeV16::Normal);
    assert_eq!(asset.mode_short, SideModeV16::Normal);
    assert_eq!(leg_a.side, SideV16::Long);
    assert_eq!(leg_b.side, SideV16::Short);
    let survivor_q = leg_a.basis_pos_q.unsigned_abs();
    let liquidated_survivor_q = leg_b.basis_pos_q.unsigned_abs();
    assert_eq!(survivor_q, OPEN_Q.unsigned_abs());
    assert!(
        liquidated_survivor_q > 0 && liquidated_survivor_q < survivor_q,
        "setup must leave a genuine partial-liquidation imbalance"
    );
    assert_eq!(asset.oi_eff_long_q, liquidated_survivor_q);
    assert_eq!(asset.oi_eff_short_q, liquidated_survivor_q);

    let cross_q = liquidated_survivor_q + (survivor_q - liquidated_survivor_q) / 2;
    assert!(liquidated_survivor_q < cross_q && cross_q < survivor_q);
    let same_call_long_addition = cross_q - liquidated_survivor_q;
    assert_eq!(
        asset.oi_eff_long_q + same_call_long_addition,
        cross_q,
        "the rejected transaction must exercise same-call OI self-financing"
    );

    env.svm.expire_blockhash();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let account_a_before = env.svm.get_account(&account_a).unwrap();
    let account_b_before = env.svm.get_account(&account_b).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let rejected = env
        .try_trade_asset_with_cu(
            0,
            &owner_b,
            account_b,
            &owner_a,
            account_a,
            cross_q as i128,
            PRICE,
            0,
        )
        .expect_err("crossed trade must not create two same-side survivor legs");
    assert!(
        rejected.contains("Custom(21)") || rejected.contains("custom program error: 0x15"),
        "crossed trade must fail at the engine OI gate, got {rejected}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&account_a).unwrap(), account_a_before);
    assert_eq!(env.svm.get_account(&account_b).unwrap(), account_b_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let reduce_cu = env.rebalance_reduce_with_cu(&owner_b, account_b, 0, liquidated_survivor_q);
    assert_cu_within("issue 103 short owner exit", reduce_cu, CUSTODY_CU_LIMIT);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(account_b), 0),
        "the smaller post-liquidation side must retain a public owner exit",
    );

    env.svm.expire_blockhash();
    let forfeit_cu =
        env.forfeit_recovery_leg_with_cu(&owner_a, account_a, 0, percolator::MAX_VAULT_TVL);
    assert_cu_within("issue 103 ADL survivor exit", forfeit_cu, CUSTODY_CU_LIMIT);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(account_a), 0),
        "the zero-effective-OI ADL survivor must retain a public owner cleanup route",
    );
    let exited_a = env.portfolio_state(account_a);
    let exited_b = env.portfolio_state(account_b);
    assert_eq!(
        exited_a.capital.get(),
        1_000,
        "ADL survivor cleanup must preserve the owner's principal",
    );
    assert!(
        exited_b.capital.get() > 0,
        "the partially liquidated owner must retain withdrawable principal",
    );
    assert_eq!(exited_a.pnl.get(), 0);
    assert_eq!(exited_b.pnl.get(), 0);
    assert_eq!(exited_a.fee_credits.get(), 0);
    assert_eq!(exited_b.fee_credits.get(), 0);
    let (_, exited_group) = env.market_state();
    assert_eq!(exited_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(exited_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(exited_group.assets[0].stored_pos_count_long, 0);
    assert_eq!(exited_group.assets[0].stored_pos_count_short, 0);
    assert_eq!(exited_group.vault as u64, env.token_amount(env.vault));
    assert!(exited_group.vault >= exited_group.c_tot + exited_group.insurance);

    env.svm.expire_blockhash();
    let insurance_before_a_withdrawal = env.market_state().1.insurance;
    let (dest_a, withdraw_a_cu) = env.withdraw_with_cu(&owner_a, account_a, exited_a.capital.get());
    assert_cu_within(
        "issue 103 ADL survivor withdrawal",
        withdraw_a_cu,
        CUSTODY_CU_LIMIT,
    );
    let elapsed_maintenance = 27u128 * 27;
    assert_eq!(
        env.token_amount(dest_a),
        (exited_a.capital.get() - elapsed_maintenance) as u64,
        "withdraw-all must pay the principal remaining after crystallizing maintenance"
    );
    assert_eq!(
        env.market_state().1.insurance - insurance_before_a_withdrawal,
        elapsed_maintenance,
        "withdraw-all must attribute the crystallized maintenance exactly once"
    );
    env.svm.expire_blockhash();
    let (dest_b, withdraw_b_cu) = env.withdraw_with_cu(&owner_b, account_b, exited_b.capital.get());
    assert_cu_within(
        "issue 103 partial liquidation withdrawal",
        withdraw_b_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest_b), exited_b.capital.get() as u64);

    env.svm.expire_blockhash();
    let close_a_cu = env.close_portfolio_with_cu(&owner_a, account_a);
    assert_cu_within(
        "issue 103 ADL survivor account close",
        close_a_cu,
        CUSTODY_CU_LIMIT,
    );
    env.svm.expire_blockhash();
    let close_b_cu = env.close_portfolio_with_cu(&owner_b, account_b);
    assert_cu_within(
        "issue 103 partial liquidation account close",
        close_b_cu,
        CUSTODY_CU_LIMIT,
    );
    env.close_portfolio_with_cu(&keeper_owner, keeper);
    let (_, final_group) = env.market_state();
    assert_eq!(final_group.materialized_portfolio_count, 0);
    assert_eq!(final_group.vault as u64, env.token_amount(env.vault));
    assert!(final_group.vault >= final_group.c_tot + final_group.insurance);
}

#[test]
fn v16_program_stale_resolve_matured_no_observation_liquidation_rejects() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert!(
        health_cert(&env.portfolio_state(short_account)).certified_liq_deficit != 0,
        "setup must be liquidatable before stale-window probe"
    );

    env.svm.warp_to_slot(40);
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_short = env.svm.get_account(&short_account).unwrap();
    env.svm.expire_blockhash();
    let result = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 40,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    let err = result
        .expect_err("stale-window no-observation liquidation must reject before live mutation");
    assert!(
        err.contains("Custom(27)") || err.contains("custom program error: 0x1b"),
        "stale-window no-observation liquidation should fail as OracleStale, got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_market,
        "rejected stale-window liquidation leaves market state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        before_short,
        "rejected stale-window liquidation leaves target portfolio unchanged"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless stale resolve remains available after rejected liquidation: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// Issue #103: after a partial liquidation, a crossed trade must not use one account's same-call
#[test]
fn v16_program_stale_resolve_matured_b_stale_cleanup_still_progresses() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    env.mark_b_stale_gap(long_account, 0, 1);
    assert!(
        env.portfolio_state(long_account).b_stale_state != 0,
        "setup must leave a B-stale account that blocks final cleanup"
    );

    env.svm.warp_to_slot(40);
    env.svm.expire_blockhash();
    let settle = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 40,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
        ],
        &[],
    );
    assert!(
        settle.is_ok(),
        "stale boundary must still allow B-stale cleanup required for resolution: {settle:?}"
    );
    assert_eq!(
        env.portfolio_state(long_account).b_stale_state,
        0,
        "B-stale cleanup clears the account blocker"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless stale resolve remains available after B cleanup: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// before the terminal snapshot can be influenced.
#[test]
fn v16_program_force_close_abandoned_asset_rejects_when_resolve_matured() {
    const STALE_SLOTS: u64 = 20;
    const DELAY: u64 = 5;
    const SHUTDOWN_SLOT: u64 = 10;
    const FRESH_FORCE_SLOT: u64 = SHUTDOWN_SLOT + DELAY + 1;
    const STALE_FORCE_SLOT: u64 = 40;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_permissionless_resolve_with_cu(STALE_SLOTS, DELAY);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let fresh_long_owner = Keypair::new();
    let fresh_short_owner = Keypair::new();
    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let fresh_long = env.create_portfolio(&fresh_long_owner);
    let fresh_short = env.create_portfolio(&fresh_short_owner);
    let stale_long = env.create_portfolio(&stale_long_owner);
    let stale_short = env.create_portfolio(&stale_short_owner);
    for (owner, portfolio) in [
        (&fresh_long_owner, fresh_long),
        (&fresh_short_owner, fresh_short),
        (&stale_long_owner, stale_long),
        (&stale_short_owner, stale_short),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }
    env.trade_asset_with_cu(
        1,
        &fresh_long_owner,
        fresh_long,
        &fresh_short_owner,
        fresh_short,
        POS_SCALE as i128,
        100,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &stale_long_owner,
        stale_long,
        &stale_short_owner,
        stale_short,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );
    let recovery_group = env.market_state().1;
    assert_eq!(recovery_group.mode, MarketModeV16::Live);
    assert_eq!(
        recovery_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(FRESH_FORCE_SLOT);
    let (fresh_cfg, fresh_group) = env.market_state();
    assert_eq!(fresh_group.mode, MarketModeV16::Live);
    assert!(
        !oracle_v16::permissionless_stale_matured(&fresh_cfg, FRESH_FORCE_SLOT),
        "fresh control must be before the base permissionless-resolve boundary"
    );
    let cranker = Keypair::new();
    let fresh_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        fresh_long,
        fresh_short,
        1,
        FRESH_FORCE_SLOT,
        POS_SCALE,
    );
    assert_cu_within(
        "fresh ForceCloseAbandonedAsset before resolve maturity",
        fresh_cu,
        TRADE_CU_LIMIT,
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(fresh_long), 1),
        "fresh abandoned-asset force-close proves the public path is reachable"
    );
    assert!(
        has_active_leg_for_asset(&env.portfolio_state(stale_long), 1),
        "stale-path target must still have a leg to force-close"
    );

    env.svm.warp_to_slot(STALE_FORCE_SLOT);
    let (stale_cfg, stale_group) = env.market_state();
    assert_eq!(stale_group.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&stale_cfg, STALE_FORCE_SLOT),
        "test setup must be beyond the base permissionless-resolve boundary"
    );
    for portfolio in [stale_long, stale_short] {
        let mut legacy = env.svm.get_account(&portfolio).unwrap();
        legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
        env.svm.set_account(portfolio, legacy).unwrap();
    }
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_long_before = env.svm.get_account(&stale_long).unwrap();
    let stale_short_before = env.svm.get_account(&stale_short).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    assert_eq!(
        stale_long_before.data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "stale long setup uses a legacy portfolio"
    );
    assert_eq!(
        stale_short_before.data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "stale short setup uses a legacy portfolio"
    );

    let rejected = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        stale_long,
        stale_short,
        1,
        STALE_FORCE_SLOT,
        POS_SCALE,
    );
    assert!(
        rejected.is_err(),
        "ForceCloseAbandonedAsset must reject once the base market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale ForceClose leaves market exposure unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_long).unwrap(),
        stale_long_before,
        "rejected stale ForceClose leaves the long portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_short).unwrap(),
        stale_short_before,
        "rejected stale ForceClose leaves the short portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_long).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "rejected stale ForceClose rolls back long-account realloc"
    );
    assert_eq!(
        env.svm.get_account(&stale_short).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "rejected stale ForceClose rolls back short-account realloc"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale ForceClose moves no custody"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale ForceClose: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// attacker could block final CloseSlab liveness with a fresh account created after trading stopped.
#[test]
fn v16_program_init_portfolio_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while fresh, InitPortfolio works and ClosePortfolio clears the count.
    env.svm.warp_to_slot(4);
    let fresh_owner = Keypair::new();
    let fresh_portfolio = env.create_portfolio(&fresh_owner);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "fresh InitPortfolio materializes a portfolio"
    );
    env.close_portfolio_with_cu(&fresh_owner, fresh_portfolio);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "fresh ClosePortfolio clears the control portfolio"
    );

    env.svm.warp_to_slot(40);
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let stale_portfolio = env.program_account(env.portfolio_account_len);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_portfolio_before = env.svm.get_account(&stale_portfolio).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_portfolio, false),
        ],
        &[&attacker],
    );
    assert!(
        rejected.is_err(),
        "InitPortfolio must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale InitPortfolio leaves materialized count unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_portfolio).unwrap(),
        stale_portfolio_before,
        "rejected stale InitPortfolio leaves the target account uninitialized"
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "no stale-window portfolio can block terminal slab reclaim"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale InitPortfolio: {resolve:?}"
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "resolved market has no attacker-created materialized account"
    );
    env.close_slab_with_cu();
}

// lifecycle/value-in path must freeze just like Deposit/InitPortfolio/top-ups.
#[test]
fn v16_program_permissionless_asset_activation_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(10);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while fresh, a permissionless creator can append an empty asset.
    let fresh_creator = Keypair::new();
    env.svm.warp_to_slot(4);
    env.activate_permissionless_asset_with_fee(
        &fresh_creator,
        1,
        4,
        100,
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        10,
    );
    assert_eq!(
        env.market_state().1.config.max_market_slots,
        2,
        "fresh permissionless activation appended asset 1"
    );

    env.svm.warp_to_slot(40);
    let stale_creator = Keypair::new();
    env.ensure_signer_account(stale_creator.pubkey());
    let stale_fee_source = env.token_account(stale_creator.pubkey(), 10);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let source_before = env.svm.get_account(&stale_fee_source).unwrap();
    let activation_market_id = env.market_state().1.next_market_id;

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 2,
            market_id: activation_market_id,
            now_slot: 0,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: stale_creator.pubkey().to_bytes(),
            insurance_operator: stale_creator.pubkey().to_bytes(),
            backing_bucket_authority: stale_creator.pubkey().to_bytes(),
            oracle_authority: stale_creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(stale_creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_fee_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&stale_creator],
    );
    assert!(
        rejected.is_err(),
        "permissionless asset activation must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale activation leaves market capacity and authorities unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale activation moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&stale_fee_source).unwrap(),
        source_before,
        "rejected stale activation does not collect the permissionless init fee"
    );
    assert_eq!(
        env.market_state().1.config.max_market_slots,
        2,
        "stale activation cannot append a new market slot"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale activation: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// base market is resolve-matured, that lifecycle mutation must freeze before terminal resolution.
#[test]
fn v16_program_permissionless_asset_shutdown_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(10);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let fresh_creator = Keypair::new();
    let stale_creator = Keypair::new();
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &fresh_creator,
        1,
        1,
        100,
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        10,
    );
    env.svm.warp_to_slot(2);
    env.activate_permissionless_asset_with_fee(
        &stale_creator,
        2,
        2,
        100,
        stale_creator.pubkey(),
        stale_creator.pubkey(),
        stale_creator.pubkey(),
        stale_creator.pubkey(),
        10,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    let (_, active_group) = env.market_state();
    assert_eq!(active_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(active_group.assets[2].lifecycle, AssetLifecycleV16::Active);

    env.svm.warp_to_slot(4);
    let fresh_shutdown = env.try_shutdown_asset_with_authority(&fresh_creator, 1, 4);
    assert!(
        fresh_shutdown.is_ok(),
        "fresh permissionless asset shutdown succeeds: {fresh_shutdown:?}"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "fresh shutdown proves the per-asset admin path is reachable"
    );

    env.svm.warp_to_slot(40);
    let (stale_cfg, stale_group) = env.market_state();
    assert_eq!(stale_group.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&stale_cfg, 40),
        "test setup must be beyond the permissionless resolve stale boundary"
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.try_shutdown_asset_with_authority(&stale_creator, 2, 40);
    assert!(
        rejected.is_err(),
        "permissionless asset shutdown must reject once the base market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale shutdown leaves lifecycle/profile state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale shutdown moves no custody"
    );
    assert_eq!(
        env.market_state().1.assets[2].lifecycle,
        AssetLifecycleV16::Active,
        "stale shutdown cannot move the asset into Recovery"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale shutdown: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// snapshot and trader exit surface can be changed in the stale window before permissionless resolve.
#[test]
fn v16_program_marketauth_lifecycle_actions_reject_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(1);
    env.activate_asset(1, 1, 100);
    env.svm.warp_to_slot(2);
    env.activate_asset(2, 2, 100);
    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous fresh controls: marketauth lifecycle actions are reachable before stale maturity.
    env.svm.warp_to_slot(4);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        1,
        0,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::DrainOnly,
        "fresh marketauth DrainOnly path is reachable"
    );
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        4,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Retired,
        "fresh marketauth Retire path is reachable"
    );

    env.svm.warp_to_slot(40);
    let (stale_cfg, stale_group) = env.market_state();
    assert_eq!(stale_group.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&stale_cfg, 40),
        "test setup must be beyond the permissionless resolve stale boundary"
    );
    assert_eq!(stale_group.assets[2].lifecycle, AssetLifecycleV16::Active);
    let market_id = stale_group.assets[2].market_id;

    let before_drain = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_drain = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index: 2,
            market_id,
            now_slot: 0,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        stale_drain.is_err(),
        "marketauth DrainOnly must reject once the base market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_drain,
        "rejected stale DrainOnly leaves lifecycle state unchanged"
    );

    let before_retire = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_retire = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_RETIRE,
            asset_index: 2,
            market_id,
            now_slot: 40,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        stale_retire.is_err(),
        "marketauth Retire must reject once the base market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_retire,
        "rejected stale Retire leaves lifecycle state unchanged"
    );
    assert_eq!(
        env.market_state().1.assets[2].lifecycle,
        AssetLifecycleV16::Active,
        "stale marketauth lifecycle actions cannot alter the active asset"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale lifecycle actions: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// non-marketauth asset admin can mutate live market state before the terminal resolve snapshot.
#[test]
fn v16_program_restart_asset_oracle_rejects_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.update_market_init_fee_policy_with_cu(10);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        200,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        10,
    );

    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&creator, 1, 2)
        .expect("permissionless asset admin can shut down own asset while fresh");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while the base oracle is fresh, the same asset admin can restart.
    env.svm.expire_blockhash();
    env.try_restart_asset_oracle_with_authority(&creator, 1, 3, 210)
        .expect("fresh permissionless asset restart succeeds");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active
    );

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&creator, 1, 4)
        .expect("prepare the asset for the stale restart attempt");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.try_restart_asset_oracle_with_authority(&creator, 1, 40, 250);
    assert!(
        rejected.is_err(),
        "RestartAssetOracle must reject once the base market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale restart leaves lifecycle and oracle profile unchanged"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "stale restart cannot reactivate the permissionless asset"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale restart: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// earnings withdrawals must freeze before terminal resolution captures the final support envelope.
#[test]
fn v16_program_live_domain_withdrawals_reject_when_resolve_matured() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);
    env.top_up_backing_bucket_with_authority(&admin, 1, 100, 100);
    let earnings_ledger = env.backing_domain_ledger_account();
    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
    });
    env.set_token_account_amount(
        env.vault,
        env.mint,
        env.vault_authority,
        env.market_state().1.vault as u64,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous controls: while fresh, all live value-out paths can withdraw.
    env.svm.warp_to_slot(4);
    let (fresh_insurance_dest, _) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, 10)
        .expect("fresh insurance withdrawal succeeds");
    assert_eq!(
        env.token_amount(fresh_insurance_dest),
        10,
        "fresh insurance withdrawal moved custody"
    );
    let fresh_backing_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(fresh_backing_dest, 1, 10);
    assert_eq!(
        env.token_amount(fresh_backing_dest),
        10,
        "fresh backing withdrawal moved custody"
    );
    let fresh_earnings_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(
        earnings_ledger,
        fresh_earnings_dest,
        1,
        5,
    );
    assert_eq!(
        env.token_amount(fresh_earnings_dest),
        5,
        "fresh backing-provider earnings withdrawal moved custody"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[1].utilization_fee_earnings,
        25,
        "stale-path setup leaves provider earnings to attack"
    );

    env.svm.warp_to_slot(40);
    let stale_insurance_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let stale_backing_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let stale_earnings_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let earnings_ledger_before = env.svm.get_account(&earnings_ledger).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let insurance_dest_before = env.svm.get_account(&stale_insurance_dest).unwrap();
    let backing_dest_before = env.svm.get_account(&stale_backing_dest).unwrap();
    let earnings_dest_before = env.svm.get_account(&stale_earnings_dest).unwrap();

    env.svm.expire_blockhash();
    let stale_insurance = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 20,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_insurance_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_insurance.is_err(),
        "WithdrawInsuranceAsset must reject once the market is resolve-matured"
    );

    let market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let stale_backing = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            amount: 20,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_backing.is_err(),
        "WithdrawBackingBucket must reject once the market is resolve-matured"
    );

    env.svm.expire_blockhash();
    let stale_earnings = env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(earnings_ledger, false),
            AccountMeta::new(stale_earnings_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_earnings.is_err(),
        "WithdrawBackingBucketEarnings must reject once the market is resolve-matured"
    );

    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale domain withdrawals leave market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&earnings_ledger).unwrap(),
        earnings_ledger_before,
        "rejected stale earnings withdrawal leaves the provider ledger unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale domain withdrawals move no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&stale_insurance_dest).unwrap(),
        insurance_dest_before,
        "rejected stale insurance withdrawal pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&stale_backing_dest).unwrap(),
        backing_dest_before,
        "rejected stale backing withdrawal pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&stale_earnings_dest).unwrap(),
        earnings_dest_before,
        "rejected stale earnings withdrawal pays no tokens"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after rejected stale withdrawals: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_attack_resolved_cross_margin_deep_insolvency_winds_down_publicly() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg_asset1 = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("asset1 mark cfg");
    };
    cfg_asset1(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
        },
    );
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let cp_owner = Keypair::new();
    let cp = env.create_portfolio(&cp_owner);
    env.deposit(&victim_owner, victim, 250);
    env.deposit(&cp_owner, cp, 2_000_000);
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        -(POS_SCALE as i128),
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        -(POS_SCALE as i128),
        100,
        0,
    );

    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        cfg_asset1(
            &mut env,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: slot + 1,
                asset_index: 1,
                now_slot: slot,
                mark_e6: mark,
            },
        );
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                env.svm.expire_blockhash();
                let _ = env.send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
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
    }

    for _ in 0..8 {
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                env.svm.expire_blockhash();
                let _ = env.send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
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
    }

    let before = env.portfolio_state(victim);
    assert_eq!(before.capital.get(), 0, "setup makes victim insolvent");
    assert!(
        before.pnl.get() < 0,
        "setup leaves real bad debt before terminal wind-down"
    );
    assert!(
        has_active_leg_for_asset(&before, 0) && has_active_leg_for_asset(&before, 1),
        "setup leaves unattributed multi-asset exposure"
    );
    for _ in 0..3 {
        match env.market_state().1.mode {
            MarketModeV16::Resolved => break,
            MarketModeV16::Recovery => {
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 2,
                            observations: vec![],
                        },
                        vec![
                            AccountMeta::new_readonly(env.payer.pubkey(), false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(victim, false),
                        ],
                        &[],
                    )
                    .expect("Recovery has a permissionless terminal continuation");
                assert_cu_within(
                    "cross-margin Recovery terminal continuation",
                    cu,
                    CRANK_CU_LIMIT,
                );
            }
            mode => panic!("permissionless insolvency sequence stalled in {mode:?}"),
        }
    }
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);

    env.svm.expire_blockhash();
    let loser_crank_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![CrankObservationHint {
                    asset_index: u16::MAX,
                    oracle_accounts: u8::MAX,
                }],
            },
            vec![
                AccountMeta::new_readonly(victim_owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(victim, false),
            ],
            &[],
        )
        .expect("resolved deep cross-margin bad debt has public progress");
    assert_cu_within(
        "Resolved PermissionlessCrank unattributed bad debt",
        loser_crank_cu,
        CRANK_CU_LIMIT,
    );
    let victim_after = env.portfolio_state(victim);
    assert_eq!(
        victim_after.capital.get(),
        0,
        "bad-debt loser has no payout"
    );
    assert_eq!(victim_after.pnl.get(), 0, "bad debt cleared terminally");
    assert!(
        percolator::active_bitmap_is_empty(victim_after.active_bitmap.map(|w| w.get())),
        "loser's resolved legs detached"
    );
    env.close_portfolio_with_cu(&victim_owner, victim);

    let winner_dest = env.close_resolved(&cp_owner, cp);
    let winner_payout = env.token_amount(winner_dest);
    assert!(
        (2_000_249..=2_000_250).contains(&winner_payout),
        "winner recovers capital plus the conservative vault-bounded residual, not paper pnl: {winner_payout}"
    );
    env.close_portfolio_with_cu(&cp_owner, cp);
    let (_, g) = env.market_state();
    assert_eq!(
        g.materialized_portfolio_count, 0,
        "all users can dematerialize"
    );
    assert_eq!(g.c_tot, 0, "all senior capital wound down");
    assert!(g.vault <= 1, "at most conservative rounding dust remains");
    assert_eq!(
        env.token_amount(env.vault) as u128,
        g.vault,
        "SPL custody matches accounting"
    );
}

#[test]
fn v16_bpf_permissionless_stale_resolve_is_bounded_and_oracle_free() {
    let mut env = V16CuEnv::new();
    let configure_cu = env.configure_permissionless_resolve_with_cu(5, 1);
    let stale_resolve_cu = env.resolve_stale_permissionless_with_cu(5);
    println!(
        "v16 permissionless stale resolve CU configure={configure_cu}, resolve={stale_resolve_cu}"
    );
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "configure permissionless resolve CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        stale_resolve_cu <= CUSTODY_CU_LIMIT,
        "permissionless stale resolve CU {} exceeded limit {}",
        stale_resolve_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(cfg.permissionless_resolve_stale_slots, 5);
    assert_eq!(cfg.force_close_delay_slots, 1);
    assert_eq!(group.mode, percolator::MarketModeV16::Resolved);
    assert_eq!(group.resolved_slot, 5);
}
