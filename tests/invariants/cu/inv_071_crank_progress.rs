//! INV-071 - Crank progress.
//!
//! Normative obligation: Every successful crank strictly decreases a finite liveness rank or enters a lower terminal mode.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_b_budget_prerequisite_matrix_hits_resolved_adl_lock`, `v16_program_bankruptcy_escalation_matrix_discovers_funded_survivor_lock`, `v16_program_micro_price_schedule_matrix_discovers_clock_consuming_noop_cranks`, `v16_attack_resolved_permissionless_crank_survives_drained_owner_system_account`, `v16_attack_stale_liquidation_budget_observation_crank_progresses_without_reward_or_value`, `v16_attack_auto_crank_prioritizes_b_stale_over_liquidation_reward_tail`, `v16_attack_auto_crank_reaches_later_material_liquidation_past_tiny_first_leg`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_b_budget_prerequisite_matrix_hits_resolved_adl_lock() {
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
    env.trade_asset_with_cu(
        0,
        &owners[2],
        accounts[2],
        &owners[0],
        accounts[0],
        32 * POS_SCALE as i128,
        9_976 * SCALE,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        POS_SCALE as i128,
        9_976 * SCALE,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        13 * POS_SCALE as i128,
        9_976 * SCALE,
        7,
    );
    crank_at(&mut env, accounts[0], 1);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 9_931 * SCALE);
    crank_at(&mut env, accounts[2], 2);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        POS_SCALE as i128,
        9_931 * SCALE,
        0,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 9_860 * SCALE);
    crank_at(&mut env, accounts[0], 3);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        POS_SCALE as i128,
        9_860 * SCALE,
        0,
    );

    env.svm.warp_to_slot(4);
    env.push_auth_mark_with_cu(4, 9_801 * SCALE);
    crank_at(&mut env, accounts[2], 4);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        POS_SCALE as i128,
        9_801 * SCALE,
        0,
    );

    env.resolve();
    let dest = env.token_account(owners[2].pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&accounts[2]).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let prerequisite = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owners[2].pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(accounts[2], false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        prerequisite.is_err(),
        "the PR216 prerequisite unexpectedly landed on the pinned engine"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&accounts[2]).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let blocked = env.portfolio_state(accounts[2]);
    assert!(blocked.capital.get() != 0 || blocked.pnl.get() != 0);
}

