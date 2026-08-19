//! INV-071 - Crank progress.
//!
//! Normative obligation: Every successful crank strictly decreases a finite liveness rank or enters a lower terminal mode.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): expiry matrices, bankruptcy
//! escalation, no-op crank detection, resolved cranks, stale liquidation-budget progress, priority
//! selection, and current solvent partial-liquidation progress. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_prospective_loss_expiry_matrix_discovers_resolved_exit_lock() {
    const PRICE: u64 = 100;
    const LOW_PRICE: u64 = 98;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: PRICE,
        max_price_move_bps_per_slot: 200,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let neutral_owner = Keypair::new();
    let fee_peer_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let neutral = env.create_portfolio(&neutral_owner);
    let fee_peer = env.create_portfolio(&fee_peer_owner);
    for (owner, portfolio) in [
        (&long_owner, long),
        (&short_owner, short),
        (&neutral_owner, neutral),
        (&fee_peer_owner, fee_peer),
    ] {
        env.deposit(owner, portfolio, DEPOSIT);
    }
    env.trade_with_cu(&long_owner, long, &short_owner, short, SIZE_Q, PRICE, 0);
    env.trade_asset_with_cu(
        1,
        &short_owner,
        short,
        &fee_peer_owner,
        fee_peer,
        SIZE_Q,
        PRICE,
        0,
    );
    env.top_up_backing_bucket(0, 1_000_000, 8);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, LOW_PRICE);
    env.push_auth_mark_for_asset_as_admin(1, 2, LOW_PRICE);
    for asset_index in [0, 1] {
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
    }
    env.svm.warp_to_slot(3);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert!(env
        .portfolio_state(short)
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    env.trade_with_cu(
        &short_owner,
        short,
        &neutral_owner,
        neutral,
        SIZE_Q,
        LOW_PRICE,
        0,
    );

    env.svm.warp_to_slot(9);
    for _ in 0..8 {
        if env.market_state().1.assets[0].slot_last == 9 {
            break;
        }
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 9,
                observations: crank_observations(0),
            },
        );
    }
    let long_before = env.portfolio_state(long);
    let market_before = env.market_state().1;
    assert_eq!(long_before.pnl.get(), 0);
    assert!(
        market_before.assets[0].k_long < active_leg_for_asset(&long_before, 0).k_snap,
        "the target must retain a prospective negative K delta"
    );
    assert!(long_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    assert_eq!(
        market_before.source_backing_buckets[0].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(market_before.source_backing_buckets[0].expiry_slot, 8);
    env.resolve();

    let destinations = [
        env.token_account(long_owner.pubkey(), 0),
        env.token_account(short_owner.pubkey(), 0),
        env.token_account(neutral_owner.pubkey(), 0),
        env.token_account(fee_peer_owner.pubkey(), 0),
    ];
    let accounts = [
        (&long_owner, long),
        (&short_owner, short),
        (&neutral_owner, neutral),
        (&fee_peer_owner, fee_peer),
    ];
    let mut rejected = 0usize;
    for ((owner, portfolio), destination) in accounts.into_iter().zip(destinations).cycle().take(64)
    {
        env.svm.expire_blockhash();
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_portfolio = env.svm.get_account(&portfolio).unwrap();
        let before_vault = env.svm.get_account(&env.vault).unwrap();
        let close = env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        if close.is_err() {
            rejected += 1;
            assert_eq!(env.svm.get_account(&env.market).unwrap(), before_market);
            assert_eq!(env.svm.get_account(&portfolio).unwrap(), before_portfolio);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), before_vault);
        }
    }

    let states = [
        env.portfolio_state(long),
        env.portfolio_state(short),
        env.portfolio_state(neutral),
        env.portfolio_state(fee_peer),
    ];
    let locked = states.iter().any(|portfolio| {
        portfolio.capital.get() != 0
            || !percolator::active_bitmap_is_empty(active_bitmap(portfolio))
    });
    assert_eq!(
        rejected, 16,
        "only the prospective-loss account should reject once per schedule round"
    );
    assert!(locked, "rejections retained no funded user state");
    assert!(has_active_leg_for_asset(&states[0], 0));
    assert!(states[0].capital.get() != 0);
    assert_eq!(env.token_amount(destinations[0]), 0);
}

