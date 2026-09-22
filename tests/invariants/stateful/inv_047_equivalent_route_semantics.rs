//! INV-047 - Equivalent-route semantics.
//!
//! Normative obligation: the same authorized economic intent must produce the same normalized
//! state delta through CPI/no-CPI and single/batch public trade routes. Route-specific matcher
//! transport state may differ, but protocol value, positions, OI, fees, custody, and unrelated
//! state may not.
//!
//! Evidence (generated F over public I routes with M comparison): each case creates four identical
//! public LiteSVM worlds, installs the same LP-consented nonzero market base fee, and executes one
//! trade through `TradeNoCpi`, `TradeCpi`, `BatchTradeNoCpi`, and `BatchTradeCpi`. The comparison is
//! byte-exact for both markets, every portfolio, the backing ledger, every SPL account, economic
//! lamports, and token supply after normalizing documented transport/capability differences:
//! the CPI-only matcher request sequence, the single-CPI matcher's 64-byte ABI return cache, and
//! the LP matcher-enabled bit and expiry retained after a matcher-synchronized fill but revoked by
//! a bilateral fill. Enabled/expiry postconditions and exact owner fee debits and insurance credit
//! are checked before normalization. Matcher tuple, fee cap, position epoch, and every other byte
//! remain exact. The fixed matrix covers minimum, interior, and maximum fee rates; the generated
//! matrix varies seed, size, side, and fee rate.
//!
//! The bounded stale-liability product composes all four routes with direct admission, canonical
//! public refresh, omitted-liability hints, reversed cached hints, and duplicate-benign-hint
//! rollback/retry. A stale certificate would admit the candidate, but full refresh includes a
//! 50-atom loss and must reject it. A public SPL deposit then makes the same trade usable. The
//! committed account frames converge after the transport normalization above. Both expiry
//! postconditions are checked before normalization. Certificates are checked with the existing
//! full-refresh oracle. This supplies
//! INV-056 composition evidence without repeating its max-shape omission matrix.
//!
//! Guarantee boundary: this closes the one-leg trade-route and nonzero-fee partition. It does not
//! establish wrapper/engine transition equivalence for every public instruction or equivalence
//! between unrelated direct and composite lifecycle operations.

use super::env_usize;
use crate::support::{
    fuzz_model::{
        assert_current_certificate_matches_snapshot_full_refresh, execute_trade_route,
        independent_health_certificate, TradeRoute,
    },
    v16_svm::{snapshot_engine_full_refresh, MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    constants::{PORTFOLIO_MATCHER_EXPIRY_LEN, PORTFOLIO_MATCHER_EXPIRY_OFF},
    error::PercolatorError,
    ix::CrankObservationHint,
    state,
};
use proptest::prelude::*;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::Signer};

#[path = "inv_047_retained_mixed_transport.rs"]
mod retained_mixed_transport;

const TAKER: usize = 0;
const MAKER: usize = 1;
const MATCHER_RETURN_CACHE_LEN: usize = 64;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedTradeRouteFrame {
    primary_market: Vec<u8>,
    foreign_market: Vec<u8>,
    primary_portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    token_accounts: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
    token_supply: u128,
}

fn normalized_matcher_contexts(env: &V16Svm) -> Result<Vec<Vec<u8>>, String> {
    let mut contexts = env.all_matcher_context_data();
    let maker = contexts
        .get_mut(MAKER)
        .ok_or("INV-047 missing maker matcher context")?;
    if maker.len() < MATCHER_RETURN_CACHE_LEN {
        return Err(format!(
            "INV-047 matcher context is only {} bytes",
            maker.len()
        ));
    }
    maker[..MATCHER_RETURN_CACHE_LEN].fill(0);
    Ok(contexts)
}

fn normalized_primary_market(env: &V16Svm) -> Result<Vec<u8>, String> {
    let mut market = env.market_data(false);
    let (mut config, _) = state::read_market(&market)
        .map_err(|error| format!("INV-047 decode primary market: {error:?}"))?;
    config.matcher_req_seq = 0;
    state::write_wrapper_config(&mut market, &config)
        .map_err(|error| format!("INV-047 normalize matcher request sequence: {error:?}"))?;
    Ok(market)
}

fn normalized_primary_portfolios(env: &V16Svm) -> Result<Vec<Vec<u8>>, String> {
    env.all_primary_portfolio_data()
        .into_iter()
        .enumerate()
        .map(|(index, mut portfolio)| {
            let mut matcher =
                state::read_portfolio_matcher_config(&portfolio).map_err(|error| {
                    format!("INV-047 decode portfolio {index} matcher config: {error:?}")
                })?;
            matcher.set_enabled(0).map_err(|error| {
                format!("INV-047 normalize portfolio {index} matcher state: {error:?}")
            })?;
            state::write_portfolio_matcher_config(&mut portfolio, &matcher).map_err(|error| {
                format!("INV-047 write portfolio {index} matcher normalization: {error:?}")
            })?;
            if index == MAKER {
                portfolio[PORTFOLIO_MATCHER_EXPIRY_OFF
                    ..PORTFOLIO_MATCHER_EXPIRY_OFF + PORTFOLIO_MATCHER_EXPIRY_LEN]
                    .fill(0);
            }
            Ok(portfolio)
        })
        .collect()
}

fn normalized_trade_route_frame(env: &V16Svm) -> Result<NormalizedTradeRouteFrame, String> {
    Ok(NormalizedTradeRouteFrame {
        primary_market: normalized_primary_market(env)?,
        foreign_market: env.market_data(true),
        primary_portfolios: normalized_primary_portfolios(env)?,
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        token_accounts: env.all_token_account_data(),
        matcher_contexts: normalized_matcher_contexts(env)?,
        economic_lamports: env.all_economic_account_lamports(),
        token_supply: env.token_supply_observed(),
    })
}

