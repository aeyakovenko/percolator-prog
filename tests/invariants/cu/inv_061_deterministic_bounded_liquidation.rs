//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: Liquidation is deterministic, risk reducing, OI coherent, and bounded at maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_post_adl_transfer_matrix_discovers_backing_drain`, `v16_program_reset_carry_liquidation_matrix_discovers_crank_lock`, `v16_attack_liquidation_reward_share_without_tail_still_progresses`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[derive(Debug)]
struct PostAdlTransferOutcome {
    opposing_loss: u128,
    converted: u128,
    withdrawn: u64,
    backing_consumed_num: u128,
}

fn run_post_adl_transfer_world(split_before_mark: bool) -> PostAdlTransferOutcome {
    const OPEN_PRICE: u64 = 1_000_000;
    const CLOSE_PRICE: u64 = 500_000;
    const OPEN_Q: u128 = POS_SCALE / 10;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, OPEN_PRICE);
    env.top_up_backing_bucket(0, 200_000, 10_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let relay_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let successor = env.create_portfolio(&successor_owner);
    let relay = env.create_portfolio(&relay_owner);
    for (owner, portfolio) in [
        (&long_owner, long),
        (&short_owner, short),
        (&successor_owner, successor),
        (&relay_owner, relay),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        OPEN_PRICE,
        0,
    );
    env.rebalance_reduce_with_cu(&long_owner, long, 0, OPEN_Q / 2);
    let adl = env.market_state().1;
    assert_eq!(adl.assets[0].oi_eff_long_q, OPEN_Q / 2);
    assert_eq!(adl.assets[0].oi_eff_short_q, OPEN_Q / 2);
    assert!(adl.assets[0].a_short < percolator::ADL_ONE);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short), 0)
            .basis_pos_q
            .unsigned_abs(),
        OPEN_Q
    );

    let (claim_owner, claimant) = if split_before_mark {
        for _ in 0..2 {
            env.svm.expire_blockhash();
            env.try_trade_asset_with_cu(
                0,
                &short_owner,
                short,
                &successor_owner,
                successor,
                (OPEN_Q / 2) as i128,
                OPEN_PRICE,
                0,
            )
            .expect("split transfer reissues the full raw short basis");
        }
        assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(successor), 0)
                .basis_pos_q
                .unsigned_abs(),
            OPEN_Q
        );
        (&successor_owner, successor)
    } else {
        (&short_owner, short)
    };

    let account_value = |account: &percolator::PortfolioAccountV16Account| {
        account.capital.get() as i128 + account.pnl.get()
    };
    let long_value_before = account_value(&env.portfolio_state(long));
    let backing_before = env.market_state().1.source_backing_buckets[0];
    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, CLOSE_PRICE);
    for portfolio in [long, claimant] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    let long_value_after = account_value(&env.portfolio_state(long));
    let opposing_loss = (long_value_before - long_value_after) as u128;
    assert!(opposing_loss > 0);
    assert!(env.portfolio_state(claimant).pnl.get() > 0);

    for _ in 0..2 {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            claim_owner,
            claimant,
            &relay_owner,
            relay,
            (OPEN_Q / 2) as i128,
            CLOSE_PRICE,
            0,
        )
        .expect("post-mark split transfer leaves the profitable claimant flat");
    }
    assert!(!has_active_leg_for_asset(&env.portfolio_state(claimant), 0));
    let capital_before = env.portfolio_state(claimant).capital.get();
    env.convert_released_pnl_with_cu(claim_owner, claimant, u128::MAX);
    let capital_after = env.portfolio_state(claimant).capital.get();
    let converted = capital_after - capital_before;
    assert!(converted > 0);
    let backing_after = env.market_state().1.source_backing_buckets[0];
    let backing_consumed_num = backing_before
        .fresh_unliened_backing_num
        .checked_sub(backing_after.fresh_unliened_backing_num)
        .expect("conversion cannot mint provider backing");

    let destination = env.withdraw(claim_owner, claimant, converted);
    let withdrawn = env.token_amount(destination);
    assert_eq!(withdrawn, converted as u64);
    PostAdlTransferOutcome {
        opposing_loss,
        converted,
        withdrawn,
        backing_consumed_num,
    }
}

#[test]
fn v16_program_post_adl_transfer_extraction_matrix_discovers_backing_drain() {
    let control = run_post_adl_transfer_world(false);
    let split = run_post_adl_transfer_world(true);
    assert_eq!(split.opposing_loss, control.opposing_loss);
    assert!(split.converted > control.converted);
    assert_eq!(split.withdrawn as u128, split.converted);
    assert_eq!(control.withdrawn as u128, control.converted);
    assert!(split.backing_consumed_num > control.backing_consumed_num);
    assert_eq!(
        split.converted - control.converted,
        (split.backing_consumed_num - control.backing_consumed_num) / BOUND_SCALE
    );
}

