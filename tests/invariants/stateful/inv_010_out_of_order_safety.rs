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
//! `v16_program_portfolio_value_and_control_requests_exhaust_all_landing_orders` crosses retained
//! deposit, withdrawal, and matcher-disable requests over all 3! orders and three value boundaries.
//! Since all three bind one portfolio sequence, exactly the first may commit. The test independently
//! checks that winner's capital/SPL delta, exact rollback of both stale followers, and a fresh full
//! owner withdrawal, so sequencing cannot turn out-of-order delivery into value duplication or an
//! exit lock.
//! `v16_program_deposit_and_owner_reduction_commute_across_independent_bindings` then proves a
//! retained capital-sequence mutation and a retained position-episode mutation both land in either
//! order and converge economically. The only raw-state difference is the health-certificate cache:
//! a reduction landing last recertifies against the deposited capital, while a deposit landing last
//! conservatively invalidates the older certificate. Both orders leave fresh complete position and
//! capital exits.
//! `v16_program_authority_handoff_and_retained_policy_obey_both_landing_orders` crosses a retained
//! market-authority handoff with all eight market/asset-0 policy lanes at low, midpoint, and maximum
//! valid values. Policy-first permits both authorized requests; handoff-first makes the old
//! authority's policy reject with an exact economic snapshot. In all 48 worlds the incoming
//! authority installs a fresh lane-specific policy and a funded user retains a complete SPL exit.
//!
//! Guarantee boundary: this fixed-pin regression covers the portfolio-scoped matcher capability.
//! The authority/policy composition covers the market-authority and inherited asset-0 insurance
//! authority roles. Resolve and claim transitions remain to be crossed with authority changes.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm, USER_DEPOSIT};
use percolator::{HealthCertV16Account, PortfolioAccountV16Account, POS_SCALE};
use percolator_prog::ix::Instruction as ProgInstruction;
use solana_sdk::{pubkey::Pubkey, signature::Signer, transaction::Transaction};