fn run_nonzero_fee_route(
    seed: [u8; 32],
    route: TradeRoute,
    lots: u8,
    account_a_long: bool,
    fee_bps: u16,
) -> Result<NormalizedTradeRouteFrame, String> {
    if lots == 0 || fee_bps == 0 || fee_bps > 10_000 {
        return Err("INV-047 generated case is outside the nonzero-fee domain".into());
    }
    let mut env = V16Svm::new(seed, MarketConfig::default());
    env.update_trade_fee_policy(u64::from(fee_bps))
        .map_err(|error| format!("INV-047 install common base fee: {error}"))?;
    let initial_contexts = env.all_matcher_context_data();
    let insurance_before = env.primary_market_state().1.insurance;
    let capital_before = [TAKER, MAKER].map(|index| env.primary_portfolio(index).capital.get());
    // Whole lots at the fixed mark have integral notional; each owner pays a rounded-up fee.
    let expected_fee =
        (u128::from(lots) * u128::from(INITIAL_PRICE) * u128::from(fee_bps)).div_ceil(10_000);
    let signed_lots = if account_a_long {
        i128::from(lots)
    } else {
        -i128::from(lots)
    };
    let size_q = signed_lots
        .checked_mul(POS_SCALE as i128)
        .ok_or("INV-047 generated size overflow")?;

    env.begin_public_trace();
    let landed = execute_trade_route(
        &mut env,
        route,
        TAKER,
        MAKER,
        0,
        size_q,
        INITIAL_PRICE,
        u64::from(fee_bps),
    )
    .map_err(|error| format!("INV-047 {route:?} trade failed: {error}"))?;
    if landed.compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "INV-047 {route:?} consumed {} CU",
            landed.compute_units
        ));
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("INV-047 {route:?} public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.len() != 1
        || !trace.steps[0].succeeded
    {
        return Err(format!(
            "INV-047 {route:?} was not one successful public transition: {trace:?}"
        ));
    }

    let (_, group) = env.primary_market_state();
    if group.insurance != insurance_before + 2 * expected_fee {
        return Err(format!(
            "INV-047 {route:?} fee insurance mismatch: {}, expected {}",
            group.insurance,
            insurance_before + 2 * expected_fee
        ));
    }
    if group.vault != group.c_tot + group.insurance
        || u128::from(env.token_amount(env.vault)) != group.vault
    {
        return Err(format!(
            "INV-047 {route:?} custody mismatch: vault={}, c_tot={}, insurance={}, SPL={}",
            group.vault,
            group.c_tot,
            group.insurance,
            env.token_amount(env.vault)
        ));
    }
    let taker = env.primary_portfolio(TAKER);
    let maker = env.primary_portfolio(MAKER);
    if taker.capital.get() != capital_before[0] - expected_fee
        || maker.capital.get() != capital_before[1] - expected_fee
    {
        return Err(format!(
            "INV-047 {route:?} owner fee mismatch: capital={}/{}, before={capital_before:?}, fee/owner={expected_fee}",
            taker.capital.get(), maker.capital.get()
        ));
    }
    for index in [TAKER, MAKER] {
        let portfolio = env.primary_portfolio_data(index);
        let matcher = state::read_portfolio_matcher_config(&portfolio)
            .map_err(|error| format!("INV-047 read portfolio {index} matcher: {error:?}"))?;
        let expiry = state::read_portfolio_matcher_expiry(&portfolio)
            .map_err(|error| format!("INV-047 read portfolio {index} expiry: {error:?}"))?;
        let synchronized =
            index == MAKER && matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
        let expected_expiry = if synchronized { u64::MAX } else { 0 };
        if matcher.enabled() != u64::from(synchronized) || expiry != expected_expiry {
            return Err(format!(
                "INV-047 {route:?} portfolio {index} matcher postcondition: enabled={}, expiry={expiry}, expected={}/{expected_expiry}",
                matcher.enabled(), u64::from(synchronized)
            ));
        }
    }
    let taker_position = taker
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 0)
        .map(|leg| leg.basis_pos_q)
        .ok_or_else(|| format!("INV-047 {route:?} did not create the taker leg"))?;
    let maker_position = maker
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 0)
        .map(|leg| leg.basis_pos_q)
        .ok_or_else(|| format!("INV-047 {route:?} did not create the maker leg"))?;
    if taker_position != size_q || maker_position != -size_q {
        return Err(format!(
            "INV-047 {route:?} position mismatch: {taker_position}/{maker_position}, expected {size_q}/{}",
            -size_q
        ));
    }

    let contexts = env.all_matcher_context_data();
    for (index, (before, after)) in initial_contexts.iter().zip(&contexts).enumerate() {
        if index != MAKER && after != before {
            return Err(format!(
                "INV-047 {route:?} mutated unrelated matcher context {index}"
            ));
        }
        if index == MAKER
            && (after.len() < MATCHER_RETURN_CACHE_LEN
                || after[MATCHER_RETURN_CACHE_LEN..] != before[MATCHER_RETURN_CACHE_LEN..])
        {
            return Err(format!(
                "INV-047 {route:?} mutated matcher state outside the ABI return cache"
            ));
        }
    }

    normalized_trade_route_frame(&env)
}

fn verify_nonzero_fee_route_equivalence(
    seed: [u8; 32],
    lots: u8,
    account_a_long: bool,
    fee_bps: u16,
) -> Result<(), String> {
    let mut expected = None;
    for route in ROUTES {
        let actual = run_nonzero_fee_route(seed, route, lots, account_a_long, fee_bps)?;
        if let Some((expected_route, expected_frame)) = &expected {
            if actual != *expected_frame {
                return Err(format!(
                    "INV-047 {route:?} diverged from {expected_route:?} for lots={lots}, account_a_long={account_a_long}, fee_bps={fee_bps}"
                ));
            }
        } else {
            expected = Some((route, actual));
        }
    }
    Ok(())
}

