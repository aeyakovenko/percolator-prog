//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_backing_expiry_boundary_rejects_stale_fee_and_preserves_exit` constructs a retained
//! trade while backing is fresh and lands it after authenticated Clock expiry. The unsafe increase
//! must return `EngineStale` with exact rollback, zero provider fee, and no victim loss; a reducing
//! trade must remain executable.
//! `v16_program_backing_expiry_trade_route_boundary_matrix` repeats the freshness check through all
//! four public trade routes at `expiry-1`, `expiry`, and `expiry+1`. Every pre-expiry control must
//! grow a real counterparty-backed lien; single routes must also charge and extract a real provider
//! fee. Both expired boundaries reject atomically and preserve a risk-reducing trade.
//! `v16_program_retained_backing_topup_boundary_matrix` generates signed retained top-ups at all
//! three expiry boundaries and compares omitted and submitted operations. A fresh request debits
//! provider SPL and credits canonical custody/accounting exactly, then remains boundedly
//! settleable after the backing lapses. Expired requests roll back every delta and preserve
//! terminal user progress. The immediately preceding TDD commit reproduces PR291's terminal lock
//! with this same matrix on the pre-fix engine pin.
//! `v16_program_backing_expiry_conversion_boundary_matrix` generates released source-backed claims
//! at all three expiry boundaries. The pre-expiry control must consume backing, credit capital, and
//! withdraw real SPL value; both expired boundaries reject with exact rollback and zero
//! provider-principal movement while preserving withdrawal of all senior capital.
//! `v16_program_backing_principal_release_respects_authenticated_expiry` retains a provider
//! withdrawal while the bucket is fresh and lands it at all three authenticated boundaries. Only
//! the pre-expiry request may recover principal; equal/late requests must roll back rather than
//! bypass expiry forfeiture.
//! `v16_program_resolved_close_normalizes_backing_at_expiry` creates the source-backed claim through
//! public trades, resolves at all three boundaries, and permutes claimant plus terminal-route
//! order. Every rejected step rolls back exactly, every successful payout reconciles against both
//! engine and SPL custody, and equal/late resolution removes lapsed backing from the fresh-credit
//! classes before terminal disposition.
//! `v16_program_post_snapshot_expiry_topup_is_public_and_order_independent` then captures a genuinely
//! partial payout receipt before a second source domain expires. It advances authenticated Clock
//! through both terminal routes, requires a value-moving payout top-up, and proves that exact/late
//! expiry removes the lapsed backing without changing claimant-order or route-order economics.
//! `v16_program_expiry_refill_failure_histories_preserve_claim_and_senior_exit` crosses two
//! successive expiry/refill cycles with aggregate/split refills and failed refill-plus-conversion
//! transactions. Failed suffixes must restore the earlier token transfer, backing classification,
//! intent sequence and claim; only the last fresh tranche may fund the eventual owner payout.
//! `v16_program_expiry_refill_source_sides_and_ratios_preserve_attribution` reuses that history
//! with both source sides and under/exact/over-backed final refills. Account-local claim ownership,
//! unused-domain counters, history-derived rates and unspent surplus remain exact through expiry,
//! rejected refill bundles, fresh conversion and all three owner payouts.
//! The sibling INV-031 test `v16_program_shared_lien_expiry_refill_preserves_owner_attribution`
//! covers exact/late expiry while two account-local liens are live, ordered owner release, rejected
//! replacement backing while impairment remains, and aggregate/split refill after both liens clear.
//!
//! Guarantee boundary: the trade, conversion, and retained-top-up consumers have fixed-pin bounded
//! evidence over the generated route and expiry boundaries represented here.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census,
        assert_source_credit_rate_transition, assert_source_credit_rates, execute_trade_route,
    },
    v16_svm::{MarketConfig, TxSuccess, V16Svm, TX_CU_LIMIT},
};
use percolator::{
    active_bitmap_is_empty, BackingBucketStatusV16, MarketModeV16, BOUND_SCALE, CREDIT_RATE_SCALE,
    POS_SCALE,
};
use percolator_prog::ix::{CrankObservationHint, Instruction as ProgInstruction};
use percolator_prog::state;
use solana_sdk::{
    instruction::{AccountMeta, Instruction, InstructionError},
    transaction::{Transaction, TransactionError},
};

#[path = "inv_063_retained_reserve_stock.rs"]
mod retained_reserve_stock;

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    markets: [Vec<u8>; 2],
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    matcher_contexts: Vec<Vec<u8>>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        markets: [env.market_data(false), env.market_data(true)],
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        matcher_contexts: env.all_matcher_context_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn refill_history_step(
    env: &mut V16Svm,
    label: &str,
    action: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) -> Result<TxSuccess, String> {
    let before = snapshot(env);
    let before_group = env.primary_market_state().1;
    let result = action(env);
    if result.is_err() {
        assert_eq!(
            snapshot(env),
            before,
            "{label}: exact failed transaction frame"
        );
    } else {
        assert_ne!(
            snapshot(env),
            before,
            "{label}: successful step must mutate"
        );
    }
    let group = env.primary_market_state().1;
    assert_public_stock_census(label, env).expect("refill history stock census");
    assert_public_encumbrance_census(label, env).expect("refill history encumbrance census");
    assert_source_credit_rates(label, &group).expect("refill history rate oracle");
    assert_source_credit_rate_transition(label, &before_group, &group)
        .expect("refill history rate transition oracle");
    result
}

fn refresh_refill_history_claimant(
    env: &mut V16Svm,
    slot: u64,
    label: &str,
    check: impl Fn(&V16Svm),
) {
    for _ in 0..8 {
        let group = env.primary_market_state().1;
        let account = env.primary_portfolio(0);
        let cert = account
            .health_cert
            .try_to_runtime()
            .expect("claimant certificate");
        if cert.valid
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == account.active_bitmap.map(|word| word.get())
        {
            return;
        }
        refill_history_step(env, label, |env| {
            env.crank(
                0,
                slot,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: env.primary_profile(0).oracle_leg_count,
                }],
            )
        })
        .expect("bounded claimant recertification");
        check(env);
    }
    panic!("{label}: claimant certificate did not become current");
}

