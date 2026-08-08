//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: Liquidation is deterministic, risk reducing, OI coherent, and bounded at maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): ADL-transfer and reset-carry
//! liquidation matrices plus public liquidation health checks, bounded partial closes, fee caps,
//! no-repeat charging after restored health, reward split bounds, and no vault minting. These tests exercise the deployed public
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
    env.crank_steps_after_market_catchup(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        claimant,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
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
                env.convert_released_pnl_ix(portfolio, u128::MAX),
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
    let neutral_owner = Keypair::new();
    let l1 = env.create_portfolio(&l1o);
    let l2 = env.create_portfolio(&l2o);
    let l3 = env.create_portfolio(&l3o);
    let l4 = env.create_portfolio(&l4o);
    let l5 = env.create_portfolio(&l5o);
    let s1 = env.create_portfolio(&s1o);
    let s2 = env.create_portfolio(&s2o);
    let s3 = env.create_portfolio(&s3o);
    let neutral = env.create_portfolio(&neutral_owner);

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

    for slot in 6..=8u64 {
        env.svm.warp_to_slot(slot);
        env.crank_steps_after_market_catchup(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            1,
        );
        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_loser = env.svm.get_account(&s2).unwrap();
        let fixed_vault = env.svm.get_account(&env.vault).unwrap();
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

// keeper supplies no quantity, so repeated minimum-fee chunk selection is not representable.
#[test]
fn v16_program_partial_liquidation_bounded_and_conserves() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
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
                AccountMeta::new(short_account, false),
            ],
            &[],
        );
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_short_q;
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    let (_, g_post) = env.market_state();
    let closed = oi_pre.saturating_sub(g_post.assets[0].oi_eff_short_q);
    assert!(closed <= oi_pre, "engine cannot close more than live OI");
    assert!(
        g_post.assets[0].oi_eff_short_q <= oi_pre,
        "OI never increased"
    );
    // conservation: vault unchanged (internal), accounting==real, senior conservation.
    assert_eq!(
        g_post.vault, g_pre.vault,
        "vault unchanged by partial liquidation"
    );
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation"
    );
}

// force-close, no fee extraction, position intact.
#[test]
fn v16_program_healthy_account_not_liquidatable() {
    // maintenance 50% < initial 100% -> a freshly-opened account is well above maintenance.
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // attacker tries to liquidate the healthy la (no adverse price move).
    env.svm.warp_to_slot(1);
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[],
    );

    // healthy account: position intact, no fee extracted, conservation.
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "healthy account's position NOT force-closed by liquidation"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        1_000_000,
        "healthy account capital not docked a liquidation fee"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced (position intact)"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// extraction. Attempting to liquidate the losing leg must not unfairly drain the healthy account.
#[test]
fn v16_program_cross_margin_solvent_account_not_unfairly_liquidated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
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
        .expect("mark cfg");
    };
    cfg(
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
    env.deposit(&victim_owner, victim, 1_000_000);
    env.deposit(&cp_owner, cp, 1_000_000);
    // victim LONG asset0 and SHORT asset1 (cross-margined opposite exposures).
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        POS_SCALE as i128,
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
    // both marks up 10%: victim GAINS on asset0 (long), LOSES on asset1 (short) -> net ~flat.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    cfg(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 1,
            now_slot: 10,
            mark_e6: 110,
        },
    );
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
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
    let v_before = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_before = v_before.capital.get() as i128 + v_before.pnl.get();
    let (_, g_before) = env.market_state();
    // attacker tries to liquidate the victim's LOSING leg (asset1).
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 11,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
        ],
        &[],
    );
    let v_after = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_after = v_after.capital.get() as i128 + v_after.pnl.get();
    let (_, g_after) = env.market_state();
    // the solvent victim's total equity is not reduced by the liquidation attempt (no unfair drain).
    assert!(
        equity_after >= equity_before,
        "solvent cross-margined victim equity not drained by liquidation attempt ({} -> {})",
        equity_before,
        equity_after
    );
    assert_eq!(
        g_after.vault, g_before.vault,
        "no tokens moved by liquidation attempt"
    );
    assert!(
        g_after.vault >= g_after.c_tot + g_after.insurance,
        "senior conservation"
    );
}