#[test]
fn v16_program_post_adl_transfer_matrix_discovers_phantom_value() {
    const PRICE: u64 = 100;
    const OPEN_Q: i128 = 13_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        h_max: 6_480_000,
        initial_price: PRICE,
        maintenance_margin_bps: 500,
        initial_margin_bps: 500,
        min_nonzero_mm_req: 59_900,
        min_nonzero_im_req: 60_000,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 10_000_000,
        liquidation_fee_bps: 0,
        maintenance_fee_per_slot: 2_700,
        ..V16CuMarketParams::default()
    });
    env.top_up_backing_bucket(1, 100_000, 10_000);
    let survivor_owner = Keypair::new();
    let liquidated_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let survivor = env.create_portfolio(&survivor_owner);
    let liquidated = env.create_portfolio(&liquidated_owner);
    let successor = env.create_portfolio(&successor_owner);
    env.deposit(&survivor_owner, survivor, 100_000);
    env.deposit(&liquidated_owner, liquidated, 118_900);
    env.deposit(&successor_owner, successor, 100_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, PRICE);

    env.svm.warp_to_slot(8);
    env.trade_with_cu(
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
        survivor,
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

    let adl = env.market_state().1;
    let effective_q = adl.assets[0].oi_eff_long_q;
    let raw_q = active_leg_for_asset(&env.portfolio_state(survivor), 0)
        .basis_pos_q
        .unsigned_abs();
    assert_eq!(raw_q, OPEN_Q.unsigned_abs());
    assert!(effective_q < raw_q);
    assert!(adl.assets[0].a_long < percolator::ADL_ONE);

    env.svm.expire_blockhash();
    let split_cu = env
        .try_trade_asset_with_cu(
            0,
            &survivor_owner,
            survivor,
            &successor_owner,
            successor,
            -((raw_q / 2) as i128),
            PRICE,
            0,
        )
        .expect("vulnerable pin reissues post-ADL raw basis to a fresh portfolio");
    assert_cu_within("post-ADL split transfer", split_cu, TRADE_CU_LIMIT);
    assert!(has_active_leg_for_asset(&env.portfolio_state(successor), 0));

    let account_value = |account: &percolator::PortfolioAccountV16Account| {
        account.capital.get() as i128 + account.pnl.get()
    };
    let before_values = [survivor, liquidated, successor]
        .map(|portfolio| account_value(&env.portfolio_state(portfolio)));
    let vault_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(40);
    env.push_auth_mark_with_cu(40, PRICE + 1);
    for portfolio in [survivor, liquidated, successor] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 40,
                observations: crank_observations(0),
            },
        );
    }
    let after_states = [survivor, liquidated, successor].map(|p| env.portfolio_state(p));
    let after_values = after_states.map(|account| account_value(&account));
    let value_deltas = [0usize, 1, 2].map(|i| after_values[i] - before_values[i]);
    let aggregate_creation: i128 = value_deltas.iter().sum();
    let positive_mark_value: u128 = value_deltas
        .iter()
        .copied()
        .filter(|delta| *delta > 0)
        .map(|delta| delta as u128)
        .sum();
    let negative_mark_value: u128 = value_deltas
        .iter()
        .copied()
        .filter(|delta| *delta < 0)
        .map(i128::unsigned_abs)
        .sum();
    assert!(aggregate_creation > 0);
    assert_eq!(
        positive_mark_value - negative_mark_value,
        aggregate_creation as u128
    );
    assert_eq!(env.token_amount(env.vault), vault_before);

    for (owner, portfolio) in [(&survivor_owner, survivor), (&successor_owner, successor)] {
        let pnl = env.portfolio_state(portfolio).pnl.get();
        if pnl > 0 {
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(&portfolio).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            env.svm.expire_blockhash();
            let conversion = env.send(
                ProgInstruction::ConvertReleasedPnl { amount: u128::MAX },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            );
            assert!(
                conversion.is_err(),
                "active malformed exposure unexpectedly converted phantom value"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        }
    }
}