const LP: usize = 0;
const TAKER: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LandingOperation {
    Enable,
    Disable,
    Trade,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortfolioLandingOperation {
    Deposit,
    Withdraw,
    Disable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthorityPolicyKind {
    TradeFee,
    FeeRedirect,
    MarketInitFee,
    LiquidationFee,
    MaintenanceFee,
    Resolve,
    BackingFeeLong,
    BackingFeeShort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicyBoundary {
    Low,
    Midpoint,
    Maximum,
}

impl PolicyBoundary {
    const ALL: [Self; 3] = [Self::Low, Self::Midpoint, Self::Maximum];

    fn bps(self, fresh: bool) -> u16 {
        match (self, fresh) {
            (Self::Low, false) => 1,
            (Self::Low, true) => 2,
            (Self::Midpoint, false) => 5_000,
            (Self::Midpoint, true) => 5_001,
            (Self::Maximum, false) => 10_000,
            (Self::Maximum, true) => 9_999,
        }
    }

    fn init_fee(self, fresh: bool) -> u128 {
        match (self, fresh) {
            (Self::Low, false) => 1,
            (Self::Low, true) => 2,
            (Self::Midpoint, false) => u32::MAX as u128,
            (Self::Midpoint, true) => u32::MAX as u128 + 1,
            (Self::Maximum, false) => u64::MAX as u128,
            (Self::Maximum, true) => u64::MAX as u128 - 1,
        }
    }

    fn resolve_slots(self, fresh: bool) -> (u64, u64) {
        let stale_max = percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS;
        let delay_max = percolator_prog::constants::MAX_FORCE_CLOSE_DELAY_SLOTS;
        match (self, fresh) {
            (Self::Low, false) => (1, 1),
            (Self::Low, true) => (2, 2),
            (Self::Midpoint, false) => (stale_max / 2, delay_max / 2),
            (Self::Midpoint, true) => (stale_max / 2 + 1, delay_max / 2 + 1),
            (Self::Maximum, false) => (stale_max, delay_max),
            (Self::Maximum, true) => (stale_max - 1, delay_max - 1),
        }
    }
}

impl AuthorityPolicyKind {
    const ALL: [Self; 8] = [
        Self::TradeFee,
        Self::FeeRedirect,
        Self::MarketInitFee,
        Self::LiquidationFee,
        Self::MaintenanceFee,
        Self::Resolve,
        Self::BackingFeeLong,
        Self::BackingFeeShort,
    ];

    fn sequence(self, env: &V16Svm) -> u64 {
        let sequences = env.primary_control_sequences(0);
        match self {
            Self::TradeFee => sequences.trade_fee,
            Self::FeeRedirect => sequences.fee_redirect,
            Self::MarketInitFee => sequences.market_init_fee,
            Self::LiquidationFee => sequences.liquidation_fee,
            Self::MaintenanceFee => sequences.maintenance_fee,
            Self::Resolve => sequences.permissionless_resolve,
            Self::BackingFeeLong => sequences.backing_fee_long,
            Self::BackingFeeShort => sequences.backing_fee_short,
        }
    }

    fn instruction(self, env: &V16Svm, boundary: PolicyBoundary, fresh: bool) -> ProgInstruction {
        let policy_sequence = self.sequence(env) + 1;
        let bps = boundary.bps(fresh);
        match self {
            Self::TradeFee => ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: u64::from(bps),
                policy_sequence,
            },
            Self::FeeRedirect => ProgInstruction::UpdateFeeRedirectPolicy {
                redirect_bps: bps,
                policy_sequence,
            },
            Self::MarketInitFee => ProgInstruction::UpdateMarketInitFeePolicy {
                min_init_fee: boundary.init_fee(fresh),
                policy_sequence,
            },
            Self::LiquidationFee => ProgInstruction::UpdateLiquidationFeePolicy {
                cranker_share_bps: bps,
                policy_sequence,
            },
            Self::MaintenanceFee => ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps: bps,
                policy_sequence,
            },
            Self::Resolve => {
                let (stale_slots, force_close_delay_slots) = boundary.resolve_slots(fresh);
                ProgInstruction::ConfigurePermissionlessResolve {
                    asset_generation_frontier: env.primary_market_state().1.next_market_id,
                    stale_slots,
                    force_close_delay_slots,
                    policy_sequence,
                }
            }
            Self::BackingFeeLong | Self::BackingFeeShort => {
                ProgInstruction::UpdateBackingFeePolicy {
                    domain: u16::from(self == Self::BackingFeeShort),
                    market_id: env.primary_market_state().1.assets[0].market_id,
                    fee_bps: bps,
                    insurance_share_bps: bps,
                    policy_sequence,
                }
            }
        }
    }

    fn assert_value(self, env: &V16Svm, boundary: PolicyBoundary, fresh: bool) {
        let (cfg, _) = env.primary_market_state();
        let bps = boundary.bps(fresh);
        match self {
            Self::TradeFee => assert_eq!(cfg.trade_fee_base_bps, u64::from(bps)),
            Self::FeeRedirect => assert_eq!(cfg.fee_redirect_to_market_0_bps, bps),
            Self::MarketInitFee => {
                assert_eq!(cfg.permissionless_market_init_fee, boundary.init_fee(fresh))
            }
            Self::LiquidationFee => assert_eq!(cfg.liquidation_cranker_fee_share_bps, bps),
            Self::MaintenanceFee => assert_eq!(cfg.maintenance_cranker_fee_share_bps, bps),
            Self::Resolve => {
                let (stale_slots, force_close_delay_slots) = boundary.resolve_slots(fresh);
                assert_eq!(cfg.permissionless_resolve_stale_slots, stale_slots);
                assert_eq!(cfg.force_close_delay_slots, force_close_delay_slots);
            }
            Self::BackingFeeLong => {
                assert_eq!(cfg.backing_trade_fee_bps_long, bps);
                assert_eq!(cfg.backing_trade_fee_insurance_share_bps_long, bps);
            }
            Self::BackingFeeShort => {
                assert_eq!(cfg.backing_trade_fee_bps_short, bps);
                assert_eq!(cfg.backing_trade_fee_insurance_share_bps_short, bps);
            }
        }
    }

    fn assert_initial_value(self, env: &V16Svm) {
        let (cfg, _) = env.primary_market_state();
        match self {
            Self::TradeFee => assert_eq!(cfg.trade_fee_base_bps, 0),
            Self::FeeRedirect => assert_eq!(cfg.fee_redirect_to_market_0_bps, 0),
            Self::MarketInitFee => assert_eq!(cfg.permissionless_market_init_fee, 0),
            Self::LiquidationFee => assert_eq!(cfg.liquidation_cranker_fee_share_bps, 0),
            Self::MaintenanceFee => assert_eq!(cfg.maintenance_cranker_fee_share_bps, 0),
            Self::Resolve => {
                assert_eq!(cfg.permissionless_resolve_stale_slots, 0);
                assert_eq!(cfg.force_close_delay_slots, 0);
            }
            Self::BackingFeeLong => {
                assert_eq!(cfg.backing_trade_fee_bps_long, 0);
                assert_eq!(cfg.backing_trade_fee_insurance_share_bps_long, 0);
            }
            Self::BackingFeeShort => {
                assert_eq!(cfg.backing_trade_fee_bps_short, 0);
                assert_eq!(cfg.backing_trade_fee_insurance_share_bps_short, 0);
            }
        }
    }
}

impl PortfolioLandingOperation {
    const PERMUTATIONS: [[Self; 3]; 6] = [
        [Self::Deposit, Self::Withdraw, Self::Disable],
        [Self::Deposit, Self::Disable, Self::Withdraw],
        [Self::Withdraw, Self::Deposit, Self::Disable],
        [Self::Withdraw, Self::Disable, Self::Deposit],
        [Self::Disable, Self::Deposit, Self::Withdraw],
        [Self::Disable, Self::Withdraw, Self::Deposit],
    ];
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

struct RetainedPortfolioRequests {
    deposit: Transaction,
    withdraw: Transaction,
    disable: Transaction,
}

impl RetainedPortfolioRequests {
    fn transaction(&self, operation: PortfolioLandingOperation) -> Transaction {
        match operation {
            PortfolioLandingOperation::Deposit => self.deposit.clone(),
            PortfolioLandingOperation::Withdraw => self.withdraw.clone(),
            PortfolioLandingOperation::Disable => self.disable.clone(),
        }
    }
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

fn byte_differences(left: &[u8], right: &[u8]) -> Vec<(usize, u8, u8)> {
    left.iter()
        .zip(right)
        .enumerate()
        .filter_map(|(index, (left, right))| (*left != *right).then_some((index, *left, *right)))
        .collect()
}

fn snapshot_difference_summary(left: &EconomicSnapshot, right: &EconomicSnapshot) -> Vec<String> {
    let mut out = Vec::new();
    for (label, left, right) in [
        ("market", left.market.as_slice(), right.market.as_slice()),
        (
            "foreign_market",
            left.foreign_market.as_slice(),
            right.foreign_market.as_slice(),
        ),
        (
            "backing_ledger",
            left.backing_ledger.as_slice(),
            right.backing_ledger.as_slice(),
        ),
    ] {
        let differences = byte_differences(left, right);
        if !differences.is_empty() {
            out.push(format!("{label}: {differences:?}"));
        }
    }
    for (index, (left, right)) in left.portfolios.iter().zip(&right.portfolios).enumerate() {
        let differences = byte_differences(left, right);
        if !differences.is_empty() {
            out.push(format!("portfolio[{index}]: {differences:?}"));
        }
    }
    if left.matcher_contexts != right.matcher_contexts {
        out.push("matcher contexts differ".to_string());
    }
    if left.tokens != right.tokens {
        out.push("token accounts differ".to_string());
    }
    if left.lamports != right.lamports {
        out.push("lamports differ".to_string());
    }
    out
}

fn assert_deposit_reduction_snapshots_converge(
    mut reduction_last: EconomicSnapshot,
    mut deposit_last: EconomicSnapshot,
    deposit_amount: u128,
) {
    const PORTFOLIO_WIRE_OFFSET: usize = percolator_prog::constants::HEADER_LEN;
    const HEALTH_CERT_OFFSET: usize =
        PORTFOLIO_WIRE_OFFSET + core::mem::offset_of!(PortfolioAccountV16Account, health_cert);
    const HEALTH_CERT_VALID_OFFSET: usize =
        HEALTH_CERT_OFFSET + core::mem::offset_of!(HealthCertV16Account, valid);
    const HEALTH_CERT_END: usize =
        HEALTH_CERT_OFFSET + core::mem::size_of::<HealthCertV16Account>();

    let reduction_wire: PortfolioAccountV16Account = bytemuck::pod_read_unaligned(
        &reduction_last.portfolios[LP][PORTFOLIO_WIRE_OFFSET
            ..PORTFOLIO_WIRE_OFFSET + core::mem::size_of::<PortfolioAccountV16Account>()],
    );
    let deposit_wire: PortfolioAccountV16Account = bytemuck::pod_read_unaligned(
        &deposit_last.portfolios[LP][PORTFOLIO_WIRE_OFFSET
            ..PORTFOLIO_WIRE_OFFSET + core::mem::size_of::<PortfolioAccountV16Account>()],
    );
    let mut reduction_cert = reduction_wire
        .health_cert
        .try_to_runtime()
        .expect("decode reduction-last certificate");
    let mut deposit_cert = deposit_wire
        .health_cert
        .try_to_runtime()
        .expect("decode deposit-last certificate");

    assert_eq!(reduction_last.portfolios[LP][HEALTH_CERT_VALID_OFFSET], 1);
    assert_eq!(deposit_last.portfolios[LP][HEALTH_CERT_VALID_OFFSET], 0);
    assert!(reduction_cert.valid);
    assert!(!deposit_cert.valid);
    assert_eq!(
        reduction_cert.certified_equity,
        deposit_cert.certified_equity + i128::try_from(deposit_amount).unwrap(),
        "the recertified route must include the landed deposit"
    );
    reduction_cert.certified_equity = deposit_cert.certified_equity;
    reduction_cert.valid = false;
    deposit_cert.valid = false;
    assert_eq!(reduction_cert, deposit_cert);

    reduction_last.portfolios[LP][HEALTH_CERT_OFFSET..HEALTH_CERT_END].fill(0);
    deposit_last.portfolios[LP][HEALTH_CERT_OFFSET..HEALTH_CERT_END].fill(0);
    let differences = snapshot_difference_summary(&reduction_last, &deposit_last);
    assert!(
        differences.is_empty(),
        "independent retained operations may differ only by their conservative health certificate: {differences:?}"
    );
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

fn run_portfolio_value_control_landing_order(order: [PortfolioLandingOperation; 3], amount: u128) {
    let order_index = PortfolioLandingOperation::PERMUTATIONS
        .iter()
        .position(|candidate| *candidate == order)
        .expect("known portfolio permutation");
    let mut seed = [0x6d; 32];
    seed[0] = order_index as u8;
    seed[1..9].copy_from_slice(&(amount as u64).to_le_bytes());
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let subject = &env.actors[LP];
    let source = subject.source_token;
    let destination = subject.destination_token;
    let initial_capital = env.primary_portfolio(LP).capital.get();
    let initial_source = u128::from(env.token_amount(source));
    let initial_destination = u128::from(env.token_amount(destination));
    let initial_vault = u128::from(env.token_amount(env.vault));
    let initial_sequence = env.primary_portfolio_matcher_sequence(LP);
    let supply = env.token_supply_observed();
    assert!(amount > 0 && amount < initial_capital);
    assert!(amount <= initial_source);

    let requests = RetainedPortfolioRequests {
        deposit: env.build_retained_deposit(LP, amount),
        withdraw: env.build_retained_withdrawal(LP, amount),
        disable: env.build_retained_matcher_config(LP, 0),
    };

    for (index, operation) in order.into_iter().enumerate() {
        let before = snapshot(&env);
        let result = env.land_retained(requests.transaction(operation));
        if index == 0 {
            result.expect("first current-sequence portfolio request must land");
        } else {
            result.expect_err("later request retained at the consumed sequence must reject");
            assert_eq!(
                snapshot(&env),
                before,
                "stale follower must roll back exactly: {order:?}, amount={amount}, op={operation:?}"
            );
        }
    }

    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        initial_sequence + 1,
        "exactly one shared-sequence request may commit"
    );
    let (expected_capital, expected_source, expected_destination, expected_vault) = match order[0] {
        PortfolioLandingOperation::Deposit => (
            initial_capital + amount,
            initial_source - amount,
            initial_destination,
            initial_vault + amount,
        ),
        PortfolioLandingOperation::Withdraw => (
            initial_capital - amount,
            initial_source,
            initial_destination + amount,
            initial_vault - amount,
        ),
        PortfolioLandingOperation::Disable => (
            initial_capital,
            initial_source,
            initial_destination,
            initial_vault,
        ),
    };
    assert_eq!(env.primary_portfolio(LP).capital.get(), expected_capital);
    assert_eq!(u128::from(env.token_amount(source)), expected_source);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        expected_destination
    );
    assert_eq!(u128::from(env.token_amount(env.vault)), expected_vault);
    let matcher =
        percolator_prog::state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP))
            .expect("decode matcher state after landing order");
    assert_eq!(
        matcher.enabled(),
        u64::from(order[0] != PortfolioLandingOperation::Disable)
    );
    assert_eq!(env.token_supply_observed(), supply);

    env.withdraw_primary(LP, expected_capital)
        .expect("a fresh owner request must retain a complete capital exit");
    assert_eq!(env.primary_portfolio(LP).capital.get(), 0);
    assert_eq!(u128::from(env.token_amount(source)), expected_source);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        expected_destination + expected_capital
    );
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        expected_vault - expected_capital
    );
    assert_eq!(env.token_supply_observed(), supply);
}

