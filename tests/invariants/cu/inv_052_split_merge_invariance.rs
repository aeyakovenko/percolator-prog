//! INV-052 - split/merge invariance.
//!
//! Partitioning an authorized operation must not produce a more favorable final
//! economic state than the aggregate operation. These public LiteSVM checks compare
//! normalized market and portfolio economics for one aggregate route versus split
//! execution of the same total trade or withdrawal.

use super::*;

#[derive(Debug, PartialEq, Eq)]
struct TradeEconomicSnapshot {
    vault: u128,
    c_tot: u128,
    insurance: u128,
    oi_eff_long_q: u128,
    oi_eff_short_q: u128,
    account_a_capital: u128,
    account_b_capital: u128,
    account_a_pnl: i128,
    account_b_pnl: i128,
    account_a_basis_q: i128,
    account_b_basis_q: i128,
}

fn active_basis_for_asset(account: &PortfolioAccountV16, asset_index: usize) -> i128 {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .unwrap_or(0)
}

fn split_trade_snapshot(parts_q: &[i128]) -> TradeEconomicSnapshot {
    let mut env = V16CuEnv::new();
    let owner_a = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let owner_b = Keypair::new();
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 1_000_000);
    env.deposit(&owner_b, account_b, 1_000_000);

    for &part_q in parts_q {
        env.trade_asset_with_cu(0, &owner_a, account_a, &owner_b, account_b, part_q, 100, 0);
    }

    let (_, group) = env.market_state();
    let a = env.portfolio_state(account_a);
    let b = env.portfolio_state(account_b);
    TradeEconomicSnapshot {
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
        oi_eff_long_q: group.assets[0].oi_eff_long_q,
        oi_eff_short_q: group.assets[0].oi_eff_short_q,
        account_a_capital: a.capital.get(),
        account_b_capital: b.capital.get(),
        account_a_pnl: a.pnl.get(),
        account_b_pnl: b.pnl.get(),
        account_a_basis_q: active_basis_for_asset(&a, 0),
        account_b_basis_q: active_basis_for_asset(&b, 0),
    }
}

#[test]
fn v16_program_split_trade_matches_aggregate_trade_economics() {
    let aggregate = split_trade_snapshot(&[3_000 * POS_SCALE as i128]);
    let split = split_trade_snapshot(&[1_000 * POS_SCALE as i128, 2_000 * POS_SCALE as i128]);
    assert_eq!(
        split, aggregate,
        "splitting a same-price no-fee trade must not alter final economics",
    );
}

#[derive(Debug)]
struct FeeTradeSnapshot {
    insurance: u128,
    account_capital_sum: u128,
    oi_eff_long_q: u128,
    oi_eff_short_q: u128,
    account_a_basis_q: i128,
    account_b_basis_q: i128,
}

fn split_fee_trade_snapshot(parts_q: &[i128], fee_bps: u64) -> FeeTradeSnapshot {
    let mut env = V16CuEnv::new();
    let owner_a = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let owner_b = Keypair::new();
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 1_000_000);
    env.deposit(&owner_b, account_b, 1_000_000);

    for &part_q in parts_q {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            0, &owner_a, account_a, &owner_b, account_b, part_q, 101, fee_bps,
        );
    }

    let (_, group) = env.market_state();
    let a = env.portfolio_state(account_a);
    let b = env.portfolio_state(account_b);
    FeeTradeSnapshot {
        insurance: group.insurance,
        account_capital_sum: a.capital.get() + b.capital.get(),
        oi_eff_long_q: group.assets[0].oi_eff_long_q,
        oi_eff_short_q: group.assets[0].oi_eff_short_q,
        account_a_basis_q: active_basis_for_asset(&a, 0),
        account_b_basis_q: active_basis_for_asset(&b, 0),
    }
}

#[test]
fn v16_program_split_fee_trade_cannot_reduce_collected_fees() {
    let aggregate = split_fee_trade_snapshot(&[3 * POS_SCALE as i128], 333);
    let split = split_fee_trade_snapshot(
        &[POS_SCALE as i128, POS_SCALE as i128, POS_SCALE as i128],
        333,
    );

    assert_eq!(split.oi_eff_long_q, aggregate.oi_eff_long_q);
    assert_eq!(split.oi_eff_short_q, aggregate.oi_eff_short_q);
    assert_eq!(split.account_a_basis_q, aggregate.account_a_basis_q);
    assert_eq!(split.account_b_basis_q, aggregate.account_b_basis_q);
    assert!(
        split.insurance >= aggregate.insurance,
        "fee-bearing split trade must not collect less protocol fee than the aggregate route: split={} aggregate={}",
        split.insurance,
        aggregate.insurance,
    );
    assert!(
        split.account_capital_sum <= aggregate.account_capital_sum,
        "splitting a fee-bearing trade must not leave traders with more capital: split={} aggregate={}",
        split.account_capital_sum,
        aggregate.account_capital_sum,
    );
}