#[test]
fn v16_program_prospective_source_expiry_prerequisite_matrix_keeps_exit_live() {
    const PRICE: u64 = 100;
    const LOW_PRICE: u64 = 98;
    const REBOUND_PRICE: u64 = 99;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE,
        max_price_move_bps_per_slot: 200,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let neutral_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let neutral = env.create_portfolio(&neutral_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    env.trade_with_cu(&long_owner, long, &short_owner, short, SIZE_Q, PRICE, 0);
    env.top_up_backing_bucket(0, 93, 8);
    env.top_up_backing_bucket(1, 32, 8);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, LOW_PRICE);
    env.crank(
        neutral,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert_eq!(env.portfolio_state(long).pnl.get(), 0);
    assert!(env.portfolio_state(long).capital.get() < DEPOSIT);

    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(0, 4, REBOUND_PRICE);
    env.crank(
        neutral,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );
    let long_before = env.portfolio_state(long);
    let short_before = env.portfolio_state(short);
    assert_eq!(long_before.pnl.get(), 0);
    assert_eq!(short_before.pnl.get(), 0);
    assert!(long_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    assert!(short_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));

    env.svm.warp_to_slot(9);
    for _ in 0..8 {
        if env.market_state().1.assets[0].slot_last == 9 {
            break;
        }
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 9,
                observations: crank_observations(0),
            },
        );
    }
    let before_resolve = env.market_state().1;
    assert_eq!(before_resolve.assets[0].slot_last, 9);
    assert_eq!(
        before_resolve.source_backing_buckets[0].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(before_resolve.source_backing_buckets[0].expiry_slot, 8);
    assert_eq!(before_resolve.pnl_matured_pos_tot, 0);
    env.resolve();

    let long_destination = env.token_account(long_owner.pubkey(), 0);
    let short_destination = env.token_account(short_owner.pubkey(), 0);
    let mut rejected = 0usize;
    for (owner, portfolio, destination) in [
        (&long_owner, long, long_destination),
        (&short_owner, short, short_destination),
    ]
    .into_iter()
    .cycle()
    .take(32)
    {
        env.svm.expire_blockhash();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let close = env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        if close.is_err() {
            rejected += 1;
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        }
    }

    let long_after = env.portfolio_state(long);
    let short_after = env.portfolio_state(short);
    let long_locked = has_active_leg_for_asset(&long_after, 0) || long_after.capital.get() != 0;
    let short_locked = has_active_leg_for_asset(&short_after, 0) || short_after.capital.get() != 0;
    assert_eq!(rejected, 0, "the pinned predecessor unexpectedly locked");
    assert!(!long_locked && !short_locked);
    assert_eq!(env.token_amount(long_destination), 99_900_000);
    assert_eq!(env.token_amount(short_destination), 100_100_000);
}

