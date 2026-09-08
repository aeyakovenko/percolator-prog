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
//! lamports, and token supply after normalizing three documented transport/capability differences:
//! the CPI-only matcher request sequence, the single-CPI matcher's 64-byte ABI return cache, and
//! the LP matcher-enabled bit retained after a matcher-synchronized fill but revoked by a bilateral
//! fill. Matcher tuple, fee cap, position epoch, and every other byte remain exact. The fixed matrix
//! covers minimum, interior, and maximum fee rates; the generated matrix varies seed, size, side,
//! and fee rate.
//!
//! The bounded stale-liability product composes all four routes with direct admission, canonical
//! public refresh, omitted-liability hints, reversed cached hints, and duplicate-benign-hint
//! rollback/retry. A stale certificate would admit the candidate, but full refresh includes a
//! 50-atom loss and must reject it. A public SPL deposit then makes the same trade usable. The
//! committed account frames converge after the transport normalization above plus the LP expiry
//! cleared with its enabled bit by bilateral fills. Both expiry postconditions are checked before
//! normalization. Certificates are checked with the existing full-refresh oracle. This supplies
//! INV-056 composition evidence without repeating its max-shape omission matrix.
//!
//! Guarantee boundary: this closes the one-leg trade-route and nonzero-fee partition. It does not
//! establish wrapper/engine transition equivalence for every public instruction or equivalence
//! between unrelated direct and composite lifecycle operations.

use super::env_usize;
use crate::support::{
    fuzz_model::{
        assert_current_certificate_matches_snapshot_full_refresh, execute_trade_route, TradeRoute,
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
    if group.insurance <= insurance_before {
        return Err(format!(
            "INV-047 {route:?} nonzero fee did not fund insurance: {}/{}",
            group.insurance, insurance_before
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
        let mut portfolios =
            normalized_primary_portfolios(env).expect("normalize route portfolios");
        portfolios[MAKER][PORTFOLIO_MATCHER_EXPIRY_OFF
            ..PORTFOLIO_MATCHER_EXPIRY_OFF + PORTFOLIO_MATCHER_EXPIRY_LEN]
            .fill(0);
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
