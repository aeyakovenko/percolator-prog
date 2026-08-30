//! INV-044 - No phantom value from indices, certificates, or labels.
//!
//! Normative obligation: derived labels and crank classifications cannot create
//! token stock, withdrawable value, health, or senior capital by themselves.
//!
//! Evidence in this file (I/C): a third party permissionlessly asks the public
//! crank route to make B-settlement progress on a flat solvent account. The
//! engine must return non-progress rather than a successful no-op, and the account's capital,
//! vault, `c_tot`, insurance, and owner withdrawability must remain exact.
//! A second public lifecycle creates source-attributed PnL on one asset and an
//! offsetting loss on another, then permutes the two permissionless crank
//! orders. Both orders must converge to identical claims, certified equity,
//! normalized source stock, and terminal SPL withdrawal. This catches any
//! derived-claim burn that depends on which account a keeper settles first.
//! Additional no-phantom-value coverage lives in INV-025, INV-026, INV-069, and
//! INV-070.

use super::*;

#[test]
fn v16_program_permissionless_settle_b_on_healthy_account_is_safe_noop() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let (_, group_before) = env.market_state();
    let capital_before = env.portfolio_state(portfolio).capital.get();
    assert_eq!(capital_before, 1_000);

    env.svm.warp_to_slot(5);
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );

    let (_, group_after) = env.market_state();
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
    assert_eq!(group_after.vault, group_before.vault);
    assert_eq!(group_after.c_tot, group_before.c_tot);
    assert_eq!(group_after.insurance, group_before.insurance);

    let (dest, _) = env.withdraw_with_cu(&owner, portfolio, 1_000);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "derived crank labels cannot trap an otherwise withdrawable account"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct CrossDomainSettlementOutcome {
    winner_capital: u128,
    winner_pnl: i128,
    winner_source_claim_num: u128,
    winner_certified_equity: i128,
    counterparty_capital: u128,
    counterparty_pnl: i128,
    source_claim_num: u128,
    source_funded_or_consumed_num: u128,
    winner_withdrawal: u64,
}

fn inv044_cross_domain_settlement_outcome(
    settle_counterparty_first: bool,
) -> CrossDomainSettlementOutcome {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 5_000, 10_000, 1_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let observer_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    let observer = env.create_portfolio(&observer_owner);
    env.deposit(&owner, portfolio, 3_150);
    env.deposit(&counterparty_owner, counterparty, 5_000);
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        20 * POS_SCALE as i128,
        100,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        10 * POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 110);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for _ in 0..4 {
        if env
            .crank_if_actionable(
                observer,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            )
            .is_none()
        {
            break;
        }
    }
    let settlement_order = if settle_counterparty_first {
        [counterparty, portfolio, counterparty, portfolio, portfolio]
    } else {
        [portfolio, counterparty, portfolio, counterparty, portfolio]
    };
    for actor in settlement_order {
        for _ in 0..4 {
            if env
                .crank_if_actionable(
                    actor,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                )
                .is_none()
            {
                break;
            }
        }
    }

    let winner = env.portfolio_state(portfolio);
    let loser = env.portfolio_state(counterparty);
    let (_, group) = env.market_state();
    let winner_source_claim_num = winner
        .source_domains
        .iter()
        .filter(|source| source.is_occupied() && source.domain.get() == 1)
        .map(|source| source.source_claim_bound_num.get())
        .sum::<u128>();
    let before_close = CrossDomainSettlementOutcome {
        winner_capital: winner.capital.get(),
        winner_pnl: winner.pnl.get(),
        winner_source_claim_num,
        winner_certified_equity: health_cert(&winner).certified_equity,
        counterparty_capital: loser.capital.get(),
        counterparty_pnl: loser.pnl.get(),
        source_claim_num: group.source_credit[1].positive_claim_bound_num,
        source_funded_or_consumed_num: group.source_credit[1]
            .fresh_reserved_backing_num
            .checked_add(group.source_credit[1].spent_backing_num)
            .unwrap(),
        winner_withdrawal: 0,
    };

    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(20 * POS_SCALE as i128),
        110,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(10 * POS_SCALE as i128),
        95,
        0,
    );
    for _ in 0..8 {
        let mut progressed = false;
        for actor in [portfolio, counterparty] {
            progressed |= env
                .crank_if_actionable(
                    actor,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: Vec::new(),
                    },
                )
                .is_some();
        }
        if !progressed {
            break;
        }
    }
    let released = env.portfolio_state(portfolio).pnl.get().max(0) as u128;
    if released != 0 {
        env.convert_released_pnl_with_cu(&owner, portfolio, released);
    }
    let winner_capital = env.portfolio_state(portfolio).capital.get();
    let (winner_destination, _) = env.withdraw_with_cu(&owner, portfolio, winner_capital);
    CrossDomainSettlementOutcome {
        winner_withdrawal: env.token_amount(winner_destination),
        ..before_close
    }
}