#[test]
fn v16_program_b_budget_lock_prerequisite_rejects_post_adl_basis_reissue() {
    const SCALE: u64 = 100_000_000;
    const INITIAL_PRICE: u64 = 10_000 * SCALE;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        min_nonzero_mm_req: 1,
        min_nonzero_im_req: 2,
        maintenance_margin_bps: 250,
        initial_margin_bps: 500,
        max_trading_fee_bps: 10,
        liquidation_fee_bps: 0,
        liquidation_fee_cap: 0,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, INITIAL_PRICE);
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let accounts = [
        env.create_portfolio(&owners[0]),
        env.create_portfolio(&owners[1]),
        env.create_portfolio(&owners[2]),
    ];
    for index in 0..accounts.len() {
        env.deposit(&owners[index], accounts[index], 20_000 * u128::from(SCALE));
    }
    let crank_at = |env: &mut V16CuEnv, portfolio: Pubkey, slot: u64| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("public crank in B-budget setup")
    };

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 9_976 * SCALE);
    crank_at(&mut env, accounts[2], 1);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        -(29 * POS_SCALE as i128),
        9_976 * SCALE,
        3,
    );
    env.rebalance_reduce_with_cu(&owners[0], accounts[0], 0, 25 * POS_SCALE);
    let adl = env.market_state().1;
    assert!(adl.assets[0].a_long < percolator::ADL_ONE);
    assert_eq!(adl.assets[0].oi_eff_long_q, 4 * POS_SCALE);
    assert_eq!(adl.assets[0].oi_eff_short_q, 4 * POS_SCALE);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let account0_before = env.svm.get_account(&accounts[0]).unwrap();
    let account1_before = env.svm.get_account(&accounts[1]).unwrap();
    let account2_before = env.svm.get_account(&accounts[2]).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let prerequisite = env.try_trade_asset_with_cu(
        0,
        &owners[2],
        accounts[2],
        &owners[0],
        accounts[0],
        32 * POS_SCALE as i128,
        9_976 * SCALE,
        0,
    );
    let error = prerequisite
        .expect_err("the former resolved B-budget lock prefix must stop at post-ADL basis reissue");
    assert!(
        error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
        "B-budget prerequisite reached the wrong gate: {error}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&accounts[0]).unwrap(), account0_before);
    assert_eq!(env.svm.get_account(&accounts[1]).unwrap(), account1_before);
    assert_eq!(env.svm.get_account(&accounts[2]).unwrap(), account2_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let raw_before = active_leg_for_asset(&env.portfolio_state(accounts[0]), 0)
        .basis_pos_q
        .unsigned_abs();
    let exit_cu = env.rebalance_reduce_with_cu(&owners[0], accounts[0], 0, POS_SCALE);
    assert_cu_within("B-budget prefix owner exit", exit_cu, CUSTODY_CU_LIMIT);
    let after_exit = env.market_state().1;
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(accounts[0]), 0)
            .basis_pos_q
            .unsigned_abs(),
        raw_before - POS_SCALE
    );
    assert_eq!(after_exit.assets[0].oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(after_exit.assets[0].oi_eff_short_q, 3 * POS_SCALE);
}

#[test]
fn v16_program_bankruptcy_escalation_matrix_commits_recovery_and_resolves() {
    const OPEN_PRICE: u64 = 1_000_000;
    const ADVERSE_PRICE: u64 = 1_070_000;

    for short_capital in [55_000u128, 56_000] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            public_b_chunk_atoms: 1,
            max_bankrupt_close_lifetime_slots: 1,
            ..production_risk_params()
        });
        env.update_liquidation_fee_policy_with_cu(0);
        env.configure_auth_mark_with_cu(0, OPEN_PRICE);

        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 100_000_000);
        env.deposit(&short_owner, short, short_capital);
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

        let mut recovery_transition = None;
        for slot in 1..=40u64 {
            env.svm.warp_to_slot(slot);
            let _ = env.push_auth_mark_with_cu(slot, ADVERSE_PRICE);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let (_, group_before) = env.market_state();
            let short_before = env.svm.get_account(&short).unwrap();
            let cert_before = health_cert(&env.portfolio_state(short));
            env.svm.expire_blockhash();
            let cu = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(short, false),
                    ],
                    &[],
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "bankruptcy progress crank failed at slot {slot}, cert={cert_before:?}: {error}"
                    )
                });
            if env.market_state().1.mode == MarketModeV16::Recovery {
                recovery_transition =
                    Some((cu, market_before, group_before, short_before, cert_before));
                break;
            }
        }

        let (recovery_cu, market_before, group_before, short_before, cert_before) =
            recovery_transition.expect("bankruptcy must reach Recovery in bounded public cranks");
        assert!(
            cert_before.valid && cert_before.certified_liq_deficit != 0,
            "the recovery transition must start from a current liquidatable account"
        );
        assert_cu_within(
            "bankruptcy escalation recovery declaration",
            recovery_cu,
            CRANK_CU_LIMIT,
        );
        let (_, recovered) = env.market_state();
        assert_eq!(recovered.mode, MarketModeV16::Recovery);
        assert_eq!(
            recovered.recovery_reason,
            Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
        );
        assert_ne!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
        assert_eq!(recovered.vault, group_before.vault);
        assert_eq!(recovered.c_tot, group_before.c_tot);
        assert_eq!(recovered.insurance, group_before.insurance);
        assert_eq!(recovered.vault as u64, env.token_amount(env.vault));

        let recovery_market = env.svm.get_account(&env.market).unwrap();
        let recovery_short = env.svm.get_account(&short).unwrap();
        env.svm.expire_blockhash();
        let finalize_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[],
            )
            .expect("Recovery must have a bounded permissionless Resolved continuation");
        assert_cu_within(
            "bankruptcy escalation Recovery-to-Resolved",
            finalize_cu,
            CRANK_CU_LIMIT,
        );
        let (_, resolved) = env.market_state();
        assert_eq!(resolved.mode, MarketModeV16::Resolved);
        assert_ne!(env.svm.get_account(&env.market).unwrap(), recovery_market);
        assert_eq!(env.svm.get_account(&short).unwrap(), recovery_short);
        assert_eq!(resolved.vault, recovered.vault);
        assert_eq!(resolved.c_tot, recovered.c_tot);
        assert_eq!(resolved.insurance, recovered.insurance);
        assert_eq!(resolved.vault as u64, env.token_amount(env.vault));
    }
}