#[test]
fn v16_program_nonzero_fee_trade_routes_are_byte_exact_after_transport_normalization() {
    for (case, lots, account_a_long, fee_bps) in [
        (0x47, 1, true, 1),
        (0x48, 7, false, 333),
        (0x49, 8, true, 10_000),
    ] {
        verify_nonzero_fee_route_equivalence([case; 32], lots, account_a_long, fee_bps)
            .unwrap_or_else(|error| panic!("fixed INV-047 route matrix failed: {error}"));
    }
}

#[test]
fn v16_program_fractional_close_partitions_preserve_double_ceil_fees_and_owner_payouts() {
    use crate::support::fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census,
    };

    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 3_334;
    const DEPOSITS: [u128; 5] = [101, 107, 1, 1, 1];
    let part_q = POS_SCALE / 50 + 1;
    let total_q = 2 * part_q;
    let fee = |q: u128| {
        (q * u128::from(PRICE))
            .div_ceil(POS_SCALE)
            .checked_mul(u128::from(FEE_BPS))
            .unwrap()
            .div_ceil(10_000)
    };
    // Both notionals lie just above two atoms. Rounding notional before fees
    // makes the split cost four atoms per owner, versus two for one exact close.
    assert_ne!(part_q * u128::from(PRICE) % POS_SCALE, 0);
    assert_eq!((fee(total_q), 2 * fee(part_q)), (2, 4));
    let total_deposit: u128 = DEPOSITS.iter().sum();
    let mut peak_cu = 0;

    for direction in [-1i128, 1] {
        let mut common_initial = None;
        for parts in [vec![total_q], vec![part_q, part_q]] {
            let expected_fee: u128 = parts.iter().map(|&q| fee(q)).sum();
            assert_eq!(expected_fee - fee(total_q), 2 * (parts.len() as u128 - 1));
            let mut common_prefixes = None;
            for route in ROUTES {
                let label = format!("direction={direction}, parts={parts:?}, route={route:?}");
                let mut env = V16Svm::new(
                    [0x5c; 32],
                    MarketConfig {
                        initial_price: PRICE,
                        actor_deposits: DEPOSITS,
                        ..MarketConfig::default()
                    },
                );
                env.begin_public_trace();
                env.trade_no_cpi(TAKER, MAKER, 0, direction * total_q as i128, PRICE, 0)
                    .unwrap();
                env.update_trade_fee_policy(FEE_BPS).unwrap();
                env.set_matcher_config_with_trade_fee_cap(MAKER, 1, FEE_BPS as u16)
                    .unwrap();
                let initial = normalized_trade_route_frame(&env).unwrap();
                if let Some(expected) = &common_initial {
                    assert_eq!(
                        &initial, expected,
                        "identical public starting state: {label}"
                    );
                } else {
                    common_initial = Some(initial);
                }
                let tokens_before = env.all_token_account_data();
                let supply_before = env.token_supply_observed();
                let passive = [2, 3, 4].map(|actor| env.primary_portfolio_data(actor));
                let mut prefixes = Vec::new();
                let mut remaining = total_q;
                let mut charged = 0;
                let mut payouts = [0u128; 2];
                let check = |env: &V16Svm, remaining: u128, charged: u128, payouts: [u128; 2]| {
                    let group = env.primary_market_state().1;
                    for actor in [TAKER, MAKER] {
                        let account = env.primary_portfolio(actor);
                        assert_eq!(
                            account.capital.get(),
                            DEPOSITS[actor] - charged - payouts[actor],
                            "{label}: owner {actor}"
                        );
                        assert_eq!(account.pnl.get(), 0, "{label}: owner {actor}");
                        let legs: Vec<_> = account
                            .legs
                            .iter()
                            .map(|leg| leg.try_to_runtime().unwrap())
                            .filter(|leg| leg.active)
                            .collect();
                        assert_eq!(legs.len(), usize::from(remaining != 0), "{label}");
                        if remaining != 0 {
                            assert_eq!(legs[0].asset_index, 0);
                            assert_eq!(
                                legs[0].basis_pos_q,
                                direction
                                    * if actor == TAKER {
                                        remaining as i128
                                    } else {
                                        -(remaining as i128)
                                    }
                            );
                        }
                        assert_eq!(
                            u128::from(env.token_amount(env.actors[actor].destination_token)),
                            payouts[actor],
                            "{label}: payout {actor}"
                        );
                    }
                    assert_eq!(group.assets[0].oi_eff_long_q, remaining, "{label}");
                    assert_eq!(group.assets[0].oi_eff_short_q, remaining, "{label}");
                    assert_eq!(group.insurance, 2 * charged, "{label}");
                    assert_eq!(
                        &group.insurance_domain_budget[..2],
                        &[charged; 2],
                        "{label}"
                    );
                    assert_eq!(
                        group.c_tot,
                        total_deposit - 2 * charged - payouts.iter().sum::<u128>(),
                        "{label}"
                    );
                    assert_eq!(
                        group.vault,
                        total_deposit - payouts.iter().sum::<u128>(),
                        "{label}"
                    );
                    assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
                    assert_eq!(
                        u128::from(env.token_amount(env.vault)),
                        group.vault,
                        "{label}"
                    );
                    assert_eq!(env.token_supply_observed(), supply_before, "{label}");
                    assert_eq!(
                        [2, 3, 4].map(|actor| env.primary_portfolio_data(actor)),
                        passive,
                        "{label}"
                    );
                    assert_public_stock_census(&label, env).unwrap();
                    assert_public_encumbrance_census(&label, env).unwrap();
                };
                check(&env, remaining, charged, payouts);
                for &q in &parts {
                    let landed = execute_trade_route(
                        &mut env,
                        route,
                        TAKER,
                        MAKER,
                        0,
                        -direction * q as i128,
                        PRICE,
                        FEE_BPS,
                    )
                    .unwrap_or_else(|error| panic!("{label}: {error}"));
                    peak_cu = peak_cu.max(landed.compute_units);
                    remaining -= q;
                    charged += fee(q);
                    check(&env, remaining, charged, payouts);
                    assert_eq!(
                        env.all_token_account_data(),
                        tokens_before,
                        "close custody: {label}"
                    );
                    prefixes.push(normalized_trade_route_frame(&env).unwrap());
                }
                assert_eq!(remaining, 0);
                assert_eq!(charged, expected_fee);
                for actor in [TAKER, MAKER] {
                    let amount = DEPOSITS[actor] - expected_fee;
                    let landed = env.withdraw_primary(actor, amount).unwrap();
                    peak_cu = peak_cu.max(landed.compute_units);
                    payouts[actor] = amount;
                    check(&env, 0, charged, payouts);
                    prefixes.push(normalized_trade_route_frame(&env).unwrap());
                }
                assert_eq!(
                    payouts.map(|amount| amount + expected_fee),
                    [DEPOSITS[0], DEPOSITS[1]]
                );
                if let Some(expected) = &common_prefixes {
                    assert_eq!(&prefixes, expected, "route economics and payouts: {label}");
                } else {
                    common_prefixes = Some(prefixes);
                }
                let trace = env.finish_public_trace();
                trace.validate_public_execution().unwrap();
                assert_eq!(trace.out_of_band_economic_mutations, 0, "{label}");
                assert_eq!(trace.steps.len(), 5 + parts.len(), "{label}");
                assert!(trace.steps.iter().all(|step| step.succeeded), "{label}");
            }
        }
    }
    assert!(peak_cu < TX_CU_LIMIT);
    println!("INV-024/047/052: 16 fractional close worlds, exact owner payouts; peak CU={peak_cu}");
}