#[test]
fn v16_program_portfolio_value_and_control_requests_exhaust_all_landing_orders() {
    for amount in [1, 1_000, USER_DEPOSIT - 1] {
        for order in PortfolioLandingOperation::PERMUTATIONS {
            run_portfolio_value_control_landing_order(order, amount);
        }
    }
}

fn run_deposit_reduction_order(
    seed: [u8; 32],
    deposit_amount: u128,
    reduce_q: u128,
    deposit_first: bool,
) -> (EconomicSnapshot, u128) {
    let config = MarketConfig::default();
    let mut env = V16Svm::new(seed, config);
    env.trade_no_cpi(LP, TAKER, 0, 2 * POS_SCALE as i128, config.initial_price, 0)
        .expect("prepare real bilateral exposure");
    let initial_sequence = env.primary_portfolio_matcher_sequence(LP);
    let initial_position_epoch = env.primary_portfolio_position_epoch(LP);
    let initial_capital = env.primary_portfolio(LP).capital.get();
    let source = env.actors[LP].source_token;
    let initial_source = u128::from(env.token_amount(source));
    let destination = env.actors[LP].destination_token;
    let initial_destination = u128::from(env.token_amount(destination));
    let initial_vault = u128::from(env.token_amount(env.vault));
    let supply = env.token_supply_observed();
    let deposit = env.build_retained_deposit(LP, deposit_amount);
    let reduce = env.build_retained_rebalance_reduce(LP, 0, reduce_q);

    let requests = if deposit_first {
        [("deposit", deposit), ("reduction", reduce)]
    } else {
        [("reduction", reduce), ("deposit", deposit)]
    };
    for (label, request) in requests {
        env.land_retained(request).unwrap_or_else(|error| {
            panic!("{label} must land in either independent order: {error}")
        });
    }

    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        initial_sequence + 1
    );
    assert_eq!(
        env.primary_portfolio_position_epoch(LP),
        initial_position_epoch + 1
    );
    assert_eq!(
        env.primary_portfolio(LP).capital.get(),
        initial_capital + deposit_amount
    );
    assert_eq!(
        u128::from(env.token_amount(source)),
        initial_source - deposit_amount
    );
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        initial_vault + deposit_amount
    );
    assert_eq!(env.token_supply_observed(), supply);
    let remaining_q = env.primary_portfolio(LP).legs[0]
        .basis_pos_q
        .get()
        .unsigned_abs();
    assert_eq!(remaining_q, 2 * POS_SCALE as u128 - reduce_q);
    let converged = snapshot(&env);

    let final_reduction = env.build_retained_rebalance_reduce(LP, 0, remaining_q);
    env.land_retained(final_reduction)
        .expect("a fresh owner reduction must remain live after either landing order");
    assert_eq!(env.primary_portfolio(LP).legs[0].basis_pos_q.get(), 0);
    let final_capital = env.primary_portfolio(LP).capital.get();
    env.withdraw_primary(LP, final_capital)
        .expect("a fresh owner withdrawal must remain live after either landing order");
    assert_eq!(env.primary_portfolio(LP).capital.get(), 0);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        initial_destination + final_capital
    );
    assert_eq!(env.token_supply_observed(), supply);
    (converged, remaining_q)
}