#[derive(Debug)]
struct MicroPriceScheduleOutcome {
    effective_price: u64,
    raw_target: u64,
    asset_slot_last: u64,
    successful_calls: usize,
    zero_delta_clock_advances: usize,
    vault_tokens: u64,
}

fn run_micro_price_schedule(eager: bool) -> MicroPriceScheduleOutcome {
    const PRICE: u64 = 100;
    const TARGET: u64 = 200;
    const FINAL_SLOT: u64 = 5;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, PRICE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 10_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    let vault_tokens = env.token_amount(env.vault);

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, TARGET);
    let schedule: Vec<u64> = if eager {
        (1..=FINAL_SLOT).collect()
    } else {
        vec![FINAL_SLOT]
    };
    let mut successful_calls = 0usize;
    let mut zero_delta_clock_advances = 0usize;
    for slot in schedule {
        env.svm.warp_to_slot(slot);
        let (_, before) = env.market_state();
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[],
            )
            .expect("public price crank");
        assert!(cu < 1_400_000);
        successful_calls += 1;
        let (_, after) = env.market_state();
        if after.assets[0].effective_price == before.assets[0].effective_price
            && after.assets[0].slot_last > before.assets[0].slot_last
        {
            zero_delta_clock_advances += 1;
        }
        assert_eq!(env.token_amount(env.vault), vault_tokens);
    }

    let (_, group) = env.market_state();
    MicroPriceScheduleOutcome {
        effective_price: group.assets[0].effective_price,
        raw_target: group.assets[0].raw_oracle_target_price,
        asset_slot_last: group.assets[0].slot_last,
        successful_calls,
        zero_delta_clock_advances,
        vault_tokens: env.token_amount(env.vault),
    }
}

#[test]
fn v16_program_micro_price_schedule_is_partition_invariant_and_eventually_progresses() {
    let delayed = run_micro_price_schedule(false);
    let eager = run_micro_price_schedule(true);
    assert_eq!(delayed.raw_target, 200);
    assert_eq!(eager.raw_target, delayed.raw_target);
    assert!(
        delayed.effective_price > 100,
        "five elapsed slots must make one price atom representable: {delayed:?}"
    );
    assert_eq!(
        eager.effective_price, delayed.effective_price,
        "carried sub-atom movement must make eager and delayed cranks equivalent: eager={eager:?}, delayed={delayed:?}"
    );
    assert_eq!(eager.asset_slot_last, 5);
    assert_eq!(delayed.asset_slot_last, eager.asset_slot_last);
    assert_eq!(eager.successful_calls, 5);
    assert_eq!(delayed.successful_calls, 1);
    assert_eq!(
        eager.zero_delta_clock_advances, 4,
        "four sub-atom steps should carry into the fifth visible price move: {eager:?}"
    );
    assert_eq!(delayed.zero_delta_clock_advances, 0);
    assert_eq!(eager.vault_tokens, delayed.vault_tokens);
}