// nonzero liquidation_fee_bps (via production_risk_params, which satisfies the engine solvency envelope).
#[test]
fn v16_program_liquidation_cranker_reward_bounded_by_fee() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000); // cranker share = 50%
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co); // cranker (could be a self-liquidator)
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000); // ~2x the 5% IM -> insolvent on a modest move
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    // liquidate the insolvent short, crediting the cranker portfolio.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation with a fee should proceed: {:?}", r);
    let (_, g1) = env.market_state();
    let cranker_reward = env.portfolio_state(c).capital.get() as i128 - c0 as i128;
    let ins_delta = g1.insurance as i128 - g0.insurance as i128;
    let total_fee = cranker_reward + ins_delta;

    // non-vacuity: a real liquidation fee was charged and the cranker got a real reward.
    assert!(
        cranker_reward > 0,
        "cranker received a reward (non-vacuous), got {}",
        cranker_reward
    );
    assert!(total_fee > 0, "a liquidation fee was charged");
    // BOUND: the reward never exceeds the fee, and at 50% share it is exactly half (rest to insurance).
    assert!(
        cranker_reward <= total_fee,
        "cranker reward must not exceed the fee (no profit beyond the fee)"
    );
    assert_eq!(
        cranker_reward, ins_delta,
        "50%% share: cranker reward == insurance share (exact split)"
    );
    assert!(
        cranker_reward < total_fee,
        "cranker gets < the full fee -> self-liquidation nets negative"
    );
    // NO MINT: the fee is internal (liquidated account -> cranker + insurance), vault unchanged.
    assert_eq!(g1.vault, g0.vault, "liquidation fee mints no vault tokens");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation through fee'd liquidation"
    );
}

// configured cap. Protection: the total fee (cranker + insurance) never exceeds liquidation_fee_cap.
#[test]
fn v16_program_liquidation_fee_capped() {
    const CAP: u128 = 100;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        liquidation_fee_cap: CAP, // tiny cap; 5bps * ~1e6 notional would be ~535 uncapped
        ..production_risk_params()
    });
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation should proceed: {:?}", r);
    let (_, g1) = env.market_state();
    let cranker_reward = (env.portfolio_state(c).capital.get() as i128 - c0 as i128) as u128;
    let ins_delta = (g1.insurance as i128 - g0.insurance as i128) as u128;
    let total_fee = cranker_reward + ins_delta;

    // CAP: the total liquidation fee is bounded by liquidation_fee_cap — NOT the uncapped bps*notional.
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    assert!(
        total_fee <= CAP,
        "total liquidation fee {} must be capped at liquidation_fee_cap {}",
        total_fee,
        CAP
    );
    // the cap actually bit: 5bps of ~1e6 notional (~535) would have exceeded CAP=100 absent the cap.
    assert_eq!(
        total_fee, CAP,
        "the fee is the cap exactly (uncapped fee would be ~535 >> 100)"
    );
    assert_eq!(g1.vault, g0.vault, "fee is internal, no vault mint");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// the first selected liquidation charges a fee; the second no-longer-liquidatable crank charges zero.
#[test]
fn v16_program_repeated_partial_liquidation_stops_charging_after_health_restored() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0); // all fee to insurance (clean single accumulator)
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 200_000); // enough to open 2*POS_SCALE at 5% IM, then go insolvent
    env.trade_asset_with_cu(0, &lo, l, &so, s, (2 * POS_SCALE) as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    // mark is now fixed (no further pushes). Liquidate in two equal POS_SCALE partials.
    let liq = |env: &mut V16CuEnv| -> u128 {
        let ins0 = env.market_state().1.insurance;
        env.crank(
            s,
            ProgInstruction::PermissionlessCrank {
                now_slot: 30,
                observations: crank_observations(0),
            },
        );
        env.market_state().1.insurance - ins0
    };
    let fee1 = liq(&mut env);
    let fee2 = liq(&mut env);

    // STOP: a real first liquidation charged a fee, but replaying the same close hint after the
    // account is no longer liquidatable cannot charge a second liquidation fee.
    assert!(
        fee1 > 0,
        "first partial charges a fee (non-vacuous), fee1={}",
        fee1
    );
    assert_eq!(
        fee2, 0,
        "second no-longer-liquidatable close hint charges no fee (fee1={} fee2={})",
        fee1, fee2
    );
    let g = env.market_state().1;
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// transfer only). The existing bounded test only exercises a 50% share (exact cranker/insurance split).
#[test]
fn v16_program_liquidation_full_cranker_share_takes_whole_fee_no_mint() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(10_000); // 100% cranker share — the boundary
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co); // cranker
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000); // thin short -> insolvent on a price doubling
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    // Liquidate the insolvent short, crediting the cranker portfolio.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation with a fee should proceed: {r:?}");

    let (_, g1) = env.market_state();
    let cranker_reward = env.portfolio_state(c).capital.get() as i128 - c0 as i128;
    let ins_delta = g1.insurance as i128 - g0.insurance as i128;
    let total_fee = cranker_reward + ins_delta;

    assert!(
        cranker_reward > 0,
        "cranker received a real reward (non-vacuous): {cranker_reward}"
    );
    assert!(total_fee > 0, "a liquidation fee was charged");
    // 100% share: the cranker takes the ENTIRE fee; insurance gets nothing.
    assert_eq!(
        ins_delta, 0,
        "100%% share: no liquidation fee retained to insurance"
    );
    assert_eq!(
        cranker_reward, total_fee,
        "100%% share: cranker reward == the whole fee"
    );
    // The reward never exceeds the fee (no profit/mint beyond what was charged).
    assert!(
        cranker_reward <= total_fee,
        "cranker reward bounded by the fee"
    );
    // The fee is internal (liquidated account -> cranker), minting no vault tokens.
    assert_eq!(g1.vault, g0.vault, "liquidation fee mints no vault tokens");
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation through 100%-share liquidation"
    );
}