#[test]
fn v16_program_bankruptcy_escalation_matrix_discovers_funded_survivor_lock() {
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

        let mut blocked_slot = None;
        for slot in 1..=40u64 {
            env.svm.warp_to_slot(slot);
            let _ = env.push_auth_mark_with_cu(slot, ADVERSE_PRICE);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let short_before = env.svm.get_account(&short).unwrap();
            let cert_before = health_cert(&env.portfolio_state(short));
            env.svm.expire_blockhash();
            let result = env.send(
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
            );
            if let Err(error) = result {
                if cert_before.valid && cert_before.certified_liq_deficit != 0 {
                    assert!(
                        error.contains("Custom(23)")
                            || error.contains("custom program error: 0x17"),
                        "current bankrupt crank failed for an unrelated reason: {error}"
                    );
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                    assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
                    blocked_slot = Some(slot);
                    break;
                }
            }
        }

        let blocked_slot = blocked_slot.expect(
            "a one-atom close budget and one-slot lifetime must reach the escalation boundary",
        );
        let (_, blocked_group) = env.market_state();
        assert_eq!(blocked_group.mode, MarketModeV16::Live);
        assert!(blocked_group.vault >= blocked_group.c_tot + blocked_group.insurance);
        assert_eq!(blocked_group.vault as u64, env.token_amount(env.vault));
        assert!(
            env.portfolio_state(long).capital.get() != 0
                || env.portfolio_state(long).pnl.get() != 0,
            "the rollback boundary must retain funded counterparty value"
        );

        let fixed_point_market = env.svm.get_account(&env.market).unwrap();
        let fixed_point_long = env.svm.get_account(&long).unwrap();
        let fixed_point_short = env.svm.get_account(&short).unwrap();
        let fixed_point_vault = env.token_amount(env.vault);
        for _ in 0..3 {
            env.svm.expire_blockhash();
            let retry = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: blocked_slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[],
            );
            assert!(
                retry.is_err(),
                "identical honest crank unexpectedly progressed"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                fixed_point_market
            );
            assert_eq!(env.svm.get_account(&long).unwrap(), fixed_point_long);
            assert_eq!(env.svm.get_account(&short).unwrap(), fixed_point_short);
            assert_eq!(env.token_amount(env.vault), fixed_point_vault);
        }

        let short_q = active_leg_for_asset(&env.portfolio_state(short), 0)
            .basis_pos_q
            .unsigned_abs();
        env.svm.expire_blockhash();
        let reduce_cu = env
            .send(
                ProgInstruction::RebalanceReduce {
                    asset_index: 0,
                    reduce_q: short_q,
                },
                vec![
                    AccountMeta::new(short_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[&short_owner],
            )
            .expect("owner reduction remains a bounded alternative to crank escalation");
        assert_cu_within(
            "bankruptcy escalation owner reduction",
            reduce_cu,
            CUSTODY_CU_LIMIT,
        );
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(short), 0),
            "owner reduction must remove the bankrupt leg"
        );

        let mut long_reduced = false;
        let mut last_long_error = String::new();
        let mut fixed_point_repeats = 0usize;
        for _ in 0..32 {
            if !has_active_leg_for_asset(&env.portfolio_state(long), 0) {
                long_reduced = true;
                break;
            }
            let long_q = active_leg_for_asset(&env.portfolio_state(long), 0)
                .basis_pos_q
                .unsigned_abs();
            let market_before_reduce = env.svm.get_account(&env.market).unwrap();
            let long_before_reduce = env.svm.get_account(&long).unwrap();
            env.svm.expire_blockhash();
            match env.send(
                ProgInstruction::RebalanceReduce {
                    asset_index: 0,
                    reduce_q: long_q,
                },
                vec![
                    AccountMeta::new(long_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[&long_owner],
            ) {
                Ok(_) => {
                    long_reduced = true;
                    break;
                }
                Err(error) => {
                    last_long_error = error;
                    assert_eq!(
                        env.svm.get_account(&env.market).unwrap(),
                        market_before_reduce
                    );
                    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_reduce);
                }
            }
            let market_before_crank = env.svm.get_account(&env.market).unwrap();
            let long_before_crank = env.svm.get_account(&long).unwrap();
            env.svm.expire_blockhash();
            let crank = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: blocked_slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[],
            );
            let unchanged = env.svm.get_account(&env.market).unwrap() == market_before_crank
                && env.svm.get_account(&long).unwrap() == long_before_crank;
            if crank.is_err() {
                assert!(unchanged, "rejected crank must roll back exactly");
            }
            fixed_point_repeats = if unchanged {
                fixed_point_repeats + 1
            } else {
                0
            };
            if fixed_point_repeats >= 3 {
                break;
            }
        }
        assert!(
            !long_reduced && has_active_leg_for_asset(&env.portfolio_state(long), 0),
            "the generated survivor no longer reproduces the funded lock"
        );
        assert!(
            fixed_point_repeats >= 3,
            "continuations did not reach a fixed point"
        );
        let survivor = env.portfolio_state(long);
        assert!(
            survivor.capital.get() != 0 || survivor.pnl.get() != 0,
            "the locked survivor must retain user value"
        );
        assert!(
            !last_long_error.is_empty(),
            "owner reduction must have been attempted at the fixed point"
        );
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
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
fn v16_program_micro_price_schedule_matrix_discovers_clock_consuming_noop_cranks() {
    let delayed = run_micro_price_schedule(false);
    let eager = run_micro_price_schedule(true);
    assert_eq!(delayed.raw_target, 200);
    assert_eq!(eager.raw_target, delayed.raw_target);
    assert!(
        delayed.effective_price > 100,
        "five elapsed slots must make one price atom representable: {delayed:?}"
    );
    assert_eq!(
        eager.effective_price, 100,
        "per-slot cranks must reproduce target pinning: {eager:?}"
    );
    assert_eq!(eager.asset_slot_last, 5);
    assert_eq!(eager.successful_calls, 5);
    assert_eq!(eager.zero_delta_clock_advances, 5);
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