#[test]
fn v16_attack_resolved_permissionless_crank_survives_drained_owner_system_account() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let dest = env.token_account(owner.pubkey(), 0);
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
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![CrankObservationHint {
                    asset_index: u16::MAX,
                    oracle_accounts: u8::MAX,
                }],
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("post-timeout resolved PermissionlessCrank should not depend on owner lamports");
    assert_cu_within(
        "post-timeout resolved PermissionlessCrank drained owner account",
        cu,
        CRANK_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "resolved public crank still pays the portfolio owner's token account"
    );
    assert_eq!(env.token_amount(env.vault), 0);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

#[test]
fn v16_attack_stale_liquidation_budget_observation_crank_progresses_without_reward_or_value() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const OBS_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);

    set_test_clock(&mut env, OPEN_SLOT, 100);
    let feed0 = [0x46u8; 32];
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
    .expect("configure asset-0 hybrid oracle");

    let target_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let target = env.create_portfolio(&target_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&cranker_owner, cranker, 1_000);

    set_test_clock(&mut env, OBS_SLOT, 101);
    let fresh0 = env.set_pyth_price_with_conf(&feed0, (MARK + 10_000) as i64, -6, 0, 101);
    let target_before = env.portfolio_state(target);
    let cranker_before = env.svm.get_account(&cranker).unwrap();

    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: OBS_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new_readonly(fresh0, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    );
    assert!(
        accepted.is_ok(),
        "stale liquidation budget must not roll back otherwise valid observation-only progress: {accepted:?}"
    );
    assert_cu_within(
        "stale close_q observation-only crank",
        accepted.unwrap(),
        CRANK_CU_LIMIT,
    );

    let (_, after_group) = env.market_state();
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        MARK + 10_000,
        "observation-only crank commits the supplied oracle update"
    );
    assert_eq!(
        env.portfolio_state(target).capital.get(),
        target_before.capital.get(),
        "stale-budget observation crank must not credit or debit target capital"
    );
    assert_eq!(
        env.portfolio_state(target).pnl.get(),
        target_before.pnl.get(),
        "stale-budget observation crank must not move target PnL"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(target))),
        "stale-budget observation crank must not create target exposure"
    );
    assert_eq!(
        env.svm.get_account(&cranker).unwrap(),
        cranker_before,
        "observation-only stale-budget crank pays no liquidation reward"
    );
}