#[derive(Clone, Copy, Debug)]
enum RefreshHistory {
    DirectTrade,
    Canonical,
    OmitLiability,
    ReverseCached,
    DuplicateBenignThenRetry,
}

fn hint_route_accounts(env: &V16Svm, normalize_transport: bool) -> Vec<(Pubkey, Account)> {
    let mut keys: Vec<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
    keys.push(env.foreign_actor.signer.pubkey());
    keys.sort_unstable();
    keys.dedup();
    let mut accounts: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key).expect("framed account")))
        .collect();
    if normalize_transport {
        let market = normalized_primary_market(env).expect("normalize route market");
        let portfolios = normalized_primary_portfolios(env).expect("normalize route portfolios");
        let contexts = normalized_matcher_contexts(env).expect("normalize matcher return cache");
        for (key, account) in &mut accounts {
            if *key == env.market {
                account.data.clone_from(&market);
            }
            for (index, actor) in env.actors.iter().enumerate() {
                if *key == actor.portfolio {
                    account.data.clone_from(&portfolios[index]);
                } else if *key == actor.matcher_context {
                    account.data.clone_from(&contexts[index]);
                }
            }
        }
    }
    accounts
}

fn refresh_hint_history(env: &mut V16Svm, history: RefreshHistory, slot: u64) {
    if matches!(history, RefreshHistory::DirectTrade) {
        return;
    }
    let hints = |indices: &[u16]| {
        indices
            .iter()
            .map(|asset_index| CrankObservationHint {
                asset_index: *asset_index,
                oracle_accounts: 0,
            })
            .collect()
    };
    for actor in [TAKER, MAKER] {
        if matches!(history, RefreshHistory::DuplicateBenignThenRetry) {
            let before = hint_route_accounts(env, false);
            let error = env
                .crank(actor, slot, hints(&[2, 2]))
                .expect_err("duplicate benign hints must reject");
            assert!(
                error.contains(&format!(
                    "Custom({})",
                    PercolatorError::InvalidInstruction as u32
                )),
                "unexpected duplicate rejection: {error}"
            );
            assert!(
                hint_route_accounts(env, false) == before,
                "duplicate-hint rejection changed an actual account frame"
            );
        }
        let indices: &[u16] = match history {
            RefreshHistory::OmitLiability => &[2],
            RefreshHistory::ReverseCached => &[2, 1, 0],
            _ => &[0, 1, 2],
        };
        let mut reached_fixed_point = false;
        for _ in 0..=3 {
            let before = hint_route_accounts(env, false);
            if env
                .crank_if_actionable(actor, slot, hints(indices))
                .unwrap_or_else(|error| panic!("{history:?}/actor={actor}: {error}"))
                .is_none()
            {
                assert!(hint_route_accounts(env, false) == before);
                reached_fixed_point = true;
                break;
            }
        }
        assert!(
            reached_fixed_point,
            "bounded two-leg public refresh did not finish"
        );
        assert!(
            assert_current_certificate_matches_snapshot_full_refresh(
                "INV-047/056 public hint refresh",
                &env.market_data(false),
                &env.primary_portfolio_data(actor),
            )
            .expect("hint-refreshed certificate must cover all liabilities"),
            "public hint refresh left a stale certificate"
        );
    }
}

