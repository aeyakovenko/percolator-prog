//! INV-066, INV-067, and INV-070 - resolved payout order, exact-once settlement,
//! and terminal market closure.
//!
//! This bounded public-state model composes the full wrapper route rather than
//! inspecting or injecting engine state. Each world opens two independent
//! matched positions, resolves the market, pays all five funded portfolios in a
//! selected order, retries both payout routes at the terminal fixed point,
//! closes every portfolio, and finally executes `CloseSlab`.
//!
//! The oracle checks exact SPL-vault reconciliation for every payout,
//! claimant-order-independent outcomes, no second payment on retry, zero
//! terminal accounting, successful slab closure, and complete isolation of the
//! separately initialized foreign market. After every trade, resolution, and
//! claimant transition, an independent census recomputes stock and encumbrance
//! totals from all portfolios, decoded state, the raw zero-copy header, and SPL
//! custody.
//! A second bounded model creates a genuine underfunded receipt and two independent backing-release
//! frontiers. It exhausts all 16 choices of landing a claim before and/or after each release. Each
//! claim must be an exact value-moving receipt/SPL delta, strict terminal-cleanup progress, or an
//! exact no-op, and every schedule must converge to the same normalized engine state and all five
//! external destination balances.

use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census,
    verify_resolved_receipt_release_claim_order_matrix,
};
use crate::support::v16_svm::{
    assert_closed_market_tombstone, MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT,
};
use percolator::{MarketModeV16, POS_SCALE};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalOutcome {
    payouts: [u64; PRIMARY_ACTOR_COUNT],
    max_compute_units: u64,
}

fn run_terminal_lifecycle(order: [usize; PRIMARY_ACTOR_COUNT]) -> Result<TerminalOutcome, String> {
    let config = MarketConfig::default();
    let mut env = V16Svm::new([0x66; 32], config);
    let supply_before = env.token_supply_observed();
    let foreign_market_before = env.market_data(true);
    let foreign_portfolio_before = env.foreign_portfolio_data();
    let foreign_vault_before = env.token_amount(env.foreign_vault);
    let foreign_destination_before = env.token_amount(env.foreign_actor.destination_token);
    let mut max_compute_units = 0;

    let first_trade = env
        .trade_no_cpi(0, 1, 0, POS_SCALE as i128, config.initial_price, 0)
        .map_err(|error| format!("open asset-0 matched position: {error}"))?;
    max_compute_units = max_compute_units.max(first_trade.compute_units);
    assert_public_stock_census("INV-066 after first matched trade", &env)?;
    assert_public_encumbrance_census("INV-066 after first matched trade", &env)?;
    let second_trade = env
        .trade_no_cpi(2, 3, 1, 2 * POS_SCALE as i128, config.initial_price, 0)
        .map_err(|error| format!("open asset-1 matched position: {error}"))?;
    max_compute_units = max_compute_units.max(second_trade.compute_units);
    assert_public_stock_census("INV-066 after second matched trade", &env)?;
    assert_public_encumbrance_census("INV-066 after second matched trade", &env)?;

    let resolve = env
        .resolve_market()
        .map_err(|error| format!("resolve market: {error}"))?;
    max_compute_units = max_compute_units.max(resolve.compute_units);
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err("ResolveMarket did not enter Resolved mode".to_string());
    }
    assert_public_stock_census("INV-066 after market resolution", &env)?;
    assert_public_encumbrance_census("INV-066 after market resolution", &env)?;

    let mut payouts = [0u64; PRIMARY_ACTOR_COUNT];
    for actor in order {
        let destination = env.actors[actor].destination_token;
        let destination_before = env.token_amount(destination);
        let vault_before = env.token_amount(env.vault);
        let close = env
            .close_resolved_primary(actor)
            .map_err(|error| format!("close resolved actor {actor}: {error}"))?;
        max_compute_units = max_compute_units.max(close.compute_units);
        let destination_after = env.token_amount(destination);
        let vault_after = env.token_amount(env.vault);
        let payout = destination_after
            .checked_sub(destination_before)
            .ok_or_else(|| format!("actor {actor} destination decreased"))?;
        let vault_debit = vault_before
            .checked_sub(vault_after)
            .ok_or_else(|| format!("actor {actor} payout increased the vault"))?;
        if payout != vault_debit {
            return Err(format!(
                "actor {actor} payout/vault mismatch: payout={payout}, vault debit={vault_debit}"
            ));
        }
        payouts[actor] = payout;

        // A completed claim is a fixed point. Either retry route may reject or
        // return success, but neither may mutate accounting or pay twice.
        let market_at_fixed_point = env.market_data(false);
        let portfolio_at_fixed_point = env.primary_portfolio_data(actor);
        let tokens_at_fixed_point = env.all_token_account_data();
        let _ = env.close_resolved_primary(actor);
        let _ = env.claim_resolved_payout_topup_primary(actor);
        if env.market_data(false) != market_at_fixed_point
            || env.primary_portfolio_data(actor) != portfolio_at_fixed_point
            || env.all_token_account_data() != tokens_at_fixed_point
        {
            return Err(format!(
                "actor {actor} terminal payout retry mutated state or paid twice"
            ));
        }
        assert_public_stock_census(&format!("INV-066 after resolved claimant {actor}"), &env)?;
        assert_public_encumbrance_census(
            &format!("INV-066 after resolved claimant {actor}"),
            &env,
        )?;
    }

    for actor in order {
        let close_portfolio = env
            .close_primary_portfolio(actor)
            .map_err(|error| format!("close terminal portfolio {actor}: {error}"))?;
        max_compute_units = max_compute_units.max(close_portfolio.compute_units);
    }

    let expected_payouts = config
        .actor_deposits
        .map(|value| u64::try_from(value).expect("fixture deposit fits SPL amount"));
    if payouts != expected_payouts {
        return Err(format!(
            "unchanged-price zero-fee resolution did not return deposits: payouts={payouts:?}, deposits={:?}",
            config.actor_deposits
        ));
    }
    let terminal = env.primary_market_state().1;
    if terminal.vault != 0
        || terminal.insurance != 0
        || terminal.c_tot != 0
        || terminal.materialized_portfolio_count != 0
        || env.token_amount(env.vault) != 0
    {
        return Err(format!(
            "terminal accounting was not empty: vault={}/{}, insurance={}, c_tot={}, portfolios={}",
            terminal.vault,
            env.token_amount(env.vault),
            terminal.insurance,
            terminal.c_tot,
            terminal.materialized_portfolio_count
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err("terminal lifecycle changed observed SPL supply".to_string());
    }

    let close_slab = env
        .close_primary_slab()
        .map_err(|error| format!("close fully drained slab: {error}"))?;
    max_compute_units = max_compute_units.max(close_slab.compute_units);
    let closed_market = env
        .svm
        .get_account(&env.market)
        .ok_or("closed market account disappeared unexpectedly")?;
    assert_closed_market_tombstone(&closed_market);

    if env.market_data(true) != foreign_market_before
        || env.foreign_portfolio_data() != foreign_portfolio_before
        || env.token_amount(env.foreign_vault) != foreign_vault_before
        || env.token_amount(env.foreign_actor.destination_token) != foreign_destination_before
    {
        return Err("primary terminal lifecycle mutated the foreign market".to_string());
    }
    if max_compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "terminal lifecycle exceeded the transaction CU limit: {max_compute_units}"
        ));
    }

    Ok(TerminalOutcome {
        payouts,
        max_compute_units,
    })
}