#[test]
fn v16_attack_auto_crank_prioritizes_b_stale_over_liquidation_reward_tail() {
    const OPEN_MARK: u64 = 100;
    const LIQ_MARK: u64 = 300;
    const OPEN_SLOT: u64 = 1;
    const LIQ_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    });
    env.top_up_insurance(1_000_000);
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.svm.warp_to_slot(OPEN_SLOT);
    env.configure_auth_mark_with_cu(OPEN_SLOT, OPEN_MARK);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.deposit(&cranker_owner, cranker, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        OPEN_MARK,
        0,
    );

    env.svm.warp_to_slot(LIQ_SLOT);
    env.push_auth_mark_with_cu(LIQ_SLOT, LIQ_MARK);
    for slot in [LIQ_SLOT, LIQ_SLOT + 1] {
        env.svm.warp_to_slot(slot);
        env.crank(
            short_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    let liquidatable_before = env.portfolio_state(short_account);
    let cert_before = health_cert(&liquidatable_before);
    assert!(
        cert_before.valid
            && cert_before.certified_liq_deficit != 0
            && cert_before.certified_equity > 0,
        "setup must produce a current solvent liquidatable short before adding B-stale overlap: {cert_before:?}"
    );

    env.mark_b_stale_gap(short_account, 0, 3);
    let overlapped_before = env.portfolio_state(short_account);
    let leg_before = active_leg_for_asset(&overlapped_before, 0);
    assert_eq!(leg_before.side, SideV16::Short);
    assert!(
        leg_before.b_stale && overlapped_before.b_stale_state != 0,
        "setup must add a real B-stale rank on top of the liquidatable account"
    );
    assert!(
        health_cert(&overlapped_before).certified_liq_deficit != 0,
        "B-stale setup must preserve the liquidatable overlap"
    );

    let (_, group_before) = env.market_state();
    let cranker_before = env.svm.get_account(&cranker).unwrap();
    let cranker_capital_before = env.portfolio_state(cranker).capital.get();
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: LIQ_SLOT + 1,
                observations: vec![],
            },
            vec![
                AccountMeta::new(cranker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_account, false),
                AccountMeta::new(cranker, false),
            ],
            &[&cranker_owner],
        )
        .expect("B-stale/liquidatable overlap must make B-settlement progress");
    assert_cu_within(
        "PermissionlessCrank B-stale/liquidatable overlap",
        cu,
        CRANK_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    let after = env.portfolio_state(short_account);
    let leg_after = active_leg_for_asset(&after, 0);
    assert_eq!(
        leg_after.b_snap,
        leg_before.b_snap + 1,
        "overlap selector takes the higher-priority B-settlement step"
    );
    assert_eq!(
        leg_after.basis_pos_q, leg_before.basis_pos_q,
        "hostile close_q must not liquidate while B settlement has priority"
    );
    assert_eq!(
        group_after.insurance, group_before.insurance,
        "B-settlement overlap path pays no liquidation fee"
    );
    assert_eq!(
        env.svm.get_account(&cranker).unwrap(),
        cranker_before,
        "non-liquidation overlap path must not rewrite the reward tail account"
    );
    assert_eq!(
        env.portfolio_state(cranker).capital.get(),
        cranker_capital_before,
        "non-liquidation overlap path pays no cranker reward"
    );
    assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_attack_auto_crank_reaches_later_material_liquidation_past_tiny_first_leg() {
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_040_000;
    const TINY_Q: i128 = 1;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, MARK);
    env.configure_auth_mark_for_asset_as_admin(1, 1, MARK);

    let victim_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&victim_owner, victim, 60_000);
    env.deposit(&counterparty_owner, counterparty, 2_000_000);

    // Asset 0 deliberately occupies the first active slot with the minimum representable public
    // position quantum. It must still be removable rather than shadowing the material asset-1 loss.
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &counterparty_owner,
        counterparty,
        -TINY_Q,
        MARK,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &victim_owner,
        victim,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        MARK,
        0,
    );
    assert_eq!(leg(&env.portfolio_state(victim), 0).asset_index, 0);

    // Reach the adverse price through the production 24-bps/slot circuit breaker while leaving the
    // victim untouched and stale. The counterparty is only the public accrual vehicle.
    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, ADVERSE_MARK);
        env.crank(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        );
    }
    env.crank(
        victim,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: vec![],
        },
    );

    let before = env.portfolio_state(victim);
    assert!(health_cert(&before).certified_liq_deficit > 0);
    assert!(has_active_leg_for_asset(&before, 0));
    let material_before = active_leg_for_asset(&before, 1).basis_pos_q.unsigned_abs();

    // Every successful call is engine-selected. The tiny first leg may be removed first, but it
    // must not permanently shadow the later leg that carries the material deficit.
    let mut material_after = material_before;
    for _ in 0..6 {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(victim, false),
            ],
            &[],
        )
        .expect("one honest auto-crank step");
        let state = env.portfolio_state(victim);
        material_after = if has_active_leg_for_asset(&state, 1) {
            active_leg_for_asset(&state, 1).basis_pos_q.unsigned_abs()
        } else {
            0
        };
        if material_after < material_before {
            break;
        }
    }
    assert!(
        material_after < material_before,
        "tiny first leg must not shadow liquidation of the later losing leg"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(victim), 0),
        "the minimum-quantum first leg must clear before the later material liquidation progresses"
    );
}