#[test]
fn v16_program_split_withdraw_matches_aggregate_withdraw_economics() {
    fn run(parts: &[u128]) -> (u128, u128, u128, u128) {
        let mut env = V16CuEnv::new();
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        env.deposit(&owner, portfolio, 100_000);
        let mut withdrawn = 0u128;
        for &amount in parts {
            let dest = env.withdraw(&owner, portfolio, amount);
            withdrawn = withdrawn
                .checked_add(env.token_amount(dest) as u128)
                .expect("test withdrawn sum fits");
        }
        let (_, group) = env.market_state();
        (
            group.vault,
            group.c_tot,
            env.portfolio_state(portfolio).capital.get(),
            withdrawn,
        )
    }

    assert_eq!(
        run(&[30_000]),
        run(&[10_000, 20_000]),
        "splitting withdrawals must not change custody or residual capital",
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FundingCadenceOutcome {
    long_capital_before_close: u128,
    long_pnl_before_close: i128,
    short_capital_before_close: u128,
    short_pnl_before_close: i128,
    f_long_num: i128,
    f_short_num: i128,
    final_mark: u64,
    effective_price: u64,
    long_withdrawn: u128,
    short_withdrawn: u128,
    terminal_vault: u128,
    terminal_c_tot: u128,
}

fn run_funding_cadence(crank_slots: &[u64], target_price: u64) -> FundingCadenceOutcome {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 2_000_000_000;
    const SIZE_Q: i128 = (1_000u128 * POS_SCALE) as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 10,
        max_abs_funding_e9_per_slot: 10_000,
        min_funding_lifetime_slots: 10,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_auth_mark_with_cu(0, INITIAL_PRICE);

    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    // Both schedules first commit the same authenticated target and slot-1 boundary.
    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, target_price);
    let mut boundary_progress = false;
    for portfolio in [long, short] {
        env.svm.expire_blockhash();
        boundary_progress |= env
            .crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: crank_observations(0),
                },
            )
            .is_some();
    }
    assert!(
        boundary_progress,
        "slot-1 boundary must make public progress"
    );
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        if target_price > INITIAL_PRICE {
            1_010_000
        } else {
            990_000
        }
    );

    let mut previous_slot = 1;
    for &slot in crank_slots {
        assert!(slot > previous_slot && slot <= 10);
        env.svm.warp_to_slot(slot);
        let mut slot_progress = false;
        for portfolio in [long, short] {
            env.svm.expire_blockhash();
            slot_progress |= env
                .crank_if_actionable(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                )
                .is_some();
        }
        assert!(slot_progress, "slot {slot} must make public progress");
        previous_slot = slot;
    }
    assert_eq!(previous_slot, 10, "schedule must reach the common endpoint");

    let long_state_before_close = env.portfolio_state(long);
    let short_state_before_close = env.portfolio_state(short);
    let group_before_close = env.market_state().1;
    let f_long_num = group_before_close.assets[0].f_long_num;
    let f_short_num = group_before_close.assets[0].f_short_num;
    let effective_price_before_close = group_before_close.assets[0].effective_price;

    // Realize the cadence-dependent funding through an ordinary signed close and SPL withdrawals.
    env.svm.expire_blockhash();
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        -SIZE_Q,
        target_price,
        0,
    );
    let long_flat = env.portfolio_state(long);
    let short_flat = env.portfolio_state(short);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &long_flat
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &short_flat
    )));
    if long_flat.pnl.get() > 0 {
        assert_eq!(short_flat.pnl.get(), 0);
        env.convert_released_pnl_with_cu(
            &long_owner,
            long,
            u128::try_from(long_flat.pnl.get()).unwrap(),
        );
    } else {
        assert_eq!(long_flat.pnl.get(), 0);
        assert!(short_flat.pnl.get() > 0);
        env.convert_released_pnl_with_cu(
            &short_owner,
            short,
            u128::try_from(short_flat.pnl.get()).unwrap(),
        );
    }
    let long_after_convert = env.portfolio_state(long);
    let short_after_convert = env.portfolio_state(short);
    assert_eq!(long_after_convert.pnl.get(), 0);
    assert_eq!(short_after_convert.pnl.get(), 0);

    let long_dest = env.withdraw(&long_owner, long, long_after_convert.capital.get());
    let short_dest = env.withdraw(&short_owner, short, short_after_convert.capital.get());
    let long_withdrawn = env.token_amount(long_dest) as u128;
    let short_withdrawn = env.token_amount(short_dest) as u128;
    let terminal_group = env.market_state().1;
    let terminal_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(long_withdrawn + short_withdrawn, 2 * DEPOSIT);
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(terminal_group.vault, 0);
    assert_eq!(terminal_group.c_tot, 0);
    assert_eq!(
        terminal_group.assets[0].effective_price,
        effective_price_before_close
    );

    FundingCadenceOutcome {
        long_capital_before_close: long_state_before_close.capital.get(),
        long_pnl_before_close: long_state_before_close.pnl.get(),
        short_capital_before_close: short_state_before_close.capital.get(),
        short_pnl_before_close: short_state_before_close.pnl.get(),
        f_long_num,
        f_short_num,
        final_mark: terminal_profile.mark_ewma_e6,
        effective_price: terminal_group.assets[0].effective_price,
        long_withdrawn,
        short_withdrawn,
        terminal_vault: terminal_group.vault,
        terminal_c_tot: terminal_group.c_tot,
    }
}