fn permutations(values: &mut [usize], start: usize, out: &mut Vec<[usize; PRIMARY_ACTOR_COUNT]>) {
    if start == values.len() {
        out.push(values.try_into().expect("five claimant indices"));
        return;
    }
    for index in start..values.len() {
        values.swap(start, index);
        permutations(values, start + 1, out);
        values.swap(start, index);
    }
}

#[test]
fn v16_program_full_terminal_lifecycle_is_claimant_order_independent() {
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2, 3, 4], 0, &mut orders);
    assert_eq!(
        orders.len(),
        120,
        "the bounded model must cover all 5! orders"
    );

    let baseline = run_terminal_lifecycle([0, 1, 2, 3, 4])
        .expect("canonical terminal lifecycle must complete");
    for order in orders {
        let outcome = run_terminal_lifecycle(order)
            .unwrap_or_else(|error| panic!("terminal order {order:?}: {error}"));
        assert_eq!(
            outcome.payouts, baseline.payouts,
            "claimant order changed an economic payout: {order:?}"
        );
    }
}

#[test]
fn v16_program_partial_receipt_release_and_claim_order_is_economically_invariant() {
    let evidence = verify_resolved_receipt_release_claim_order_matrix()
        .expect("public partial-receipt release/claim order matrix");
    assert_eq!(evidence.world_count, 16);
    assert_eq!(evidence.scheduled_claim_attempt_count, 32);
    assert_eq!(
        evidence.scheduled_paying_claim_count
            + evidence.scheduled_progress_only_claim_count
            + evidence.scheduled_noop_claim_count,
        evidence.scheduled_claim_attempt_count,
    );
    assert!(evidence.scheduled_paying_claim_count != 0);
    assert!(evidence.scheduled_progress_only_claim_count != 0);
    assert!(evidence.terminal_paid != 0);
    assert!(evidence.terminal_paid <= evidence.receipt_face);
    assert_eq!(evidence.terminal_actor_count, PRIMARY_ACTOR_COUNT);
    assert_eq!(evidence.final_engine_vault, evidence.final_spl_vault);
}