// the keeper has no liquidation-size input.
#[test]
fn v16_program_auto_crank_current_solvent_partial_liquidation_makes_progress() {
    let mut env = V16CuEnv::new();
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
    let position_epoch_after_trade = env.portfolio_position_epoch(short_account);

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

    let before_group = env.market_state().1;
    let before_short = env.portfolio_state(short_account);
    let before_cert = health_cert(&before_short);
    assert_eq!(
        env.portfolio_position_epoch(short_account),
        position_epoch_after_trade,
        "refresh-only cranks must not invalidate signed position consent"
    );
    assert!(
        before_cert.certified_liq_deficit != 0 && before_cert.certified_equity > 0,
        "setup must be solvent but liquidatable before partial liquidation: {before_cert:?}"
    );
    let oi_pre = before_group.assets[0].oi_eff_short_q;

    env.svm.expire_blockhash();
    let partial = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    assert!(
        partial.is_ok(),
        "current solvent liquidation must not require observations to make progress: {partial:?}"
    );

    let after_group = env.market_state().1;
    let after_short = env.portfolio_state(short_account);
    let closed = oi_pre.saturating_sub(after_group.assets[0].oi_eff_short_q);
    assert!(closed > 0, "partial liquidation must reduce open interest");
    assert_eq!(
        env.portfolio_position_epoch(short_account),
        position_epoch_after_trade + 1,
        "a successful liquidation must advance the position episode exactly once"
    );
    assert!(
        closed < oi_pre,
        "solvent liquidation should preserve the engine-selected remaining position: closed={closed}"
    );
    assert!(
        has_active_leg_for_asset(&after_short, 0),
        "partial close should leave the remaining position active"
    );
    assert_eq!(
        health_cert(&after_short).certified_liq_deficit,
        0,
        "engine-selected partial close restores maintenance health"
    );
    assert_eq!(
        after_group.vault, before_group.vault,
        "liquidation fee is internal accounting, not a vault mint"
    );
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}

// security.md sweep — crank idempotency / double-accrual (#32 race): re-cranking an asset at the SAME
// slot must be a no-op. If a second same-slot crank re-applies the price move/funding, an attacker
// could double-realize a counterparty's loss or double-charge funding. We first crank to the
// settlement fixed point (§6.1/§6.2 needs multiple passes), then assert re-cranking is an exact no-op.
#[test]
fn v16_regression_crank_idempotent_at_settlement_fixed_point() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    let crank = |env: &mut V16CuEnv, p: Pubkey, slot: u64| {
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
    };
    // crank many passes at slot 11 and watch the short's capital: it must CONVERGE to a fixed point
    // (settlement completing), not keep dropping (which would be double-accrual).
    env.svm.warp_to_slot(11);
    for _ in 0..8 {
        for p in [sh, lo] {
            crank(&mut env, p, 11);
        }
    } // crank to the settlement fixed point
    let lo1 = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let sh1 = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g1) = env.market_state();
    let ep1 = g1.assets[0].effective_price;
    for _ in 0..3 {
        for p in [sh, lo] {
            crank(&mut env, p, 11);
        }
    }
    let lo2 = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let sh2 = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g2) = env.market_state();

    assert_eq!(
        g2.assets[0].effective_price, ep1,
        "effective price unchanged by same-slot re-crank"
    );
    assert_eq!(
        (lo2.capital.get(), lo2.pnl.get()),
        (lo1.capital.get(), lo1.pnl.get()),
        "long pnl/capital not double-accrued"
    );
    assert_eq!(
        (sh2.capital.get(), sh2.pnl.get()),
        (sh1.capital.get(), sh1.pnl.get()),
        "short pnl/capital not double-accrued"
    );
    assert_eq!(
        g2.assets[0].f_long_num, g1.assets[0].f_long_num,
        "funding ledger not double-applied"
    );
    assert_eq!(g2.vault, 2_000_000, "vault conserved");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}