#[test]
fn v16_program_stale_liability_hint_histories_preserve_trade_route_admission() {
    const PRICE: u64 = 100;
    const SLOT: u64 = 2;
    const LOSS: i128 = 50;
    const TOPUP: u128 = 100;
    const FEE_BPS: u64 = 100;
    const NEW_INITIAL_REQ_AND_FEE: i128 = 110;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    let histories = [
        RefreshHistory::Canonical,
        RefreshHistory::DirectTrade,
        RefreshHistory::OmitLiability,
        RefreshHistory::ReverseCached,
        RefreshHistory::DuplicateBenignThenRetry,
    ];
    let (mut worlds, mut successes, mut rejections, mut peak_cu, mut peak_trade_cu) =
        (0, 0, 0, 0, 0);

    for liability_long in [false, true] {
        let direction = if liability_long { 1 } else { -1 };
        let moved_price = if liability_long { PRICE - 5 } else { PRICE + 5 };
        let mut expected: Option<Vec<(Pubkey, Account)>> = None;
        for route in ROUTES {
            let mut route_expected = None;
            for history in histories {
                let label = format!("{route:?}/{history:?}/liability_long={liability_long}");
                let mut seed = [0x56; 32];
                seed[0] = u8::from(liability_long);
                let mut config = MarketConfig {
                    initial_price: PRICE,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    ..MarketConfig::default()
                };
                config.actor_deposits[TAKER] = 220;
                let mut env = V16Svm::new(seed, config);
                env.begin_public_trace();
                env.trade_no_cpi(TAKER, MAKER, 1, direction * SIZE_Q, PRICE, 0)
                    .expect("open future liability through the public route");
                env.trade_no_cpi(TAKER, MAKER, 2, -direction * POS_SCALE as i128, PRICE, 0)
                    .expect("open benign active leg through the public route");
                env.ensure_primary_matcher_enabled(MAKER)
                    .expect("same LP authorization in every world");
                env.update_trade_fee_policy(FEE_BPS)
                    .expect("same disclosed nonzero base fee in every route");
                let cached = env
                    .primary_portfolio(TAKER)
                    .health_cert
                    .try_to_runtime()
                    .unwrap();
                env.warp_to_slot(SLOT);
                for (asset, price) in [(0, PRICE), (1, moved_price), (2, PRICE)] {
                    env.push_auth_mark(asset, SLOT, price)
                        .expect("publish bounded mark");
                }
                env.crank(
                    4,
                    SLOT,
                    [0, 1, 2]
                        .into_iter()
                        .map(|asset_index| CrankObservationHint {
                            asset_index,
                            oracle_accounts: 0,
                        })
                        .collect(),
                )
                .expect("unrelated public observer commits market discovery");
                let group = env.primary_market_state().1;
                assert_eq!(group.assets[1].effective_price, moved_price, "{label}");
                assert!(
                    cached.valid && cached.cert_oracle_epoch < group.oracle_epoch,
                    "{label}"
                );
                assert_eq!(
                    env.primary_portfolio(TAKER)
                        .health_cert
                        .try_to_runtime()
                        .unwrap(),
                    cached,
                    "{label}: observing the market must leave the target certificate stale"
                );
                let fresh = snapshot_engine_full_refresh(
                    &env.market_data(false),
                    &env.primary_portfolio_data(TAKER),
                )
                .expect("read-only full-refresh oracle")
                .0;
                // This candidate passes the old certificate, but not the actual uncranked loss.
                assert_eq!(
                    cached.certified_equity - fresh.certified_equity,
                    LOSS,
                    "{label}"
                );
                assert!(
                    cached.certified_equity - cached.certified_initial_req as i128
                        >= NEW_INITIAL_REQ_AND_FEE,
                    "{label}: omitted-liability negative control is vacuous"
                );
                assert!(
                    fresh.certified_equity - (fresh.certified_initial_req as i128)
                        < NEW_INITIAL_REQ_AND_FEE,
                    "{label}: liability must change candidate admission"
                );
                assert!(fresh.certified_equity > fresh.certified_maintenance_req as i128);

                let before_hints = hint_route_accounts(&env, false);
                refresh_hint_history(&mut env, history, SLOT);
                let before_reject = hint_route_accounts(&env, false);
                let error =
                    execute_trade_route(&mut env, route, TAKER, MAKER, 0, SIZE_Q, PRICE, FEE_BPS)
                        .expect_err("the refreshed liability must prevent favorable new risk");
                assert!(
                    error.contains(&format!(
                        "Custom({})",
                        PercolatorError::EngineInvalidConfig as u32
                    )),
                    "{label}: unexpected admission error: {error}"
                );
                assert!(
                    hint_route_accounts(&env, false) == before_reject,
                    "{label}: rejected trade changed SPL/custody/account frames"
                );

                let source = env.actors[TAKER].source_token;
                let source_before = env.token_amount(source);
                let vault_before = env.token_amount(env.vault);
                env.deposit_primary(TAKER, TOPUP)
                    .expect("public SPL topup makes candidate affordable");
                assert_eq!(
                    env.token_amount(source),
                    source_before - TOPUP as u64,
                    "{label}"
                );
                assert_eq!(
                    env.token_amount(env.vault),
                    vault_before + TOPUP as u64,
                    "{label}"
                );
                let tokens_before_trade = env.all_token_account_data();
                let insurance_before = env.primary_market_state().1.insurance;
                let landed =
                    execute_trade_route(&mut env, route, TAKER, MAKER, 0, SIZE_Q, PRICE, FEE_BPS)
                        .unwrap_or_else(|error| {
                            panic!("{label}: funded positive control failed: {error}")
                        });
                peak_trade_cu = peak_trade_cu.max(landed.compute_units);
                assert_eq!(env.all_token_account_data(), tokens_before_trade, "{label}");
                let after = env.primary_market_state().1;
                assert_eq!(
                    after.insurance - insurance_before,
                    20,
                    "{label}: disclosed fees"
                );
                assert_eq!(
                    u128::from(env.token_amount(env.vault)),
                    after.vault,
                    "{label}"
                );
                assert_eq!(env.primary_portfolio(TAKER).capital.get(), 260, "{label}");
                assert_eq!(env.primary_portfolio(TAKER).pnl.get(), 0, "{label}");
                assert_eq!(env.primary_portfolio(MAKER).pnl.get(), LOSS, "{label}");
                assert_eq!(
                    after.vault,
                    after.c_tot + after.insurance + LOSS as u128,
                    "{label}: custody also backs the counterparty's unconverted claim"
                );
                assert_eq!(
                    env.token_supply_observed(),
                    env.initial_token_supply,
                    "{label}"
                );
                for (actor, sign) in [(TAKER, 1), (MAKER, -1)] {
                    let account = env.primary_portfolio(actor);
                    let positions: Vec<_> = account
                        .legs
                        .iter()
                        .map(|leg| leg.try_to_runtime().unwrap())
                        .filter(|leg| leg.active)
                        .map(|leg| (leg.asset_index, leg.basis_pos_q))
                        .collect();
                    assert_eq!(positions.len(), 3, "{label}: active leg omitted");
                    for position in [
                        (0, sign * SIZE_Q),
                        (1, sign * direction * SIZE_Q),
                        (2, -sign * direction * POS_SCALE as i128),
                    ] {
                        assert!(
                            positions.contains(&position),
                            "{label}: missing {position:?}"
                        );
                    }
                    assert!(assert_current_certificate_matches_snapshot_full_refresh(
                        &label,
                        &env.market_data(false),
                        &env.primary_portfolio_data(actor),
                    )
                    .expect("committed route certificate must match full refresh"));
                }
                let maker_data = env.primary_portfolio_data(MAKER);
                let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                assert_eq!(
                    state::read_portfolio_matcher_config(&maker_data)
                        .unwrap()
                        .enabled(),
                    u64::from(cpi),
                    "{label}: route-specific matcher enabled postcondition"
                );
                assert_eq!(
                    state::read_portfolio_matcher_expiry(&maker_data).unwrap(),
                    if cpi { u64::MAX } else { 0 },
                    "{label}: route-specific matcher expiry postcondition"
                );
                assert_eq!(
                    env.primary_market_state().0.matcher_req_seq,
                    u64::from(cpi),
                    "{label}: only the successful CPI fill consumes a matcher request"
                );
                let changed_accounts = [
                    env.market,
                    env.actors[TAKER].portfolio,
                    env.actors[MAKER].portfolio,
                    env.actors[MAKER].matcher_context,
                    source,
                    env.vault,
                ];
                for (key, before) in &before_hints {
                    if !changed_accounts.contains(key) {
                        assert!(
                            env.svm.get_account(key).as_ref() == Some(before),
                            "{label}: unrelated account {key} changed"
                        );
                    }
                }
                let raw = hint_route_accounts(&env, false);
                if let Some(canonical) = &route_expected {
                    assert!(
                        &raw == canonical,
                        "{label}: hint history changed the unnormalized same-route account frame"
                    );
                } else {
                    route_expected = Some(raw);
                }
                let actual = hint_route_accounts(&env, true);
                if let Some(canonical) = &expected {
                    assert!(
                        &actual == canonical,
                        "{label}: final account frame differs from canonical route"
                    );
                } else {
                    expected = Some(actual);
                }
                let trace = env.finish_public_trace();
                trace
                    .validate_public_execution()
                    .expect("entire history must be public with exact rollback");
                assert_eq!(trace.out_of_band_economic_mutations, 0);
                for step in &trace.steps {
                    if let Some(cu) = step.compute_units {
                        successes += 1;
                        peak_cu = peak_cu.max(cu);
                    } else {
                        rejections += 1;
                    }
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 40);
    assert_eq!(successes, 464);
    assert_eq!(rejections, 120);
    assert!(peak_cu < TX_CU_LIMIT);
    println!(
        "INV-047/056 hint-history product: worlds={worlds}, successful_public_txs={successes}, exact_rejections={rejections}, peak_cu={peak_cu}, peak_trade_cu={peak_trade_cu}"
    );
}

// INVARIANTS.md: INV-047 Equivalent-route semantics; INV-053 Full-health recertification
// equivalence; INV-054 Certificate epoch completeness; INV-057 Risk-reduction availability.
// Non-redundancy: the three-asset hint and structural-delta matrices cannot reach the wrapper's
// eight-cached-leg currentness gate. This public I/M/R/C matrix crosses seven/eight active legs,
// either/both stale participants, both signs, and reductions of 1, Q-1, Q and Q+1 (cross-zero).
// All worlds allocate eight assets; the last active, nontraded leg retains adverse target lag.
// Oracle: independent raw-state health, cloned full refresh, raw positions/OI and exact Account
// frames. Transport normalization is the same checked normalization used above, never SVM writes.
// Exact selector: cargo test --offline --locked --features test-sbf --test v16_program_stateful_fuzz
// inv_047_equivalent_route_semantics::v16_program_eight_asset_stale_routes_refresh_and_reduce
// -- --exact --nocapture
// Program source: 9c67c1e1b49a8ce24f4b6adbf61736de2dc68094; engine: 4db11a8c.
// SBF: default features, platform-tools v1.52; artifact hash and peak CU printed by this test.
// Scope: healthy Live/AuthMark, unit ADL, zero funding/fees/liens/pending obligations, fixed slot
// and authorized bilateral counterparties. Other epoch writers, expiry, 9..14 legs, multi-fill
// batches and drain/reset/recovery/resolved or counterparty-free exits remain separate evidence.
#[test]
fn v16_program_eight_asset_stale_routes_refresh_and_reduce() {
    const PRICE: u64 = 100;
    const SLOT: u64 = 2;
    const CAPITAL: u128 = 2_000;
    const Q: i128 = POS_SCALE as i128;
    const REFRESH_BOUND: usize = 3;

    let current = |env: &V16Svm, actor, label: &str| {
        assert_current_certificate_matches_snapshot_full_refresh(
            label,
            &env.market_data(false),
            &env.primary_portfolio_data(actor),
        )
        .unwrap_or_else(|error| panic!("{label}: {error}"))
    };
    let raw_health = |env: &V16Svm, actor, label: &str| {
        let group = env.primary_market_state().1;
        let account = env.primary_portfolio(actor);
        let independent = independent_health_certificate(label, &group, &account).unwrap();
        let full = snapshot_engine_full_refresh(
            &env.market_data(false),
            &env.primary_portfolio_data(actor),
        )
        .unwrap()
        .0;
        assert_eq!(full, independent, "{label}: independent raw-state health");
        assert_eq!(independent.certified_equity, CAPITAL as i128, "{label}");
        assert_eq!(independent.certified_liq_deficit, 0, "{label}");
        independent
    };
    let hints = |count: u16| {
        (0..count)
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: 0,
            })
            .collect::<Vec<_>>()
    };
    let refresh = |env: &mut V16Svm, actor, count, label: &str| {
        let mut calls = 0;
        while !current(env, actor, label) && calls < REFRESH_BOUND {
            env.crank(actor, SLOT, hints(count))
                .unwrap_or_else(|error| panic!("{label}: public refresh {calls}: {error}"));
            calls += 1;
        }
        assert!(
            current(env, actor, label),
            "{label}: refresh bound exhausted"
        );
        let raw = raw_health(env, actor, label);
        assert_eq!(
            env.primary_portfolio(actor)
                .health_cert
                .try_to_runtime()
                .unwrap(),
            raw,
            "{label}: public certificate must include every active leg"
        );
        calls
    };

    let (mut worlds, mut stale_rejections, mut inline_fills, mut reducing_fills) = (0, 0, 0, 0);
    let (mut peak_cu, mut max_refresh_calls) = (0, 0);
    let mut program_hash = None;
    for count in [7u16, 8] {
        for sign in [-1i128, 1] {
            for stale_mask in [1u8, 2, 3] {
                for reduction in [1, Q - 1, Q, Q + 1] {
                    let mut expected_before = None;
                    let mut expected_after = None;
                    for route in ROUTES {
                        let label = format!(
                            "legs={count}/sign={sign}/stale={stale_mask}/reduce={reduction}/{route:?}"
                        );
                        let mut config = MarketConfig {
                            initial_price: PRICE,
                            // At price 100 the one-bps movement rounds to zero. Raw target lag
                            // changes health without K/F settlement or a loss-stale ADL gate.
                            max_price_move_bps_per_slot: 1,
                            max_accrual_dt_slots: 1,
                            min_funding_lifetime_slots: 1,
                            ..MarketConfig::default()
                        };
                        config.actor_deposits[TAKER] = CAPITAL;
                        config.actor_deposits[MAKER] = CAPITAL;
                        let mut env = V16Svm::new_with_asset_count([0x57; 32], config, 8);
                        if let Some(hash) = program_hash {
                            assert_eq!(env.loaded_program_hash, hash);
                        } else {
                            program_hash = Some(env.loaded_program_hash);
                        }
                        env.begin_public_trace();
                        assert_eq!(env.primary_market_state().1.assets.len(), 8);
                        for asset in 0..count {
                            env.trade_no_cpi(TAKER, MAKER, asset, sign * Q, PRICE, 0)
                                .unwrap_or_else(|error| {
                                    panic!("{label}: open asset {asset}: {error}")
                                });
                        }
                        env.ensure_primary_matcher_enabled(MAKER).unwrap();
                        let cached = [TAKER, MAKER].map(|actor| {
                            assert!(current(&env, actor, &label));
                            let cert = raw_health(&env, actor, &label);
                            assert_eq!(cert.certified_initial_req, u128::from(count) * 100);
                            assert_eq!(
                                cert.active_bitmap_at_cert
                                    .iter()
                                    .map(|word| word.count_ones())
                                    .sum::<u32>(),
                                u32::from(count)
                            );
                            cert
                        });
                        env.warp_to_slot(SLOT);
                        for asset in 0..count {
                            let price = if asset == count - 1 {
                                (i128::from(PRICE) - sign) as u64
                            } else {
                                PRICE
                            };
                            env.push_auth_mark(asset, SLOT, price).unwrap();
                        }
                        env.crank(4, SLOT, hints(count)).unwrap();
                        let group = env.primary_market_state().1;
                        assert_eq!(group.assets[usize::from(count - 1)].effective_price, PRICE);
                        assert_eq!(
                            group.assets[usize::from(count - 1)].raw_oracle_target_price,
                            (i128::from(PRICE) - sign) as u64
                        );
                        for actor in [TAKER, MAKER] {
                            assert_eq!(
                                env.primary_portfolio(actor)
                                    .health_cert
                                    .try_to_runtime()
                                    .unwrap(),
                                cached[actor],
                                "{label}: observer must leave the account cache untouched"
                            );
                            assert!(
                                cached[actor].cert_oracle_epoch < group.oracle_epoch,
                                "{label}"
                            );
                            assert!(!current(&env, actor, &label));
                            let fresh = raw_health(&env, actor, &label);
                            let penalty = u128::from(actor == TAKER);
                            assert_eq!(
                                fresh.certified_initial_req,
                                cached[actor].certified_initial_req + penalty
                            );
                            assert_eq!(
                                fresh.certified_maintenance_req,
                                cached[actor].certified_maintenance_req + penalty
                            );
                            assert_eq!(
                                fresh.certified_worst_case_loss,
                                cached[actor].certified_worst_case_loss + penalty
                            );
                            if stale_mask & (1 << actor) == 0 {
                                refresh(&mut env, actor, count, &label);
                            }
                        }
                        for actor in [TAKER, MAKER] {
                            assert_eq!(
                                current(&env, actor, &label),
                                stale_mask & (1 << actor) == 0
                            );
                        }
                        let before = hint_route_accounts(&env, false);
                        if let Some(expected) = &expected_before {
                            assert!(
                                &before == expected,
                                "{label}: routes need identical raw prestates"
                            );
                        } else {
                            expected_before = Some(before.clone());
                        }
                        let tokens = env.all_token_account_data();
                        let attempt = execute_trade_route(
                            &mut env,
                            route,
                            TAKER,
                            MAKER,
                            0,
                            -sign * reduction,
                            PRICE,
                            0,
                        );
                        if count == 8 {
                            let error =
                                attempt.expect_err("eight-leg stale account must require refresh");
                            assert!(
                                error.contains(&format!(
                                    "Custom({})",
                                    PercolatorError::EngineStale as u32
                                )),
                                "{label}: wrong stale error: {error}"
                            );
                            assert!(
                                hint_route_accounts(&env, false) == before,
                                "{label}: exact stale rollback"
                            );
                            stale_rejections += 1;
                            let calls: usize = [TAKER, MAKER]
                                .into_iter()
                                .map(|actor| refresh(&mut env, actor, count, &label))
                                .sum();
                            assert!(calls > 0 && calls <= 2 * REFRESH_BOUND, "{label}");
                            max_refresh_calls = max_refresh_calls.max(calls);
                            execute_trade_route(
                                &mut env,
                                route,
                                TAKER,
                                MAKER,
                                0,
                                -sign * reduction,
                                PRICE,
                                0,
                            )
                            .unwrap_or_else(|error| {
                                panic!("{label}: refreshed retry failed: {error}")
                            });
                        } else {
                            attempt.unwrap_or_else(|error| {
                                panic!("{label}: seven-leg inline refresh: {error}")
                            });
                            inline_fills += 1;
                        }
                        let after_group = env.primary_market_state().1;
                        for (actor, actor_sign) in [(TAKER, sign), (MAKER, -sign)] {
                            assert!(current(&env, actor, &label));
                            let cert = raw_health(&env, actor, &label);
                            let account = env.primary_portfolio(actor);
                            assert_eq!(
                                account.health_cert.try_to_runtime().unwrap(),
                                cert,
                                "{label}"
                            );
                            let mut positions = std::collections::BTreeMap::new();
                            for encoded in account.legs {
                                let leg = encoded.try_to_runtime().unwrap();
                                if leg.active {
                                    assert!(positions
                                        .insert(leg.asset_index, leg.basis_pos_q)
                                        .is_none());
                                }
                            }
                            assert_eq!(
                                positions.len(),
                                usize::from(count) - usize::from(reduction == Q)
                            );
                            for asset in 0..u32::from(count) {
                                let q = if asset == 0 {
                                    actor_sign * (Q - reduction)
                                } else {
                                    actor_sign * Q
                                };
                                assert_eq!(
                                    positions.get(&asset).copied().unwrap_or(0),
                                    q,
                                    "{label}: actor={actor}/asset={asset}"
                                );
                                assert_eq!(
                                    after_group.assets[asset as usize].oi_eff_long_q,
                                    q.unsigned_abs()
                                );
                                assert_eq!(
                                    after_group.assets[asset as usize].oi_eff_short_q,
                                    q.unsigned_abs()
                                );
                            }
                            assert!((Q - reduction).unsigned_abs() < Q as u128);
                        }
                        reducing_fills += 1;
                        assert_eq!(env.all_token_account_data(), tokens, "{label}: SPL frame");
                        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                        assert_eq!(u128::from(env.token_amount(env.vault)), after_group.vault);
                        assert_eq!(after_group.vault, after_group.c_tot + after_group.insurance);
                        for (key, account) in before {
                            if ![
                                env.market,
                                env.actors[TAKER].portfolio,
                                env.actors[MAKER].portfolio,
                                env.actors[MAKER].matcher_context,
                            ]
                            .contains(&key)
                            {
                                assert_eq!(
                                    env.svm.get_account(&key).unwrap(),
                                    account,
                                    "{label}: unrelated {key}"
                                );
                            }
                        }
                        let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                        let maker = env.primary_portfolio_data(MAKER);
                        assert_eq!(
                            state::read_portfolio_matcher_config(&maker)
                                .unwrap()
                                .enabled(),
                            u64::from(cpi)
                        );
                        assert_eq!(
                            state::read_portfolio_matcher_expiry(&maker).unwrap(),
                            if cpi { u64::MAX } else { 0 }
                        );
                        assert_eq!(env.primary_market_state().0.matcher_req_seq, u64::from(cpi));
                        let normalized = hint_route_accounts(&env, true);
                        if let Some(expected) = &expected_after {
                            assert!(
                                &normalized == expected,
                                "{label}: equivalent route poststate"
                            );
                        } else {
                            expected_after = Some(normalized);
                        }
                        let trace = env.finish_public_trace();
                        trace.validate_public_execution().unwrap();
                        assert_eq!(trace.out_of_band_economic_mutations, 0, "{label}");
                        assert_eq!(
                            trace.steps.iter().filter(|step| !step.succeeded).count(),
                            usize::from(count == 8)
                        );
                        peak_cu = peak_cu.max(
                            trace
                                .steps
                                .iter()
                                .filter_map(|step| step.compute_units)
                                .max()
                                .unwrap(),
                        );
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(
        (worlds, stale_rejections, inline_fills, reducing_fills),
        (192, 96, 96, 192)
    );
    assert!(peak_cu < TX_CU_LIMIT);
    println!("INV-047/053/054/057: {worlds} worlds, {stale_rejections} exact stale rollbacks, {inline_fills} inline refreshes, {reducing_fills} reducing fills; max refresh calls={max_refresh_calls}, peak CU={peak_cu}, SBF SHA256={}", program_hash.unwrap());
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 32) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_047_nonzero_fee_route_equivalence.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_generated_nonzero_fee_trade_routes_are_economically_equivalent(
        seed in any::<[u8; 32]>(),
        lots in 1u8..=8,
        account_a_long in any::<bool>(),
        fee_bps in 1u16..=10_000,
    ) {
        prop_assert!(
            verify_nonzero_fee_route_equivalence(seed, lots, account_a_long, fee_bps).is_ok(),
            "generated INV-047 route matrix diverged"
        );
    }
}