#[test]
fn v16_program_upward_funding_is_crank_partition_invariant() {
    let fragmented = run_funding_cadence(&[2, 3, 4, 5, 6, 7, 8, 9, 10], 1_100_000);
    let irregular = run_funding_cadence(&[3, 6, 7, 10], 1_100_000);
    let delayed = run_funding_cadence(&[10], 1_100_000);

    assert_eq!(fragmented, irregular);
    assert_eq!(fragmented, delayed);
    assert_eq!(fragmented.final_mark, delayed.final_mark);
    assert_eq!(fragmented.final_mark, 1_100_000);
    assert_eq!(fragmented.effective_price, delayed.effective_price);
    assert_eq!(fragmented.effective_price, 1_100_000);
    assert_eq!(fragmented.terminal_vault, delayed.terminal_vault);
    assert_eq!(fragmented.terminal_vault, 0);
    assert_eq!(fragmented.terminal_c_tot, delayed.terminal_c_tot);
    assert_eq!(fragmented.terminal_c_tot, 0);
    assert_ne!(fragmented.f_long_num, 0, "control must accrue funding");
    assert_eq!(fragmented.f_short_num, -fragmented.f_long_num);
}

#[test]
fn v16_program_downward_funding_is_crank_partition_invariant() {
    let fragmented = run_funding_cadence(&[2, 3, 4, 5, 6, 7, 8, 9, 10], 910_000);
    let irregular = run_funding_cadence(&[4, 5, 8, 10], 910_000);
    let delayed = run_funding_cadence(&[10], 910_000);

    assert_eq!(fragmented, irregular);
    assert_eq!(fragmented, delayed);
    assert_eq!(fragmented.final_mark, 910_000);
    assert_eq!(fragmented.effective_price, delayed.effective_price);
    assert_eq!(fragmented.effective_price, 910_000);
    assert!(fragmented.f_long_num > 0);
    assert_eq!(fragmented.f_short_num, -fragmented.f_long_num);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PriceCapCadenceOutcome {
    effective_price_before_close: u64,
    long_pnl_before_close: i128,
    short_capital_before_close: u128,
    long_withdrawn: u128,
    short_withdrawn: u128,
}

fn run_price_cap_cadence(crank_slots: &[u64], target_price: u64) -> PriceCapCadenceOutcome {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 2_000_000_000;
    let size_q = if target_price > INITIAL_PRICE {
        (1_000u128 * POS_SCALE) as i128
    } else {
        -((1_000u128 * POS_SCALE) as i128)
    };

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 10,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 10,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_auth_mark_with_cu(0, INITIAL_PRICE);

    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_ctx, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &short_owner, short);
    env.trade_cpi_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        size_q,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, target_price);
    env.svm.expire_blockhash();
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        if target_price > INITIAL_PRICE {
            1_010_000
        } else {
            990_000
        }
    );

    let mut previous_slot = 1;
    for &slot in crank_slots {
        assert!(slot > previous_slot && slot <= 10);
        env.svm.warp_to_slot(slot);
        env.svm.expire_blockhash();
        env.crank(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
        previous_slot = slot;
    }
    assert_eq!(previous_slot, 10, "schedule must reach the common endpoint");

    let long_before_close = env.portfolio_state(long);
    let short_before_close = env.portfolio_state(short);
    let effective_price_before_close = env.market_state().1.assets[0].effective_price;

    env.svm.expire_blockhash();
    env.trade_cpi_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        -size_q,
        0,
    );
    let long_flat = env.portfolio_state(long);
    let short_flat = env.portfolio_state(short);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &long_flat
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &short_flat
    )));
    assert!(long_flat.pnl.get() > 0);
    assert_eq!(short_flat.pnl.get(), 0);

    env.convert_released_pnl_with_cu(
        &long_owner,
        long,
        u128::try_from(long_flat.pnl.get()).unwrap(),
    );
    let long_after_convert = env.portfolio_state(long);
    let long_dest = env.withdraw(&long_owner, long, long_after_convert.capital.get());
    let short_dest = env.withdraw(&short_owner, short, short_flat.capital.get());
    let long_withdrawn = env.token_amount(long_dest) as u128;
    let short_withdrawn = env.token_amount(short_dest) as u128;
    assert_eq!(long_withdrawn + short_withdrawn, 2 * DEPOSIT);
    assert_eq!(env.token_amount(env.vault), 0);

    PriceCapCadenceOutcome {
        effective_price_before_close,
        long_pnl_before_close: long_before_close.pnl.get(),
        short_capital_before_close: short_before_close.capital.get(),
        long_withdrawn,
        short_withdrawn,
    }
}

