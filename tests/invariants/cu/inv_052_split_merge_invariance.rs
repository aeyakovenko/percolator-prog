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

fn run_funding_cadence(fragmented: bool, target_price: u64) -> FundingCadenceOutcome {
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
    for portfolio in [long, short] {
        env.svm.expire_blockhash();
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        if target_price > INITIAL_PRICE {
            1_010_000
        } else {
            990_000
        }
    );

    if fragmented {
        for slot in 2..=10u64 {
            env.svm.warp_to_slot(slot);
            for portfolio in [long, short] {
                env.svm.expire_blockhash();
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                );
            }
        }
    } else {
        env.svm.warp_to_slot(10);
        for portfolio in [long, short] {
            env.svm.expire_blockhash();
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 10,
                    observations: crank_observations(0),
                },
            );
        }
    }

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

// Direct counterexample: endpoint funding currently erases the premium integral when one delayed
// public crank reaches the same terminal mark as a sequence of one-slot cranks.
#[test]
fn v16_counterexample_delayed_converging_crank_erases_elapsed_funding() {
    let fragmented = run_funding_cadence(true, 1_100_000);
    let delayed = run_funding_cadence(false, 1_100_000);

    assert_eq!(fragmented.final_mark, delayed.final_mark);
    assert_eq!(fragmented.final_mark, 1_100_000);
    assert_eq!(fragmented.effective_price, delayed.effective_price);
    assert_eq!(fragmented.effective_price, 1_100_000);
    assert_eq!(fragmented.terminal_vault, delayed.terminal_vault);
    assert_eq!(fragmented.terminal_vault, 0);
    assert_eq!(fragmented.terminal_c_tot, delayed.terminal_c_tot);
    assert_eq!(fragmented.terminal_c_tot, 0);
    assert_ne!(fragmented.f_long_num, 0, "control must accrue funding");
    assert_eq!(
        delayed.f_long_num, 0,
        "delayed endpoint rate erases the interval"
    );
    assert!(delayed.long_pnl_before_close > fragmented.long_pnl_before_close);
    assert!(delayed.short_capital_before_close < fragmented.short_capital_before_close);
    assert_eq!(
        (delayed.long_pnl_before_close - fragmented.long_pnl_before_close) as u128,
        fragmented.short_capital_before_close - delayed.short_capital_before_close,
    );
    assert_eq!(
        delayed.long_withdrawn - fragmented.long_withdrawn,
        fragmented.short_withdrawn - delayed.short_withdrawn,
    );
    assert_eq!(delayed.long_withdrawn - fragmented.long_withdrawn, 80_000);
}

#[test]
fn v16_counterexample_downward_convergence_erases_negative_funding() {
    let fragmented = run_funding_cadence(true, 910_000);
    let delayed = run_funding_cadence(false, 910_000);

    assert_eq!(fragmented.final_mark, 910_000);
    assert_eq!(fragmented.effective_price, delayed.effective_price);
    assert_eq!(fragmented.effective_price, 910_000);
    assert!(fragmented.f_long_num > 0);
    assert_eq!(fragmented.f_short_num, -fragmented.f_long_num);
    assert_eq!(delayed.f_long_num, 0);
    assert_eq!(delayed.f_short_num, 0);
    assert!(fragmented.long_withdrawn > delayed.long_withdrawn);
    assert!(delayed.short_withdrawn > fragmented.short_withdrawn);
    assert_eq!(
        fragmented.long_withdrawn - delayed.long_withdrawn,
        delayed.short_withdrawn - fragmented.short_withdrawn,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PriceCapCadenceOutcome {
    effective_price_before_close: u64,
    long_pnl_before_close: i128,
    short_capital_before_close: u128,
    long_withdrawn: u128,
    short_withdrawn: u128,
}

fn run_price_cap_cadence(fragmented: bool, target_price: u64) -> PriceCapCadenceOutcome {
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

    if fragmented {
        for slot in 2..=10u64 {
            env.svm.warp_to_slot(slot);
            env.svm.expire_blockhash();
            env.crank(
                long,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
    } else {
        env.svm.warp_to_slot(10);
        env.svm.expire_blockhash();
        env.crank(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: 10,
                observations: crank_observations(0),
            },
        );
    }

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

// Direct counterexample: the public caller can partition identical elapsed time into one-slot
// cranks, compounding a current-price cap and changing independent users' eventual SPL payouts.
#[test]
fn v16_counterexample_price_cap_partition_changes_effective_price_and_spl_payouts() {
    let fragmented = run_price_cap_cadence(true, 2_000_000);
    let delayed = run_price_cap_cadence(false, 2_000_000);

    assert_eq!(fragmented.effective_price_before_close, 1_104_620);
    assert_eq!(delayed.effective_price_before_close, 1_100_900);
    assert!(fragmented.long_pnl_before_close > delayed.long_pnl_before_close);
    assert_eq!(
        fragmented.short_capital_before_close,
        delayed.short_capital_before_close,
    );
    assert_eq!(
        fragmented.long_withdrawn - delayed.long_withdrawn,
        3_720_000,
    );
    assert_eq!(
        delayed.short_withdrawn - fragmented.short_withdrawn,
        3_720_000,
    );
}

#[test]
fn v16_counterexample_downward_price_cap_partition_reverses_the_winner() {
    let fragmented = run_price_cap_cadence(true, 500_000);
    let delayed = run_price_cap_cadence(false, 500_000);

    assert_eq!(fragmented.effective_price_before_close, 904_387);
    assert_eq!(delayed.effective_price_before_close, 900_900);
    assert!(delayed.long_pnl_before_close > fragmented.long_pnl_before_close);
    assert_eq!(
        fragmented.short_capital_before_close,
        delayed.short_capital_before_close,
    );
    assert_eq!(
        delayed.long_withdrawn - fragmented.long_withdrawn,
        3_487_000
    );
    assert_eq!(
        fragmented.short_withdrawn - delayed.short_withdrawn,
        3_487_000
    );
}

fn run_hybrid_price_cap_endpoint(fragmented: bool) -> u64 {
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

    if fragmented {
        for slot in 2..=10u64 {
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
        }
    } else {
        set_test_clock(&mut env, 10, 110);
        env.svm.expire_blockhash();
        env.crank_with_oracle_tail(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: 10,
                observations: crank_observations(0),
            },
            &[target],
        );
    }
    env.market_state().1.assets[0].effective_price
}

#[test]
fn v16_counterexample_hybrid_pyth_price_cap_partition_changes_endpoint() {
    assert_eq!(run_hybrid_price_cap_endpoint(true), 1_104_620);
    assert_eq!(run_hybrid_price_cap_endpoint(false), 1_100_900);
}