#[test]
fn v16_bpf_permissionless_liquidation_is_bounded() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    let liquidation_cu = env.crank_steps(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        2,
    );
    println!("v16 liquidation crank CU: {liquidation_cu}");
    assert!(
        liquidation_cu <= CRANK_CU_LIMIT,
        "liquidation CU {} exceeded limit {}",
        liquidation_cu,
        CRANK_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(group.slot_last, 1);
    assert_eq!(group.assets[0].effective_price, 200);
    let remaining_q = if has_active_leg_for_asset(&short, 0) {
        active_leg_for_asset(&short, 0).basis_pos_q.unsigned_abs()
    } else {
        0
    };
    assert!(remaining_q < POS_SCALE, "liquidation strictly reduces risk");
    assert_eq!(
        health_cert(&short).certified_liq_deficit,
        0,
        "engine-selected partial restores maintenance health"
    );
}

// Engine-selected liquidation of a bankrupt account can never over-close into phantom OI or create
// value. The public instruction carries no liquidation quantity.
#[test]
fn v16_engine_selected_liquidation_cannot_overclose_or_create_value() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250); // tiny -> insolvent on up-move
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
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
                AccountMeta::new(short_account, false),
            ],
            &[],
        );
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_long_q;
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    let (_, g) = env.market_state();
    assert!(g.assets[0].oi_eff_short_q <= oi_pre, "short OI never grows");
    assert!(g.assets[0].oi_eff_long_q <= oi_pre, "long OI never grows");
    assert_eq!(g.vault, 1_000_250, "liquidation creates no vault value");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    // the short is fully closed (position gone), not over-closed into a phantom opposite position.
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&sh)),
        "short position fully closed, no phantom flip"
    );
}

// A deeply insolvent single-leg account cannot be partially liquidated while leaving uncovered open
// risk. With no keeper size input, the engine selects a full close, books the residual, and preserves
// custody and balanced OI.
#[test]
fn v16_engine_selected_deep_insolvency_close_is_full_and_conserving() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let a = env.create_portfolio(&la); // long winner
    let lb = Keypair::new();
    let b = env.create_portfolio(&lb); // short, will be DEEPLY bankrupt
    env.deposit(&la, a, 1_000);
    env.deposit(&lb, b, 1_000);
    env.trade_asset_with_cu(0, &la, a, &lb, b, (7 * POS_SCALE) as i128, 100, 0);
    // 5x move: short loss ≈ 2800 >> its 1000 capital -> deeply bankrupt (negative equity).
    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 500);
    for p in [b, a] {
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
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
    let g_pre = env.market_state().1;
    env.crank_steps_after_market_catchup(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );

    let loser = env.portfolio_state(b);
    let g_post = env.market_state().1;
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&loser)),
        "deeply insolvent single-leg account is fully closed"
    );
    assert_eq!(g_post.assets[0].oi_eff_long_q, 0);
    assert_eq!(g_post.assets[0].oi_eff_short_q, 0);
    assert_eq!(g_post.vault, g_pre.vault, "full close mints no vault value");
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation preserved through residual booking"
    );
}

// The engine-selected liquidation fee is derived from actual closed risk, not keeper input, and remains
// bounded while value conservation holds.
#[test]
fn v16_engine_selected_liquidation_fee_is_bounded_by_closed_position() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0); // all to insurance
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0); // POS_SCALE position only
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let (_, g0) = env.market_state();

    env.crank(
        s,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
    );
    let (_, g1) = env.market_state();
    let fee = g1.insurance - g0.insurance;

    assert!(fee > 0, "a fee was charged (non-vacuous)");
    assert!(
        fee < 10_000,
        "fee is bounded by the actual closed position: {fee}"
    );
    let sl = env.portfolio_state(s);
    assert!(
        sl.legs[0].basis_pos_q.get().unsigned_abs() <= POS_SCALE,
        "position closed at most to its actual size (no phantom over-close)"
    );
    assert_eq!(g1.vault, g0.vault, "fee internal, no vault mint");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}
