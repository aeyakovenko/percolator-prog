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