#[test]
fn v16_program_upward_price_cap_and_spl_payouts_are_crank_partition_invariant() {
    let fragmented = run_price_cap_cadence(&[2, 3, 4, 5, 6, 7, 8, 9, 10], 2_000_000);
    let irregular = run_price_cap_cadence(&[2, 5, 9, 10], 2_000_000);
    let delayed = run_price_cap_cadence(&[10], 2_000_000);

    assert_eq!(fragmented, irregular);
    assert_eq!(fragmented, delayed);
    assert_eq!(fragmented.effective_price_before_close, 1_100_000);
    assert!(fragmented.long_pnl_before_close > 0);
    assert_eq!(
        fragmented.long_withdrawn + fragmented.short_withdrawn,
        4_000_000_000
    );
}

#[test]
fn v16_program_downward_price_cap_and_spl_payouts_are_crank_partition_invariant() {
    let fragmented = run_price_cap_cadence(&[2, 3, 4, 5, 6, 7, 8, 9, 10], 500_000);
    let irregular = run_price_cap_cadence(&[4, 6, 10], 500_000);
    let delayed = run_price_cap_cadence(&[10], 500_000);

    assert_eq!(fragmented, irregular);
    assert_eq!(fragmented, delayed);
    assert_eq!(fragmented.effective_price_before_close, 900_000);
    assert!(fragmented.long_pnl_before_close > 0);
    assert_eq!(
        fragmented.long_withdrawn + fragmented.short_withdrawn,
        4_000_000_000
    );
}

fn run_hybrid_price_cap_endpoint(crank_slots: &[u64]) -> u64 {
    const INITIAL_PRICE: u64 = 1_000_000;
    const TARGET_PRICE: u64 = 2_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 10,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 10,
        ..V16CuMarketParams::default()
    });
    set_test_clock(&mut env, 0, 100);
    let feed = [0xd1u8; 32];
    let initial = env.set_pyth_price_with_conf(&feed, INITIAL_PRICE as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial],
        0,
        100,
        0,
        0,
        100,
        0,
    )
    .expect("configure fresh Pyth hybrid mode");

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 2_000_000_000);
    env.deposit(&short_owner, short, 2_000_000_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        (1_000u128 * POS_SCALE) as i128,
        INITIAL_PRICE,
        0,
    );

    set_test_clock(&mut env, 1, 101);
    let target = env.set_pyth_price_with_conf(&feed, TARGET_PRICE as i64, -6, 0, 101);
    env.svm.expire_blockhash();
    env.crank_with_oracle_tail(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        &[target],
    );
    assert_eq!(env.market_state().1.assets[0].effective_price, 1_010_000);

    let mut previous_slot = 1;
    for &slot in crank_slots {
        assert!(slot > previous_slot && slot <= 10);
        set_test_clock(&mut env, slot, 100 + slot as i64);
        env.svm.expire_blockhash();
        env.crank_with_oracle_tail(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            &[target],
        );
        previous_slot = slot;
    }
    assert_eq!(previous_slot, 10, "schedule must reach the common endpoint");
    env.market_state().1.assets[0].effective_price
}