fn run_expiry_refill_failure_history(
    route: TradeRoute,
    late: bool,
    split: bool,
    failed_attempts: usize,
    winner_long: bool,
    final_backing: u128,
) -> ([u64; 3], u128, usize) {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const SENIOR: usize = 2;
    const PRICE: u64 = 100;
    const PRICE_MOVE: u64 = 5;
    const SIZE: i128 = 20 * POS_SCALE as i128;
    const CLAIM: u128 = PRICE_MOVE as u128 * SIZE as u128 / POS_SCALE;
    const INITIAL_BACKING: u128 = 150;
    const DEPOSITS: [u128; 3] = [1_000, 1_000, 777];

    let domain = if winner_long { 1 } else { 0 };
    let mark = if winner_long {
        PRICE + PRICE_MOVE
    } else {
        PRICE - PRICE_MOVE
    };
    let size = if winner_long { SIZE } else { -SIZE };
    let conversion_atoms = CLAIM.min(final_backing);
    let surplus = final_backing - conversion_atoms;
    let label = format!(
        "INV-063 {route:?} late={late} split={split} failures={failed_attempts} \
         winner_long={winner_long} final_backing={final_backing}"
    );
    let mut env = V16Svm::new(
        [0x6a; 32],
        MarketConfig {
            initial_price: PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 0,
            actor_deposits: [DEPOSITS[0], DEPOSITS[1], DEPOSITS[2], 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply = env.token_supply_observed();
    let provider_before = env.token_amount(env.provider_source_token);
    let senior_before = env.primary_portfolio_data(SENIOR);
    let destinations: [u64; 3] =
        std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));
    env.begin_public_trace();
    refill_history_step(&mut env, &label, |env| {
        env.top_up_backing_bucket(domain as u16, INITIAL_BACKING, 5)
    })
    .expect("initial expiring backing");
    let prepare_matcher = |env: &mut V16Svm| {
        let data = env.primary_portfolio_data(LOSER);
        let config = state::read_portfolio_matcher_config(&data).expect("matcher capability");
        if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) && config.enabled() == 0 {
            refill_history_step(env, &label, |env| {
                env.set_matcher_config_with_trade_fee_cap(LOSER, 1, config.trade_fee_cap_bps())
            })
            .expect("explicitly checked matcher refresh");
        }
    };
    prepare_matcher(&mut env);
    refill_history_step(&mut env, &label, |env| {
        execute_trade_route(env, route, WINNER, LOSER, 0, size, PRICE, 0)
    })
    .expect("open claim-producing exposure");
    env.warp_to_slot(2);
    refill_history_step(&mut env, &label, |env| env.push_auth_mark(0, 2, mark))
        .expect("authenticate winning mark");
    for actor in [LOSER, WINNER] {
        refill_history_step(&mut env, &label, |env| {
            env.crank(
                actor,
                2,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: env.primary_profile(0).oracle_leg_count,
                }],
            )
        })
        .expect("settle the original cohort");
    }
    prepare_matcher(&mut env);
    refill_history_step(&mut env, &label, |env| {
        execute_trade_route(env, route, WINNER, LOSER, 0, -size, mark, 0)
    })
    .expect("release the claimant's exposure");

    let check =
        |env: &V16Svm, fresh: u128, provider_debit: u128, converted: bool, paid: [u64; 3]| {
            let group = env.primary_market_state().1;
            let source = group.source_credit[domain];
            let bucket = group.source_backing_buckets[domain];
            let converted_atoms = if converted { conversion_atoms } else { 0 };
            let face = if converted { 0 } else { CLAIM };
            let rate = if face == 0 {
                CREDIT_RATE_SCALE
            } else {
                fresh.min(face) * CREDIT_RATE_SCALE / face
            };
            assert_eq!(
                source.credit_rate_num, rate,
                "{label}: history-derived rate"
            );
            assert_eq!(
                source.positive_claim_bound_num,
                face * BOUND_SCALE,
                "{label}"
            );
            assert_eq!(
                source.fresh_reserved_backing_num,
                fresh * BOUND_SCALE,
                "{label}"
            );
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                fresh * BOUND_SCALE,
                "{label}"
            );
            assert_eq!(
                source.spent_backing_num,
                converted_atoms * BOUND_SCALE,
                "{label}"
            );
            assert_eq!(
                bucket.consumed_liened_backing_num,
                converted_atoms * BOUND_SCALE,
                "{label}"
            );
            assert_eq!(source.valid_liened_backing_num, 0, "{label}");
            assert_eq!(source.impaired_liened_backing_num, 0, "{label}");
            for (other_domain, other_source) in group.source_credit.iter().enumerate() {
                if other_domain != domain {
                    let other_bucket = group.source_backing_buckets[other_domain];
                    assert_eq!(
                        [
                            other_source.positive_claim_bound_num,
                            other_source.fresh_reserved_backing_num,
                            other_source.spent_backing_num,
                            other_source.valid_liened_backing_num,
                            other_source.impaired_liened_backing_num,
                            other_bucket.fresh_unliened_backing_num,
                            other_bucket.consumed_liened_backing_num,
                        ],
                        [0; 7],
                        "{label}: unrelated source domain {other_domain}"
                    );
                }
            }
            let capitals = [
                DEPOSITS[0] + converted_atoms,
                DEPOSITS[1] - CLAIM,
                DEPOSITS[2],
            ];
            for actor in [WINNER, LOSER, SENIOR] {
                let portfolio = env.primary_portfolio(actor);
                let mut account_claim = 0;
                for account_source in portfolio.source_domains.iter().filter(|s| s.is_occupied()) {
                    let attributed = account_source.source_claim_bound_num.get();
                    if attributed != 0 {
                        assert_eq!(account_source.domain.get() as usize, domain, "{label}");
                    }
                    account_claim += attributed;
                    assert_eq!(account_source.source_claim_liened_num.get(), 0, "{label}");
                    assert_eq!(account_source.source_claim_impaired_num.get(), 0, "{label}");
                    assert_eq!(
                        account_source.source_lien_counterparty_backing_num.get(),
                        0,
                        "{label}"
                    );
                }
                assert_eq!(
                    account_claim,
                    if actor == WINNER {
                        face * BOUND_SCALE
                    } else {
                        0
                    },
                    "{label}: actor {actor} claim attribution"
                );
                assert_eq!(
                    portfolio.capital.get(),
                    capitals[actor] - u128::from(paid[actor]),
                    "{label}"
                );
                assert_eq!(
                    portfolio.pnl.get(),
                    if actor == WINNER { face as i128 } else { 0 },
                    "{label}"
                );
                assert_eq!(
                    env.token_amount(env.actors[actor].destination_token) - destinations[actor],
                    paid[actor],
                    "{label}"
                );
            }
            let payouts = paid.iter().map(|amount| u128::from(*amount)).sum::<u128>();
            assert_eq!(
                group.c_tot,
                capitals.iter().sum::<u128>() - payouts,
                "{label}"
            );
            assert_eq!(
                group.vault,
                DEPOSITS.iter().sum::<u128>() + provider_debit - payouts,
                "{label}"
            );
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                group.vault,
                "{label}"
            );
            assert_eq!(
                u128::from(provider_before - env.token_amount(env.provider_source_token)),
                provider_debit,
                "{label}"
            );
            assert_eq!(group.insurance, 0, "{label}");
            assert_eq!(env.token_supply_observed(), supply, "{label}");
            if paid[SENIOR] == 0 {
                assert_eq!(env.primary_portfolio_data(SENIOR), senior_before, "{label}");
            }
        };
    let mut provider_debit = INITIAL_BACKING;
    let mut fresh = INITIAL_BACKING + CLAIM;
    check(&env, fresh, provider_debit, false, [0; 3]);

    for (expiry, next_expiry, refill) in [(5, 9, 80u128), (9, 20, final_backing)] {
        assert_eq!(
            env.primary_market_state().1.source_backing_buckets[domain].expiry_slot,
            expiry
        );
        let slot = expiry + u64::from(late);
        env.warp_to_slot(slot);
        refill_history_step(&mut env, &label, |env| env.push_auth_mark(0, slot, mark))
            .expect("authenticate the unchanged mark at expiry");
        check(&env, fresh, provider_debit, false, [0; 3]);
        let mut normalization_steps = 0;
        while env.primary_market_state().1.source_backing_buckets[domain].status
            == BackingBucketStatusV16::Fresh
        {
            assert!(
                normalization_steps < 8,
                "{label}: expiry must progress boundedly"
            );
            refill_history_step(&mut env, &label, |env| {
                env.crank(
                    WINNER,
                    slot,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: env.primary_profile(0).oracle_leg_count,
                    }],
                )
            })
            .expect("permissionless expiry normalization");
            normalization_steps += 1;
            if env.primary_market_state().1.source_backing_buckets[domain].status
                == BackingBucketStatusV16::Expired
            {
                fresh = 0;
            }
            check(&env, fresh, provider_debit, false, [0; 3]);
        }
        assert!(
            normalization_steps > 0,
            "{label}: expiry must be nonvacuous"
        );
        assert_eq!(
            env.primary_market_state().1.source_backing_buckets[domain].status,
            BackingBucketStatusV16::Expired
        );
        fresh = 0;
        check(&env, fresh, provider_debit, false, [0; 3]);
        refresh_refill_history_claimant(&mut env, slot, &label, |env| {
            check(env, fresh, provider_debit, false, [0; 3]);
        });
        check(&env, fresh, provider_debit, false, [0; 3]);

        let first_part = if split { refill / 2 - 1 } else { 0 };
        if first_part != 0 {
            refill_history_step(&mut env, &label, |env| {
                env.top_up_backing_bucket(domain as u16, first_part, next_expiry)
            })
            .expect("independent first refill partition");
            fresh += first_part;
            provider_debit += first_part;
            check(&env, fresh, provider_debit, false, [0; 3]);
        }
        let remainder = refill - first_part;
        for _ in 0..failed_attempts {
            let topup =
                env.build_retained_backing_bucket_top_up(domain as u16, remainder, next_expiry);
            let payer = topup.message.account_keys[0];
            let refresh = Transaction::new_with_payer(
                &[Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(payer, true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.actors[WINNER].portfolio, false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: vec![CrankObservationHint {
                            asset_index: 0,
                            oracle_accounts: env.primary_profile(0).oracle_leg_count,
                        }],
                    }
                    .encode(),
                }],
                Some(&payer),
            );
            let conversion = env.build_retained_convert_released_pnl(WINNER, refill.min(CLAIM) - 1);
            let bundle = env.bundle_retained_transactions(&[topup, refresh, conversion]);
            let signature = bundle.signatures[0];
            refill_history_step(&mut env, &label, |env| env.land_retained(bundle))
                .expect_err("undersized conversion suffix must roll back the preceding refill");
            let rejected = env
                .svm
                .get_transaction(&signature)
                .expect("recorded bundle")
                .as_ref()
                .expect_err("the recorded bundle must fail");
            assert_eq!(
                rejected.err,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(
                        percolator_prog::error::PercolatorError::EngineLockActive as u32,
                    )
                ),
                "{label}: expected the late conversion cap rejection"
            );
            let wrapper_success = format!("Program {} success", env.program_id);
            assert_eq!(
                rejected
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == wrapper_success)
                    .count(),
                2,
                "{label}: both refill and refresh must succeed before rejection"
            );
            assert!(
                rejected
                    .meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)),
                "{label}: refill SPL CPI must succeed before the failing suffix"
            );
            check(&env, fresh, provider_debit, false, [0; 3]);
        }
        refill_history_step(&mut env, &label, |env| {
            env.top_up_backing_bucket(domain as u16, remainder, next_expiry)
        })
        .expect("honest refill after rejected bundles");
        fresh += remainder;
        provider_debit += remainder;
        check(&env, fresh, provider_debit, false, [0; 3]);
        refresh_refill_history_claimant(&mut env, slot, &label, |env| {
            check(env, fresh, provider_debit, false, [0; 3]);
        });
        check(&env, fresh, provider_debit, false, [0; 3]);
    }

    refill_history_step(&mut env, &label, |env| {
        env.convert_released_pnl(WINNER, conversion_atoms)
    })
    .expect("only the last fresh tranche converts the released claim");
    check(&env, surplus, provider_debit, true, [0; 3]);
    let expected_payouts = [
        (DEPOSITS[0] + conversion_atoms) as u64,
        (DEPOSITS[1] - CLAIM) as u64,
        DEPOSITS[2] as u64,
    ];
    let mut paid = [0; 3];
    for actor in [WINNER, LOSER, SENIOR] {
        refill_history_step(&mut env, &label, |env| {
            env.withdraw_primary(actor, u128::from(expected_payouts[actor]))
        })
        .expect("funded owner exit after repeated expiry and refill");
        paid[actor] = expected_payouts[actor];
        check(&env, surplus, provider_debit, true, paid);
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("public refill history trace");
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert_eq!(
        trace.steps.iter().filter(|step| !step.succeeded).count(),
        2 * failed_attempts
    );
    let payouts = std::array::from_fn(|actor| {
        env.token_amount(env.actors[actor].destination_token) - destinations[actor]
    });
    (
        payouts,
        env.primary_market_state().1.vault,
        trace.steps.len(),
    )
}