#[test]
fn v16_program_deposit_and_owner_reduction_commute_across_independent_bindings() {
    for (case, deposit_amount, reduce_q) in [
        (0u8, 1u128, 1u128),
        (1, 1_000, POS_SCALE as u128),
        (2, 1_000_000, 2 * POS_SCALE as u128 - 1),
    ] {
        let mut seed = [0xa7; 32];
        seed[0] = case;
        let deposit_first = run_deposit_reduction_order(seed, deposit_amount, reduce_q, true);
        let reduction_first = run_deposit_reduction_order(seed, deposit_amount, reduce_q, false);
        assert_eq!(deposit_first.1, reduction_first.1);
        assert_deposit_reduction_snapshots_converge(
            deposit_first.0,
            reduction_first.0,
            deposit_amount,
        );
    }
}

fn run_authority_policy_order(
    seed: [u8; 32],
    policy_kind: AuthorityPolicyKind,
    boundary: PolicyBoundary,
    handoff_first: bool,
) {
    const INCOMING_AUTHORITY: usize = 2;
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply = env.token_supply_observed();
    let initial_sequence = policy_kind.sequence(&env);
    let old_policy_instruction = policy_kind.instruction(&env, boundary, false);
    let retained_policy = env.build_retained_market_control_from_admin(old_policy_instruction);
    let retained_handoff =
        env.build_retained_market_authority_handoff_from_admin(INCOMING_AUTHORITY);

    if handoff_first {
        env.land_retained(retained_handoff)
            .expect("the current authority's handoff must land");
        let before_stale_policy = snapshot(&env);
        env.land_retained(retained_policy)
            .expect_err("the former authority's retained policy must reject after handoff");
        assert_eq!(
            snapshot(&env),
            before_stale_policy,
            "the superseded authority policy must roll back every economic account"
        );
        assert_eq!(policy_kind.sequence(&env), initial_sequence);
        policy_kind.assert_initial_value(&env);
    } else {
        env.land_retained(retained_policy)
            .expect("the retained policy is authorized before the handoff");
        assert_eq!(policy_kind.sequence(&env), initial_sequence + 1);
        policy_kind.assert_value(&env, boundary, false);
        env.land_retained(retained_handoff)
            .expect("the authority handoff remains live after a policy update");
    }

    let incoming_key = env.actors[INCOMING_AUTHORITY].signer.pubkey().to_bytes();
    let (cfg_after_handoff, _) = env.primary_market_state();
    assert_eq!(cfg_after_handoff.marketauth, incoming_key);
    assert_eq!(
        env.primary_profile(0).insurance_authority,
        incoming_key,
        "asset-0's inherited policy authority must rotate atomically"
    );

    let fresh_policy_instruction = policy_kind.instruction(&env, boundary, true);
    let fresh_policy =
        env.build_retained_market_control_for_actor(INCOMING_AUTHORITY, fresh_policy_instruction);
    env.land_retained(fresh_policy)
        .expect("the incoming authority must retain a fresh policy path");
    policy_kind.assert_value(&env, boundary, true);
    assert_eq!(
        policy_kind.sequence(&env),
        initial_sequence + if handoff_first { 1 } else { 2 }
    );

    let capital = env.primary_portfolio(LP).capital.get();
    env.withdraw_primary(LP, capital)
        .expect("policy/handoff landing order must not block a funded user exit");
    assert_eq!(env.primary_portfolio(LP).capital.get(), 0);
    assert_eq!(env.token_supply_observed(), supply);
}

#[test]
fn v16_program_authority_handoff_and_retained_policy_obey_both_landing_orders() {
    for (kind_index, policy_kind) in AuthorityPolicyKind::ALL.into_iter().enumerate() {
        for (boundary_index, boundary) in PolicyBoundary::ALL.into_iter().enumerate() {
            for handoff_first in [false, true] {
                let mut seed = [0xc1; 32];
                seed[0] = kind_index as u8;
                seed[1] = boundary_index as u8;
                seed[2] = u8::from(handoff_first);
                run_authority_policy_order(seed, policy_kind, boundary, handoff_first);
            }
        }
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