#[test]
fn v16_program_reset_carry_liquidation_matrix_discovers_crank_lock() {
    let mut params = V16CuMarketParams::default();
    params.initial_price = 1;
    params.max_price_move_bps_per_slot = 10_000;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_auth_mark_with_cu(0, 1);

    let l1o = Keypair::new();
    let l2o = Keypair::new();
    let l3o = Keypair::new();
    let l4o = Keypair::new();
    let l5o = Keypair::new();
    let s1o = Keypair::new();
    let s2o = Keypair::new();
    let s3o = Keypair::new();
    let l1 = env.create_portfolio(&l1o);
    let l2 = env.create_portfolio(&l2o);
    let l3 = env.create_portfolio(&l3o);
    let l4 = env.create_portfolio(&l4o);
    let l5 = env.create_portfolio(&l5o);
    let s1 = env.create_portfolio(&s1o);
    let s2 = env.create_portfolio(&s2o);
    let s3 = env.create_portfolio(&s3o);

    for (owner, portfolio, deposit) in [
        (&l1o, l1, 1_000),
        (&l2o, l2, 1_000),
        (&l3o, l3, 1_000),
        (&l4o, l4, 1_000),
        (&l5o, l5, 1_000),
        (&s1o, s1, 2),
        (&s2o, s2, 5),
        (&s3o, s3, 1_000),
    ] {
        env.deposit(owner, portfolio, deposit);
    }
    for (long_owner, long, short_owner, short, quantity) in [
        (&l1o, l1, &s1o, s1, 1_897_305),
        (&l2o, l2, &s1o, s1, 102_695),
        (&l2o, l2, &s2o, s2, 666_301),
        (&l3o, l3, &s2o, s2, 65_831),
        (&l4o, l4, &s2o, s2, 430_043),
        (&l4o, l4, &s3o, s3, 1_130_061),
        (&l5o, l5, &s3o, s3, 767_244),
    ] {
        env.trade_asset_with_cu(0, long_owner, long, short_owner, short, quantity, 1, 0);
    }

    for (slot, mark) in [(1u64, 2u64), (2, 3), (3, 4), (4, 5), (5, 6)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        env.crank(
            s3,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for _ in 0..4 {
        if !has_active_leg_for_asset(&env.portfolio_state(s1), 0) {
            break;
        }
        env.crank(
            s1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
        );
    }
    let first = env.market_state().1;
    assert!(!has_active_leg_for_asset(&env.portfolio_state(s1), 0));
    assert_eq!(first.mode, MarketModeV16::Live);
    assert_eq!(first.assets[0].social_loss_remainder_long_num, 322_760);
    assert_ne!(first.assets[0].b_long_num, 0);

    for _ in 0..8 {
        let leg = active_leg_for_asset(&env.portfolio_state(l1), 0);
        if !leg.b_stale && leg.b_snap == env.market_state().1.assets[0].b_long_num {
            break;
        }
        env.crank(
            l1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(l1), 0).b_rem,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );
    env.rebalance_reduce_with_cu(&l1o, l1, 0, 1_897_305);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(l1), 0));
    let carry_state = env.market_state().1;
    assert_eq!(carry_state.assets[0].oi_eff_long_q, 1_162_175);
    assert_eq!(carry_state.assets[0].oi_eff_short_q, 1_162_175);
    assert_eq!(
        carry_state.assets[0].social_loss_dust_long_num,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );

    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 7);
    env.crank(
        s2,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(s2), 0));
    assert!(health_cert(&env.portfolio_state(s2)).certified_liq_deficit != 0);

    let fixed_market = env.svm.get_account(&env.market).unwrap();
    let fixed_loser = env.svm.get_account(&s2).unwrap();
    let fixed_vault = env.svm.get_account(&env.vault).unwrap();
    for slot in 6..=8u64 {
        env.svm.warp_to_slot(slot);
        env.svm.expire_blockhash();
        let liquidation = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s2, false),
            ],
            &[],
        );
        assert!(liquidation.is_err(), "reset carry unexpectedly progressed");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&s2).unwrap(), fixed_loser);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
    }
    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
    assert!(has_active_leg_for_asset(&env.portfolio_state(s2), 0));
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 1_162_175);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 1_162_175);
}

#[test]
fn v16_attack_liquidation_reward_share_without_tail_still_progresses() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper = Keypair::new();
    env.ensure_signer_account(keeper.pubkey());
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
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
    }
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0,
        "setup must make the target liquidatable before the no-tail reward-share crank"
    );

    let (_, before) = env.market_state();
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(keeper.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[&keeper],
    );
    assert!(
        accepted.is_ok(),
        "reward-enabled liquidation must remain live when the keeper omits the optional reward tail: {accepted:?}"
    );

    let (_, after) = env.market_state();
    assert!(
        after.insurance > before.insurance,
        "without a reward tail, the liquidation fee is retained by insurance"
    );
    assert_eq!(
        after.vault, before.vault,
        "liquidation reward sharing is an internal fee split, not a vault mint"
    );
    assert_eq!(
        after.vault as u64,
        env.token_amount(env.vault),
        "vault accounting remains tied to SPL custody"
    );
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation"
    );
}