#[test]
fn v16_program_expiry_refill_failure_histories_preserve_claim_and_senior_exit() {
    let mut worlds = 0;
    let mut transactions = 0;
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for late in [false, true] {
            for split in [false, true] {
                for failed_attempts in [0, 2] {
                    let (payouts, vault, steps) = run_expiry_refill_failure_history(
                        route,
                        late,
                        split,
                        failed_attempts,
                        true,
                        40,
                    );
                    assert_eq!(payouts, [1_040, 900, 777]);
                    assert_eq!(
                        vault, 330,
                        "expired cohorts must not be paid as replacement backing"
                    );
                    worlds += 1;
                    transactions += steps;
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    eprintln!("INV-063 refill histories: {worlds} worlds, {transactions} checked transactions");
}

#[test]
fn v16_program_expiry_refill_source_sides_and_ratios_preserve_attribution() {
    let mut worlds = 0;
    let mut transactions = 0;
    for final_backing in [37u128, 100, 137] {
        let converted = final_backing.min(100);
        for winner_long in [false, true] {
            for late in [false, true] {
                for split in [false, true] {
                    for failed_attempts in [0, 2] {
                        let (payouts, vault, steps) = run_expiry_refill_failure_history(
                            TradeRoute::NoCpi,
                            late,
                            split,
                            failed_attempts,
                            winner_long,
                            final_backing,
                        );
                        assert_eq!(payouts, [1_000 + converted as u64, 900, 777]);
                        assert_eq!(
                            vault,
                            330 + final_backing - converted,
                            "expired cohorts and fresh surplus must remain in custody"
                        );
                        worlds += 1;
                        transactions += steps;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    eprintln!(
        "INV-063 source/ratio histories: {worlds} worlds, {transactions} checked transactions"
    );
}

fn assert_backing_expiry_trade_route_boundary(discovery: &ExpiredBackingTradeRouteDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.uses_fresh_backing_nonvacuously(),
            "{:?} did not consume fresh backing before expiry: {discovery:?}",
            discovery.route
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => {
            assert!(
                discovery.rejects_expired_risk_increase_safely(),
                "{:?} did not reject a {:?} authenticated-expiry lien with exact rollback: {discovery:?}",
                discovery.route,
                discovery.landing
            );
            assert!(
                discovery.preserves_risk_reduction(),
                "{:?} did not preserve a {:?} risk-reducing trade: {discovery:?}",
                discovery.route,
                discovery.landing
            );
        }
    }
}

fn assert_backing_expiry_consumer_boundary(discovery: &ExpiredBackingConsumerDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.consumes_fresh_backing_nonvacuously(),
            "{:?} did not consume fresh backing before expiry: {discovery:?}",
            discovery.kind
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => assert!(
            discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
            "{:?} did not reject a {:?} backing conversion safely: {discovery:?}",
            discovery.kind,
            discovery.landing
        ),
    }
}

fn assert_retained_maturity_boundary(discovery: &RetainedMaturityDiscovery) {
    match discovery.landing {
        BackingExpiryLanding::Before => assert!(
            discovery.accepts_fresh_intent_and_preserves_terminal_progress(),
            "{:?} did not execute fresh retained backing and settle boundedly: {discovery:?}",
            discovery.kind
        ),
        BackingExpiryLanding::At | BackingExpiryLanding::After => assert!(
            discovery.rejects_expired_intent_and_preserves_terminal_progress(),
            "{:?} did not reject a {:?} retained request while preserving terminal progress: {discovery:?}",
            discovery.kind,
            discovery.landing
        ),
    }
}

fn inv063_resolved_portfolio_is_terminal(env: &V16Svm, actor: usize) -> bool {
    let group = env.primary_market_state().1;
    let account = env.primary_portfolio(actor);
    let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() else {
        return false;
    };
    let Ok(close) = account.close_progress.try_to_runtime() else {
        return false;
    };
    group.mode == MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && active_bitmap_is_empty(state::portfolio_active_bitmap(&account))
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

fn drain_inv063_resolved_accounts(
    env: &mut V16Svm,
    actor_order: &[usize],
    claim_first: bool,
    label: &str,
) -> Result<u64, String> {
    const TERMINAL_SWEEP_BOUND: usize = 32;
    let route_order = if claim_first {
        [true, false]
    } else {
        [false, true]
    };
    let mut claim_route_payout = 0u64;

    for sweep in 0..TERMINAL_SWEEP_BOUND {
        if actor_order
            .iter()
            .copied()
            .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
        {
            break;
        }
        let mut sweep_mutated = false;
        for actor in actor_order.iter().copied() {
            if inv063_resolved_portfolio_is_terminal(env, actor) {
                continue;
            }
            for is_claim in route_order {
                let before = snapshot(env);
                let engine_vault_before = env.primary_market_state().1.vault;
                let spl_vault_before = env.token_amount(env.vault);
                let destination = env.actors[actor].destination_token;
                let destination_before = env.token_amount(destination);
                let result = if is_claim {
                    env.claim_resolved_payout_topup_primary(actor)
                } else {
                    env.close_resolved_primary_signed(actor)
                };
                let Ok(success) = result else {
                    if snapshot(env) != before {
                        return Err(format!(
                            "{label} actor {actor} rejected terminal route mutated state"
                        ));
                    }
                    continue;
                };
                if success.compute_units >= TX_CU_LIMIT {
                    return Err(format!(
                        "{label} actor {actor} terminal route consumed {} CU",
                        success.compute_units
                    ));
                }
                let payout = env
                    .token_amount(destination)
                    .checked_sub(destination_before)
                    .ok_or_else(|| format!("{label} actor {actor} destination decreased"))?;
                let spl_debit = spl_vault_before
                    .checked_sub(env.token_amount(env.vault))
                    .ok_or_else(|| format!("{label} terminal route increased SPL vault"))?;
                let engine_debit = engine_vault_before
                    .checked_sub(env.primary_market_state().1.vault)
                    .ok_or_else(|| format!("{label} terminal route increased engine vault"))?;
                if payout != spl_debit || u128::from(payout) != engine_debit {
                    return Err(format!(
                        "{label} actor {actor} payout mismatch: destination={payout}, SPL={spl_debit}, engine={engine_debit}"
                    ));
                }
                if is_claim {
                    claim_route_payout = claim_route_payout
                        .checked_add(payout)
                        .ok_or_else(|| "claim-route payout overflow".to_string())?;
                }
                sweep_mutated |= snapshot(env) != before;
                if inv063_resolved_portfolio_is_terminal(env, actor) {
                    break;
                }
            }
        }
        if !sweep_mutated
            && !actor_order
                .iter()
                .copied()
                .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
        {
            return Err(format!(
                "{label} terminal routes reached a nonterminal fixed point at sweep {sweep}"
            ));
        }
    }

    if !actor_order
        .iter()
        .copied()
        .all(|actor| inv063_resolved_portfolio_is_terminal(env, actor))
    {
        return Err(format!(
            "{label} terminal routes did not converge in {TERMINAL_SWEEP_BOUND} sweeps"
        ));
    }
    Ok(claim_route_payout)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedExpiryOutcome {
    payouts: [u64; 2],
    claim_route_payout: u64,
    bucket_status: BackingBucketStatusV16,
    fresh_unliened_backing_num: u128,
    valid_liened_backing_num: u128,
    consumed_liened_backing_num: u128,
    impaired_liened_backing_num: u128,
    fresh_reserved_backing_num: u128,
    valid_source_liened_backing_num: u128,
    impaired_source_liened_backing_num: u128,
    final_engine_vault: u128,
    final_spl_vault: u64,
}

fn run_resolved_expiry_world(
    landing: BackingExpiryLanding,
    winner_first: bool,
    claim_first: bool,
) -> Result<ResolvedExpiryOutcome, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const ASSET: u16 = 0;
    const WINNING_DOMAIN: u16 = 1;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const BACKING: u128 = 150;
    const EXPIRY_SLOT: u64 = 5;

    let mut seed = [0x67; 32];
    seed[0] ^= match landing {
        BackingExpiryLanding::Before => 1,
        BackingExpiryLanding::At => 2,
        BackingExpiryLanding::After => 3,
    };
    seed[1] ^= u8::from(winner_first);
    seed[2] ^= u8::from(claim_first);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let token_supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install backing provider: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING, EXPIRY_SLOT)
        .map_err(|error| format!("fund expiring backing: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("open source-backed position: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, WINNING_MARK)
        .map_err(|error| format!("publish winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    for actor in [LOSER, WINNER] {
        env.crank(actor, 2, observations())
            .map_err(|error| format!("refresh source-backed actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(WINNER, LOSER, ASSET, -SIZE_Q, WINNING_MARK, 0)
        .map_err(|error| format!("flatten source-backed position: {error}"))?;

    let before_resolution = env.primary_market_state().1;
    let claim_before =
        before_resolution.source_credit[WINNING_DOMAIN as usize].positive_claim_bound_num;
    let bucket_before = before_resolution.source_backing_buckets[WINNING_DOMAIN as usize];
    if claim_before == 0
        || bucket_before.status != BackingBucketStatusV16::Fresh
        || bucket_before.fresh_unliened_backing_num < claim_before
    {
        return Err(format!(
            "fixture did not create a fresh source-backed terminal claim: claim={claim_before}, bucket={bucket_before:?}"
        ));
    }

    let authenticated_slot = match landing {
        BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
        BackingExpiryLanding::At => EXPIRY_SLOT,
        BackingExpiryLanding::After => EXPIRY_SLOT + 1,
    };
    env.warp_to_slot(authenticated_slot);
    env.resolve_market()
        .map_err(|error| format!("resolve at {landing:?}: {error}"))?;
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err(format!(
            "{landing:?} resolution did not enter Resolved mode"
        ));
    }
    let engine_vault_at_resolution = env.primary_market_state().1.vault;
    let spl_vault_at_resolution = env.token_amount(env.vault);
    let destinations_before = [
        env.token_amount(env.actors[WINNER].destination_token),
        env.token_amount(env.actors[LOSER].destination_token),
    ];
    let actor_order = if winner_first {
        [WINNER, LOSER]
    } else {
        [LOSER, WINNER]
    };
    let claim_route_payout = drain_inv063_resolved_accounts(
        &mut env,
        &actor_order,
        claim_first,
        &format!("{landing:?}"),
    )?;
    let group = env.primary_market_state().1;
    let bucket = group.source_backing_buckets[WINNING_DOMAIN as usize];
    let source = group.source_credit[WINNING_DOMAIN as usize];
    let payouts = [
        env.token_amount(env.actors[WINNER].destination_token)
            .checked_sub(destinations_before[0])
            .ok_or_else(|| "winner destination decreased".to_string())?,
        env.token_amount(env.actors[LOSER].destination_token)
            .checked_sub(destinations_before[1])
            .ok_or_else(|| "loser destination decreased".to_string())?,
    ];
    let payout_total = u128::from(payouts[0])
        .checked_add(u128::from(payouts[1]))
        .ok_or_else(|| "terminal payout total overflow".to_string())?;
    if engine_vault_at_resolution
        .checked_sub(group.vault)
        .ok_or_else(|| "terminal settlement increased engine vault".to_string())?
        != payout_total
        || spl_vault_at_resolution
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "terminal settlement increased SPL vault".to_string())?
            != u64::try_from(payout_total).map_err(|_| "payout total exceeds u64".to_string())?
        || u128::from(env.token_amount(env.vault)) != group.vault
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "{landing:?} terminal custody did not reconcile: payouts={payouts:?}, engine={engine_vault_at_resolution}->{}, SPL={spl_vault_at_resolution}->{}, supply={token_supply_before}->{}",
            group.vault,
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }

    Ok(ResolvedExpiryOutcome {
        payouts,
        claim_route_payout,
        bucket_status: bucket.status,
        fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
        valid_liened_backing_num: bucket.valid_liened_backing_num,
        consumed_liened_backing_num: bucket.consumed_liened_backing_num,
        impaired_liened_backing_num: bucket.impaired_liened_backing_num,
        fresh_reserved_backing_num: source.fresh_reserved_backing_num,
        valid_source_liened_backing_num: source.valid_liened_backing_num,
        impaired_source_liened_backing_num: source.impaired_liened_backing_num,
        final_engine_vault: group.vault,
        final_spl_vault: env.token_amount(env.vault),
    })
}

#[test]
fn v16_program_resolved_close_normalizes_backing_at_expiry() {
    let mut canonical = Vec::new();
    for landing in BackingExpiryLanding::ALL {
        let mut outcomes = Vec::new();
        for winner_first in [false, true] {
            for claim_first in [false, true] {
                let outcome = run_resolved_expiry_world(landing, winner_first, claim_first)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{landing:?}/winner_first={winner_first}/claim_first={claim_first}: {error}"
                        )
                    });
                outcomes.push(outcome);
            }
        }
        assert!(
            outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "{landing:?} terminal economics depend on claimant or payout-route order: {outcomes:?}"
        );
        let outcome = outcomes.remove(0);
        match landing {
            BackingExpiryLanding::Before => {
                assert!(
                    outcome.consumed_liened_backing_num != 0,
                    "fresh resolved close must consume source backing nonvacuously: {outcome:?}"
                );
            }
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                assert_ne!(outcome.bucket_status, BackingBucketStatusV16::Fresh);
                assert_eq!(outcome.fresh_unliened_backing_num, 0);
                assert_eq!(outcome.valid_liened_backing_num, 0);
                assert_eq!(outcome.consumed_liened_backing_num, 0);
                assert_eq!(outcome.fresh_reserved_backing_num, 0);
                assert_eq!(outcome.valid_source_liened_backing_num, 0);
            }
        }
        canonical.push(outcome);
    }
    assert_eq!(
        canonical[1], canonical[2],
        "exact and late expiry must have identical terminal economics"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PostSnapshotExpiryEconomicOutcome {
    payouts: [u64; 5],
    bucket_status: BackingBucketStatusV16,
    fresh_unliened_backing_num: u128,
    valid_liened_backing_num: u128,
    consumed_liened_backing_num: u128,
    fresh_reserved_backing_num: u128,
    valid_source_liened_backing_num: u128,
    final_engine_vault: u128,
    final_spl_vault: u64,
}

fn run_post_snapshot_expiry_claim_world(
    landing: BackingExpiryLanding,
    reverse_tail: bool,
    claim_first: bool,
) -> Result<(PostSnapshotExpiryEconomicOutcome, u64), String> {
    const JUNIOR_WINNER: usize = 0;
    const JUNIOR_LOSER: usize = 1;
    const BACKED_WINNER: usize = 2;
    const BACKED_LOSER: usize = 3;
    const PROVIDER: usize = 4;
    const BACKED_ASSET: u16 = 1;
    const JUNIOR_DOMAIN: u16 = 1;
    const BACKED_DOMAIN: u16 = 3;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 150;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const JUNIOR_BACKING: u128 = 1;
    const BACKING: u128 = 1_500;
    const SNAPSHOT_SLOT: u64 = 12;
    const EXPIRY_SLOT: u64 = 13;

    let mut seed = [0x68; 32];
    seed[0] ^= match landing {
        BackingExpiryLanding::Before => 1,
        BackingExpiryLanding::At => 2,
        BackingExpiryLanding::After => 3,
    };
    seed[1] ^= u8::from(reverse_tail);
    seed[2] ^= u8::from(claim_first);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 250, 1_000, 250, 0],
            ..MarketConfig::default()
        },
    );
    let token_supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        0,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install junior backing provider: {error}"))?;
    env.update_asset_authority_from_admin(
        BACKED_ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install post-snapshot backing provider: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, JUNIOR_DOMAIN, JUNIOR_BACKING, SNAPSHOT_SLOT)
        .map_err(|error| format!("fund expiring junior backing: {error}"))?;
    let backed_topup = env.build_retained_backing_bucket_top_up_for_actor(
        PROVIDER,
        BACKED_DOMAIN,
        BACKING,
        EXPIRY_SLOT,
    );
    env.land_retained(backed_topup)
        .map_err(|error| format!("fund post-snapshot backing: {error}"))?;
    env.trade_no_cpi(JUNIOR_WINNER, JUNIOR_LOSER, 0, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("open junior claim: {error}"))?;
    env.trade_no_cpi(
        BACKED_WINNER,
        BACKED_LOSER,
        BACKED_ASSET,
        SIZE_Q,
        INITIAL_PRICE,
        0,
    )
    .map_err(|error| format!("open backed claim: {error}"))?;
    for (offset, mark) in (105..=WINNING_MARK).step_by(5).enumerate() {
        let slot = 2 + u64::try_from(offset).expect("bounded mark sequence");
        env.warp_to_slot(slot);
        for asset in [0, BACKED_ASSET] {
            env.push_auth_mark(asset, slot, mark)
                .map_err(|error| format!("publish asset {asset} mark {mark}: {error}"))?;
        }
        for (actor, asset) in [
            (JUNIOR_LOSER, 0),
            (JUNIOR_WINNER, 0),
            (BACKED_LOSER, BACKED_ASSET),
            (BACKED_WINNER, BACKED_ASSET),
        ] {
            let oracle_accounts = env.primary_profile(asset as usize).oracle_leg_count;
            env.crank(
                actor,
                slot,
                vec![CrankObservationHint {
                    asset_index: asset,
                    oracle_accounts,
                }],
            )
            .map_err(|error| {
                format!("refresh actor {actor} on asset {asset} at mark {mark}: {error}")
            })?;
        }
    }
    env.trade_no_cpi(JUNIOR_WINNER, JUNIOR_LOSER, 0, -SIZE_Q, WINNING_MARK, 0)
        .map_err(|error| format!("flatten junior claim: {error}"))?;
    env.trade_no_cpi(
        BACKED_WINNER,
        BACKED_LOSER,
        BACKED_ASSET,
        -SIZE_Q,
        WINNING_MARK,
        0,
    )
    .map_err(|error| format!("flatten backed claim: {error}"))?;
    let before_resolution = env.primary_market_state().1;
    if before_resolution.source_credit[JUNIOR_DOMAIN as usize].positive_claim_bound_num == 0
        || before_resolution.source_credit[BACKED_DOMAIN as usize].positive_claim_bound_num == 0
        || before_resolution.source_backing_buckets[JUNIOR_DOMAIN as usize].status
            != BackingBucketStatusV16::Fresh
        || before_resolution.source_backing_buckets[BACKED_DOMAIN as usize].status
            != BackingBucketStatusV16::Fresh
    {
        return Err(format!(
            "post-snapshot fixture did not create both junior and backed claims: junior={:?}, backed={:?}, bucket={:?}",
            before_resolution.source_credit[JUNIOR_DOMAIN as usize],
            before_resolution.source_credit[BACKED_DOMAIN as usize],
            before_resolution.source_backing_buckets[BACKED_DOMAIN as usize]
        ));
    }

    env.warp_to_slot(SNAPSHOT_SLOT);
    env.resolve_market()
        .map_err(|error| format!("resolve before backing expiry: {error}"))?;
    let engine_vault_at_resolution = env.primary_market_state().1.vault;
    let spl_vault_at_resolution = env.token_amount(env.vault);
    let destinations_before: [u64; 5] =
        std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));

    let premature_claim_payout = drain_inv063_resolved_accounts(
        &mut env,
        &[JUNIOR_LOSER, BACKED_LOSER, PROVIDER],
        true,
        "pre-snapshot blockers",
    )?;
    if premature_claim_payout != 0 || env.primary_market_state().1.payout_snapshot_captured {
        return Err(format!(
            "closing only nonclaimants unexpectedly paid a receipt or captured the payout snapshot: payout={premature_claim_payout}, ledger={:?}",
            env.primary_market_state().1.resolved_payout_ledger
        ));
    }

    let first_destination = env.actors[JUNIOR_WINNER].destination_token;
    let mut first_receipt = env
        .primary_portfolio(JUNIOR_WINNER)
        .resolved_payout_receipt
        .try_to_runtime()
        .map_err(|error| format!("decode initial claimant receipt: {error:?}"))?;
    for step in 0..8 {
        if first_receipt.present {
            break;
        }
        let first_before = snapshot(&env);
        let first_engine_vault = env.primary_market_state().1.vault;
        let first_spl_vault = env.token_amount(env.vault);
        let first_destination_before = env.token_amount(first_destination);
        let first = env
            .close_resolved_primary_signed(JUNIOR_WINNER)
            .map_err(|error| {
                format!("capture pre-expiry payout snapshot at step {step}: {error}")
            })?;
        if first.compute_units >= TX_CU_LIMIT {
            return Err(format!(
                "snapshot-capturing CloseResolved consumed {} CU",
                first.compute_units
            ));
        }
        let first_payout = env
            .token_amount(first_destination)
            .checked_sub(first_destination_before)
            .ok_or_else(|| "snapshot claimant destination decreased".to_string())?;
        let first_spl_debit = first_spl_vault
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "snapshot close increased SPL vault".to_string())?;
        let first_engine_debit = first_engine_vault
            .checked_sub(env.primary_market_state().1.vault)
            .ok_or_else(|| "snapshot close increased engine vault".to_string())?;
        if first_payout != first_spl_debit || u128::from(first_payout) != first_engine_debit {
            return Err(format!(
                "snapshot payout mismatch: destination={first_payout}, SPL={first_spl_debit}, engine={first_engine_debit}"
            ));
        }
        if snapshot(&env) == first_before {
            let group = env.primary_market_state().1;
            let account = env.primary_portfolio(JUNIOR_WINNER);
            return Err(format!(
                "snapshot-capturing CloseResolved was a successful no-op at step {step}: snapshot={}, stale={}, b_stale={}, negative={}, blockers={}, capital={}, pnl={}, active={:?}, receipt={first_receipt:?}, ledger={:?}",
                group.payout_snapshot_captured,
                group.stale_certificate_count,
                group.b_stale_account_count,
                group.negative_pnl_account_count,
                group.resolved_payout_blocker_count,
                account.capital.get(),
                account.pnl.get(),
                state::portfolio_active_bitmap(&account),
                group.resolved_payout_ledger,
            ));
        }
        first_receipt = env
            .primary_portfolio(JUNIOR_WINNER)
            .resolved_payout_receipt
            .try_to_runtime()
            .map_err(|error| format!("decode claimant receipt at step {step}: {error:?}"))?;
    }
    if !env.primary_market_state().1.payout_snapshot_captured
        || !first_receipt.present
        || first_receipt.finalized
    {
        return Err(format!(
            "first public close did not leave a genuinely partial receipt: receipt={first_receipt:?}, ledger={:?}",
            env.primary_market_state().1.resolved_payout_ledger
        ));
    }

    let landing_slot = match landing {
        BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
        BackingExpiryLanding::At => EXPIRY_SLOT,
        BackingExpiryLanding::After => EXPIRY_SLOT + 1,
    };
    env.warp_to_slot(landing_slot);
    let tail_order = if reverse_tail {
        [
            BACKED_LOSER,
            JUNIOR_LOSER,
            BACKED_WINNER,
            PROVIDER,
            JUNIOR_WINNER,
        ]
    } else {
        [
            BACKED_WINNER,
            JUNIOR_LOSER,
            BACKED_LOSER,
            PROVIDER,
            JUNIOR_WINNER,
        ]
    };
    let claim_route_payout = drain_inv063_resolved_accounts(
        &mut env,
        &tail_order,
        claim_first,
        &format!("post-snapshot {landing:?}"),
    )?;
    let group = env.primary_market_state().1;
    let bucket = group.source_backing_buckets[BACKED_DOMAIN as usize];
    let source = group.source_credit[BACKED_DOMAIN as usize];
    let payouts: [u64; 5] = std::array::from_fn(|actor| {
        env.token_amount(env.actors[actor].destination_token)
            .checked_sub(destinations_before[actor])
            .expect("terminal destination cannot decrease")
    });
    let payout_total = payouts
        .iter()
        .try_fold(0u128, |sum, payout| sum.checked_add(u128::from(*payout)));
    let Some(payout_total) = payout_total else {
        return Err("post-snapshot payout total overflow".to_string());
    };
    if engine_vault_at_resolution
        .checked_sub(group.vault)
        .ok_or_else(|| "post-snapshot settlement increased engine vault".to_string())?
        != payout_total
        || spl_vault_at_resolution
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "post-snapshot settlement increased SPL vault".to_string())?
            != u64::try_from(payout_total).map_err(|_| "payout total exceeds u64".to_string())?
        || u128::from(env.token_amount(env.vault)) != group.vault
        || env.token_supply_observed() != token_supply_before
    {
        return Err(format!(
            "post-snapshot {landing:?} custody mismatch: payouts={payouts:?}, engine={engine_vault_at_resolution}->{}, SPL={spl_vault_at_resolution}->{}, supply={token_supply_before}->{}",
            group.vault,
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }
    Ok((
        PostSnapshotExpiryEconomicOutcome {
            payouts,
            bucket_status: bucket.status,
            fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
            valid_liened_backing_num: bucket.valid_liened_backing_num,
            consumed_liened_backing_num: bucket.consumed_liened_backing_num,
            fresh_reserved_backing_num: source.fresh_reserved_backing_num,
            valid_source_liened_backing_num: source.valid_liened_backing_num,
            final_engine_vault: group.vault,
            final_spl_vault: env.token_amount(env.vault),
        },
        claim_route_payout,
    ))
}

#[test]
fn v16_program_post_snapshot_expiry_topup_is_public_and_order_independent() {
    let mut canonical = Vec::new();
    for landing in BackingExpiryLanding::ALL {
        let mut outcomes = Vec::new();
        let mut claim_first_payouts = Vec::new();
        for reverse_tail in [false, true] {
            for claim_first in [false, true] {
                let (outcome, claim_route_payout) =
                    run_post_snapshot_expiry_claim_world(landing, reverse_tail, claim_first)
                        .unwrap_or_else(|error| {
                            panic!(
                                "{landing:?}/reverse_tail={reverse_tail}/claim_first={claim_first}: {error}"
                            )
                        });
                if claim_first {
                    claim_first_payouts.push(claim_route_payout);
                }
                outcomes.push(outcome);
            }
        }
        assert!(
            outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "post-snapshot {landing:?} economics depend on tail or route order: {outcomes:?}"
        );
        assert!(
            claim_first_payouts.iter().all(|payout| *payout != 0),
            "post-snapshot {landing:?} must exercise a value-moving ClaimResolvedPayoutTopup: {claim_first_payouts:?}"
        );
        let outcome = outcomes.remove(0);
        match landing {
            BackingExpiryLanding::Before => assert!(
                outcome.consumed_liened_backing_num != 0,
                "fresh post-snapshot support must be consumed: {outcome:?}"
            ),
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                assert_ne!(outcome.bucket_status, BackingBucketStatusV16::Fresh);
                assert_eq!(outcome.fresh_unliened_backing_num, 0);
                assert_eq!(outcome.valid_liened_backing_num, 0);
                assert_eq!(outcome.consumed_liened_backing_num, 0);
                assert_eq!(outcome.fresh_reserved_backing_num, 0);
                assert_eq!(outcome.valid_source_liened_backing_num, 0);
            }
        }
        canonical.push(outcome);
    }
    assert_eq!(
        canonical[1], canonical[2],
        "post-snapshot exact and late expiry must have identical terminal economics"
    );
}