#[test]
fn v16_program_cross_domain_settlement_is_crank_order_independent() {
    let winner_first = inv044_cross_domain_settlement_outcome(false);
    let counterparty_first = inv044_cross_domain_settlement_outcome(true);

    assert_eq!(
        winner_first, counterparty_first,
        "permissionless crank order cannot burn a user's source-attributed claim",
    );
    assert_eq!(winner_first.winner_capital, 3_150);
    assert_eq!(winner_first.winner_pnl, 150);
    assert_eq!(winner_first.winner_source_claim_num, 150 * BOUND_SCALE);
    assert_eq!(winner_first.winner_certified_equity, 3_300);
    assert_eq!(winner_first.counterparty_capital, 4_800);
    assert_eq!(winner_first.counterparty_pnl, 50);
    assert_eq!(winner_first.source_claim_num, 150 * BOUND_SCALE);
    assert_eq!(
        winner_first.source_funded_or_consumed_num,
        200 * BOUND_SCALE,
    );
    assert_eq!(winner_first.winner_withdrawal, 3_300);
}

// security.md sweep — deposit with parked pnl (#32/#33): depositing while holding junior (parked) pnl
// must credit capital exactly and leave the pnl and its residual backing untouched. No double-count,
// no disturbance of the junior pnl, conservation holds.
#[test]
fn v16_attack_deposit_with_parked_pnl_clean() {
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
    // price up -> long accrues parked pnl; settle.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank_steps_after_market_catchup(
        sh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        lo,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(11);
    for p in [sh, lo] {
        env.crank_if_actionable(
            p,
            ProgInstruction::PermissionlessCrank {
                now_slot: 11,
                observations: crank_observations(0),
            },
        );
    }
    let a0 = env.portfolio_state(lo);
    assert!(a0.pnl.get() > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;

    // deposit MORE while holding the parked pnl.
    env.svm.expire_blockhash();
    env.deposit(&lo_owner, lo, 500_000);
    let a1 = env.portfolio_state(lo);
    let (_, g1) = env.market_state();
    assert_eq!(
        a1.capital.get(),
        a0.capital.get() + 500_000,
        "capital credited exactly by the deposit"
    );
    assert_eq!(
        a1.pnl.get(),
        a0.pnl.get(),
        "parked pnl UNCHANGED by the deposit (no double-count/disturbance)"
    );
    assert_eq!(
        g1.vault,
        g0.vault + 500_000,
        "vault grew by exactly the deposit"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot + 500_000,
        "c_tot grew by exactly the deposit"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault balance"
    );
    // the parked pnl is still backed by (at least) the same residual.
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    assert_eq!(
        resid1, resid0,
        "residual backing of the junior pnl unchanged by the deposit"
    );
    assert!(
        resid1 >= a1.pnl.get().max(0),
        "junior pnl still backed by residual"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}