#[test]
fn v16_program_prior_insurance_drains_after_every_user_claim_before_close_slab() {
    const INSURANCE_ATOMS: u128 = 37;

    let config = MarketConfig::default();
    let mut env = V16Svm::new([0x70; 32], config);
    let supply_before = env.token_supply_observed();
    let foreign_market_before = env.market_data(true);
    let provider_source_before = env.token_amount(env.provider_source_token);
    let provider_destination_before = env.token_amount(env.provider_destination_token);
    let vault_before = env.token_amount(env.vault);
    let mut max_compute_units = 0;

    let top_up = env
        .top_up_insurance(INSURANCE_ATOMS)
        .map_err(|error| format!("legacy asset-0 insurance top-up: {error}"))
        .expect("the canonical insurance authority must fund terminal insurance");
    max_compute_units = max_compute_units.max(top_up.compute_units);
    assert_eq!(
        env.token_amount(env.provider_source_token) + INSURANCE_ATOMS as u64,
        provider_source_before,
        "insurance funding must debit the authority's SPL source exactly"
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault_before + INSURANCE_ATOMS as u64,
        "insurance funding must credit canonical custody exactly"
    );
    assert_eq!(env.primary_market_state().1.insurance, INSURANCE_ATOMS);
    assert_public_stock_census("INV-070 after prior insurance funding", &env)
        .expect("prior insurance funding must satisfy the complete stock census");
    assert_public_encumbrance_census("INV-070 after prior insurance funding", &env)
        .expect("prior insurance funding must not create an untracked encumbrance");

    let resolve = env
        .resolve_market()
        .expect("an unchanged live market must resolve");
    max_compute_units = max_compute_units.max(resolve.compute_units);
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let close = env
            .close_resolved_primary(actor)
            .unwrap_or_else(|error| panic!("resolve claimant {actor}: {error}"));
        max_compute_units = max_compute_units.max(close.compute_units);
        assert_public_stock_census(&format!("INV-070 after claimant {actor}"), &env)
            .unwrap_or_else(|error| panic!("claimant {actor} stock census: {error}"));
        assert_public_encumbrance_census(&format!("INV-070 after claimant {actor}"), &env)
            .unwrap_or_else(|error| panic!("claimant {actor} encumbrance census: {error}"));
    }

    for actor in 0..PRIMARY_ACTOR_COUNT {
        let close = env
            .close_primary_portfolio(actor)
            .unwrap_or_else(|error| panic!("close terminal portfolio {actor}: {error}"));
        max_compute_units = max_compute_units.max(close.compute_units);
        assert_public_stock_census(&format!("INV-070 after portfolio close {actor}"), &env)
            .unwrap_or_else(|error| panic!("portfolio close {actor} stock census: {error}"));
        assert_public_encumbrance_census(&format!("INV-070 after portfolio close {actor}"), &env)
            .unwrap_or_else(|error| panic!("portfolio close {actor} encumbrance census: {error}"));
    }

    let before_withdraw = env.primary_market_state().1;
    assert_eq!(before_withdraw.c_tot, 0);
    assert_eq!(before_withdraw.materialized_portfolio_count, 0);
    assert_eq!(before_withdraw.insurance, INSURANCE_ATOMS);
    assert_eq!(before_withdraw.vault, INSURANCE_ATOMS);
    let withdraw = env
        .withdraw_terminal_insurance_as_admin(INSURANCE_ATOMS)
        .expect("terminal insurance authority must be able to drain prior insurance");
    max_compute_units = max_compute_units.max(withdraw.compute_units);
    assert_eq!(
        env.token_amount(env.provider_destination_token),
        provider_destination_before + INSURANCE_ATOMS as u64,
        "terminal insurance must reach only the configured authority"
    );
    let drained = env.primary_market_state().1;
    assert_eq!(drained.insurance, 0);
    assert_eq!(drained.vault, 0);
    assert_eq!(env.token_amount(env.vault), 0);
    assert_public_stock_census("INV-070 after terminal insurance drain", &env)
        .expect("terminal insurance drain must satisfy the complete stock census");
    assert_public_encumbrance_census("INV-070 after terminal insurance drain", &env)
        .expect("terminal insurance drain must leave no hidden encumbrance");
    assert_eq!(env.token_supply_observed(), supply_before);

    let slab_close = env
        .close_primary_slab()
        .expect("zero-residue market must reach CloseSlab after insurance drain");
    max_compute_units = max_compute_units.max(slab_close.compute_units);
    let closed_market = env
        .svm
        .get_account(&env.market)
        .expect("closed market account remains observable");
    assert_closed_market_tombstone(&closed_market);
    assert_eq!(env.market_data(true), foreign_market_before);
    assert!(max_compute_units < TX_CU_LIMIT);
}