#[test]
fn v16_program_hybrid_pyth_price_cap_is_crank_partition_invariant() {
    let fragmented = run_hybrid_price_cap_endpoint(&[2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let irregular = run_hybrid_price_cap_endpoint(&[4, 8, 10]);
    let delayed = run_hybrid_price_cap_endpoint(&[10]);
    assert_eq!(fragmented, 1_100_000);
    assert_eq!(irregular, fragmented);
    assert_eq!(delayed, fragmented);
}

#[test]
fn v16_program_maximum_canonical_accrual_prefix_is_bounded_and_progresses() {
    const INITIAL_PRICE: u64 = 1_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 64,
        max_abs_funding_e9_per_slot: 10_000,
        min_funding_lifetime_slots: 64,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_auth_mark_with_cu(0, INITIAL_PRICE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 2_000_000_000);
    env.deposit(&short_owner, short, 2_000_000_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        (1_000u128 * POS_SCALE) as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 2_000_000);
    env.svm.warp_to_slot(64);

    let first_cu = env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 64,
            observations: crank_observations(0),
        },
    );
    let after_first = env.market_state().1.assets[0];
    assert_eq!(
        after_first.slot_last,
        percolator::V16_MAX_ACCRUAL_PATH_STEPS as u64
    );
    assert!(after_first.effective_price > INITIAL_PRICE);

    let second_cu = env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 64,
            observations: crank_observations(0),
        },
    );
    let after_second = env.market_state().1.assets[0];
    assert_eq!(after_second.slot_last, 64);
    assert!(after_second.effective_price > after_first.effective_price);
    println!("v16 canonical 32-step accrual prefix CU: first={first_cu}, second={second_cu}");
    assert!(
        first_cu < 1_400_000,
        "first max-prefix crank used {first_cu} CU"
    );
    assert!(
        second_cu < 1_400_000,
        "second max-prefix crank used {second_cu} CU"
    );
}

#[test]
fn v16_program_target_change_resets_prior_price_movement_remainder() {
    const PRICE: u64 = 100;
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

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 200);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    let first_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(first_profile.price_move_remainder_bps_num, 2_400);
    assert_eq!(env.market_state().1.assets[0].effective_price, PRICE);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 50);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    let second_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(
        second_profile.price_move_remainder_bps_num,
        2_400,
        "the new downward trajectory starts with zero carry instead of inheriting the old target's 2,400 numerator units"
    );
    assert_eq!(second_profile._padding0, [0u8; 4]);
    assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, 50);
    assert_eq!(env.market_state().1.assets[0].effective_price, PRICE);
}

#[test]
fn v16_program_full_14_leg_maximum_accrual_prefix_stays_bounded() {
    const ASSET_COUNT: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const INITIAL_PRICE: u64 = 1_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSET_COUNT,
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 64,
        max_abs_funding_e9_per_slot: 10_000,
        min_funding_lifetime_slots: 64,
        ..V16CuMarketParams::default()
    });
    for asset_index in 0..ASSET_COUNT {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, INITIAL_PRICE);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000_000);
    let legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: INITIAL_PRICE,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_no_cpi_ix(long, short, legs),
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("public 14-leg batch open");
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        ASSET_COUNT as u32
    );
    let vault_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(0, 1, 2_000_000);
    env.svm.warp_to_slot(64);
    let first_cu = env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 64,
            observations: crank_observations(0),
        },
    );
    let first = env.market_state().1.assets[0];
    assert_eq!(
        first.slot_last,
        percolator::V16_MAX_ACCRUAL_PATH_STEPS as u64
    );

    let second_cu = env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 64,
            observations: crank_observations(0),
        },
    );
    let second = env.market_state().1.assets[0];
    assert_eq!(second.slot_last, 64);
    assert!(second.effective_price > first.effective_price);
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        ASSET_COUNT as u32
    );
    println!(
        "v16 public 14-leg canonical 32-step accrual CU: first={first_cu}, second={second_cu}"
    );
    assert!(
        first_cu < 1_400_000,
        "first full-shape prefix used {first_cu} CU"
    );
    assert!(
        second_cu < 1_400_000,
        "second full-shape prefix used {second_cu} CU"
    );
}
