//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: every landing order either rejects atomically or remains inside every
//! affected signer’s latest authorization.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_matcher_mutation_order_rejects_revoked_capability` retains an LP-signed matcher
//! enable, lands the LP's later revoke, and proves both a CPI fill and replay of the earlier enable
//! reject with exact rollback. It then signs against the current sequence and executes a complete
//! CPI open/close and SPL withdrawal path, proving the guard does not disable fresh LP consent.
//! The matcher sequence is read from the real portfolio account before and after each transition.
//!
//! Guarantee boundary: this fixed-pin regression covers the portfolio-scoped matcher capability.
//! Other retained policy domains are owned by INV-014 and require their own scope-local sequences.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::POS_SCALE;
use solana_sdk::{pubkey::Pubkey, transaction::Transaction};

const LP: usize = 0;
const TAKER: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LandingOperation {
    Enable,
    Disable,
    Trade,
}

impl LandingOperation {
    const PERMUTATIONS: [[Self; 3]; 6] = [
        [Self::Enable, Self::Disable, Self::Trade],
        [Self::Enable, Self::Trade, Self::Disable],
        [Self::Disable, Self::Enable, Self::Trade],
        [Self::Disable, Self::Trade, Self::Enable],
        [Self::Trade, Self::Enable, Self::Disable],
        [Self::Trade, Self::Disable, Self::Enable],
    ];
}

struct RetainedRequests {
    enable: Transaction,
    disable: Transaction,
    trade: Transaction,
}

impl RetainedRequests {
    fn transaction(&self, operation: LandingOperation) -> Transaction {
        match operation {
            LandingOperation::Enable => self.enable.clone(),
            LandingOperation::Disable => self.disable.clone(),
            LandingOperation::Trade => self.trade.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    matcher_contexts: Vec<Vec<u8>>,
    tokens: Vec<(Pubkey, Vec<u8>)>,
    lamports: Vec<(Pubkey, u64)>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        matcher_contexts: env.all_matcher_context_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn run_landing_order(order: [LandingOperation; 3]) {
    let mut seed = [0x10; 32];
    seed[0] = LandingOperation::PERMUTATIONS
        .iter()
        .position(|candidate| *candidate == order)
        .expect("known permutation") as u8;
    let config = MarketConfig::default();
    let mut env = V16Svm::new(seed, config);
    let supply = env.token_supply_observed();
    let initial_sequence = env.primary_portfolio_matcher_sequence(LP);
    let requests = RetainedRequests {
        enable: env.build_retained_matcher_config(LP, 1),
        disable: env.build_retained_matcher_config(LP, 0),
        trade: env.build_retained_cpi_trade(TAKER, LP, 0, POS_SCALE as i128, config.initial_price),
    };

    let mut winning_control = None;
    let mut trade_landed = false;
    for operation in order {
        let before = snapshot(&env);
        let result = env.land_retained(requests.transaction(operation));
        match operation {
            LandingOperation::Enable | LandingOperation::Disable => {
                let enabled = operation == LandingOperation::Enable;
                if winning_control.is_none() {
                    result.expect("first same-sequence control must land");
                    winning_control = Some(enabled);
                } else {
                    result.expect_err("second same-sequence control must reject");
                    assert_eq!(
                        snapshot(&env),
                        before,
                        "losing retained control must roll back exactly: {order:?}"
                    );
                }
            }
            LandingOperation::Trade => {
                let matcher_enabled_at_landing = winning_control.unwrap_or(true);
                if matcher_enabled_at_landing {
                    result.expect("retained CPI trade inside current consent must land");
                    trade_landed = true;
                } else {
                    result.expect_err("retained CPI trade after disable must reject");
                    assert_eq!(
                        snapshot(&env),
                        before,
                        "trade outside current consent must roll back exactly: {order:?}"
                    );
                }
            }
        }
    }

    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        initial_sequence + 1,
        "exactly one competing matcher control may land: {order:?}"
    );
    let (_, after_order) = env.primary_market_state();
    let expected_oi = if trade_landed { POS_SCALE } else { 0 };
    assert_eq!(after_order.assets[0].oi_eff_long_q, expected_oi);
    assert_eq!(after_order.assets[0].oi_eff_short_q, expected_oi);

    if trade_landed {
        env.trade_no_cpi(TAKER, LP, 0, -(POS_SCALE as i128), config.initial_price, 0)
            .expect("both owners retain a matcher-independent public exit");
    }
    let (_, terminal) = env.primary_market_state();
    assert_eq!(terminal.assets[0].oi_eff_long_q, 0);
    assert_eq!(terminal.assets[0].oi_eff_short_q, 0);
    assert_eq!(env.token_supply_observed(), supply);
}

#[test]
fn v16_program_conflicting_matcher_controls_and_trade_exhaust_all_landing_orders() {
    for order in LandingOperation::PERMUTATIONS {
        run_landing_order(order);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_010_matcher_order_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_matcher_mutation_order_rejects_revoked_capability(
        seed in any::<[u8; 32]>()
    ) {
        let protection = verify_matcher_mutation_order_safety(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            protection.satisfies_invariant(),
            "matcher supersession protection failed: {:?}",
            protection
        );
    }
}