#[test]
fn v16_program_backing_expiry_trade_route_boundary_matrix() {
    let discoveries = discover_backing_expiry_trade_route_boundaries([0x63; 32], 2)
        .expect("build every public trade-route and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_backing_expiry_trade_route_boundary(discovery);
    }
}

#[test]
fn v16_program_backing_expiry_conversion_boundary_matrix() {
    let discoveries = discover_backing_expiry_consumer_boundaries([0x64; 32], 2)
        .expect("build every favorable backing-consumer and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        ExpiredBackingConsumerKind::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_backing_expiry_consumer_boundary(discovery);
    }
}

#[test]
fn v16_program_retained_backing_topup_boundary_matrix() {
    let discoveries = discover_retained_maturity_boundaries([0x65; 32], 3)
        .expect("build every retained maturity and expiry-boundary world");
    assert_eq!(
        discoveries.len(),
        RetainedMaturityKind::ALL.len() * BackingExpiryLanding::ALL.len()
    );
    for discovery in &discoveries {
        assert_retained_maturity_boundary(discovery);
    }
}

#[test]
fn v16_program_backing_principal_release_respects_authenticated_expiry() {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 1;
    const BACKING: u128 = 150;
    const WITHDRAWAL: u128 = 25;
    const EXPIRY_SLOT: u64 = 5;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;

    for landing in BackingExpiryLanding::ALL {
        let mut seed = [0x66; 32];
        seed[0] ^= match landing {
            BackingExpiryLanding::Before => 1,
            BackingExpiryLanding::At => 2,
            BackingExpiryLanding::After => 3,
        };
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                initial_price: INITIAL_PRICE,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_accrual_dt_slots: 1,
                min_funding_lifetime_slots: 1,
                actor_deposits: [1_000, 1_000, 0, 0, 0],
                ..MarketConfig::default()
            },
        );
        let supply_before = env.token_supply_observed();
        env.update_asset_authority_from_admin(
            ASSET,
            percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
            PROVIDER,
        )
        .expect("install the independent backing provider");
        env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN, BACKING, EXPIRY_SLOT)
            .expect("fund the expiring backing bucket");
        env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
            .expect("open a position whose favorable PnL uses the source domain");
        env.warp_to_slot(2);
        env.push_auth_mark(ASSET, 2, WINNING_PRICE)
            .expect("publish the favorable authenticated mark");
        let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
        let observations = || {
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts,
            }]
        };
        for actor in [LOSER, WINNER] {
            env.crank(actor, 2, observations())
                .expect("refresh both sides at the favorable mark");
        }
        env.trade_no_cpi(WINNER, LOSER, ASSET, -SIZE_Q, WINNING_PRICE, 0)
            .expect("flatten and retain a real source-backed winner claim");
        let winner_claim =
            env.primary_market_state().1.source_credit[DOMAIN as usize].positive_claim_bound_num;
        assert!(
            winner_claim != 0,
            "fixture must create an independent claim"
        );
        let fresh_backing_before = env.primary_market_state().1.source_backing_buckets
            [DOMAIN as usize]
            .fresh_unliened_backing_num;
        assert!(
            fresh_backing_before
                .checked_sub(WITHDRAWAL * BOUND_SCALE)
                .is_some_and(|remaining| remaining >= winner_claim),
            "fresh control withdrawal must remove only backing excess above the live claim"
        );
        let retained =
            env.build_retained_backing_bucket_withdrawal_for_actor(PROVIDER, DOMAIN, WITHDRAWAL);
        let destination = env.actors[PROVIDER].destination_token;
        let destination_before = env.token_amount(destination);
        let vault_before = env.token_amount(env.vault);
        let internal_vault_before = env.primary_market_state().1.vault;
        let before_landing = snapshot(&env);
        let landing_slot = match landing {
            BackingExpiryLanding::Before => EXPIRY_SLOT - 1,
            BackingExpiryLanding::At => EXPIRY_SLOT,
            BackingExpiryLanding::After => EXPIRY_SLOT + 1,
        };
        env.warp_to_slot(landing_slot);
        let result = env.land_retained(retained);

        match landing {
            BackingExpiryLanding::Before => {
                result.expect("fresh retained backing withdrawal must land");
                assert_eq!(
                    env.token_amount(destination) - destination_before,
                    WITHDRAWAL as u64
                );
                assert_eq!(
                    vault_before - env.token_amount(env.vault),
                    WITHDRAWAL as u64
                );
                assert_eq!(
                    internal_vault_before - env.primary_market_state().1.vault,
                    WITHDRAWAL
                );
                assert_eq!(
                    env.primary_market_state().1.source_backing_buckets[DOMAIN as usize]
                        .fresh_unliened_backing_num,
                    fresh_backing_before - WITHDRAWAL * BOUND_SCALE
                );
            }
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                let error = result.expect_err("expired retained backing withdrawal must reject");
                assert!(
                    error.contains("Custom(19)") || error.contains("custom program error: 0x13"),
                    "expired withdrawal must reject as EngineStale: {error}"
                );
                assert_eq!(
                    snapshot(&env),
                    before_landing,
                    "expired provider withdrawal must roll back exactly at {landing:?}"
                );
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
                assert_eq!(env.primary_market_state().1.vault, internal_vault_before);

                let mut expiry_steps = 0usize;
                while env.primary_market_state().1.source_backing_buckets[DOMAIN as usize].status
                    == percolator::BackingBucketStatusV16::Fresh
                    && expiry_steps < 8
                {
                    env.crank(WINNER, landing_slot, observations())
                        .expect("a bounded claimant crank must progress expiry");
                    expiry_steps += 1;
                }
                let expired = env.primary_market_state().1.source_backing_buckets[DOMAIN as usize];
                assert_ne!(
                    expired.status,
                    percolator::BackingBucketStatusV16::Fresh,
                    "the canonical expiry continuation must remove freshness"
                );
                assert_eq!(expired.fresh_unliened_backing_num, 0);
                assert!(expiry_steps != 0 && expiry_steps <= 8);
                assert_eq!(
                    env.primary_market_state().1.source_credit[DOMAIN as usize]
                        .fresh_reserved_backing_num,
                    0
                );
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
                assert_eq!(env.primary_market_state().1.vault, internal_vault_before);

                env.withdraw_backing_bucket_for_actor(PROVIDER, DOMAIN, WITHDRAWAL)
                    .expect_err("expired provider principal must remain non-withdrawable");
                assert_eq!(env.token_amount(destination), destination_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
            }
        }
        assert_eq!(env.token_supply_observed(), supply_before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_063_backing_expiry_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backing_expiry_boundary_rejects_stale_fee_and_preserves_exit(
        seed in any::<[u8; 32]>(),
        expiry_offset in prop::sample::select(vec![2u8, 3, 5, 8]),
    ) {
        let case = BackingExpiryCase {
            fee_bps: 5_000,
            expiry_offset,
            mark_move_bps: 500,
            increase_divisor: 20,
        };
        let result = discover_backing_expiry_violation(seed, case);
        prop_assert!(
            result.is_ok(),
            "backing-expiry verification failed for case {:?}: {}",
            case,
            result.unwrap_err()
        );
        let discovery = result.unwrap();
        prop_assert!(
            discovery.preserves_expiry_normalization(),
            "expired backing was not rejected without value movement while preserving exit: {:?}",
            discovery
        );
    }

    #[test]
    fn v16_program_backing_expiry_trade_routes_respect_boundary(
        seed in any::<[u8; 32]>(),
        route in prop::sample::select(DiscoveryTradeRoute::ALL.to_vec()),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discovery = discover_backing_expiry_trade_route_boundary(
            seed,
            route,
            expiry_offset,
            landing,
        )
            .map_err(TestCaseError::fail)?;
        match landing {
            BackingExpiryLanding::Before => prop_assert!(
                discovery.uses_fresh_backing_nonvacuously(),
                "{route:?} did not use pre-expiry backing nonvacuously: {discovery:?}"
            ),
            BackingExpiryLanding::At | BackingExpiryLanding::After => {
                prop_assert!(
                    discovery.rejects_expired_risk_increase_safely(),
                    "{route:?} did not reject a {landing:?} authenticated-expiry lien safely: {discovery:?}"
                );
                prop_assert!(
                    discovery.preserves_risk_reduction(),
                    "{route:?} did not preserve {landing:?} risk reduction: {discovery:?}"
                );
            }
        }
    }

    #[test]
    fn v16_program_retained_maturity_matrix_respects_expiry_boundary(
        seed in any::<[u8; 32]>(),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![2u8, 3, 4, 6]),
    ) {
        let discoveries = discover_retained_maturity_boundary(seed, expiry_offset, landing);
        prop_assert!(
            discoveries.is_ok(),
            "retained-maturity verification failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            RetainedMaturityKind::ALL.len(),
            "every retained maturity operation needs a generated world"
        );
        for discovery in discoveries {
            match landing {
                BackingExpiryLanding::Before => prop_assert!(
                    discovery.accepts_fresh_intent_and_preserves_terminal_progress(),
                    "fresh retained operation was not nonvacuous or terminal-safe: {discovery:?}"
                ),
                BackingExpiryLanding::At | BackingExpiryLanding::After => prop_assert!(
                    discovery.rejects_expired_intent_and_preserves_terminal_progress(),
                    "expired retained operation did not reject while preserving terminal progress: {discovery:?}"
                ),
            }
        }
    }

    #[test]
    fn v16_program_backing_expiry_consumer_matrix_respects_boundary(
        seed in any::<[u8; 32]>(),
        landing in prop::sample::select(BackingExpiryLanding::ALL.to_vec()),
        expiry_offset in prop::sample::select(vec![1u8, 2, 4, 6]),
    ) {
        let discoveries = discover_backing_expiry_consumer_boundary(
            seed,
            expiry_offset,
            landing,
        );
        prop_assert!(
            discoveries.is_ok(),
            "expired-backing consumer verification failed at offset {expiry_offset}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            ExpiredBackingConsumerKind::ALL.len(),
            "every favorable backing consumer needs a generated expiry world"
        );
        for discovery in discoveries {
            match landing {
                BackingExpiryLanding::Before => prop_assert!(
                    discovery.consumes_fresh_backing_nonvacuously(),
                    "fresh backing consumer was not exercised nonvacuously: {discovery:?}"
                ),
                BackingExpiryLanding::At | BackingExpiryLanding::After => prop_assert!(
                    discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
                    "expired backing consumer was not rejected safely with a senior exit: {discovery:?}"
                ),
            }
        }
    }
}
