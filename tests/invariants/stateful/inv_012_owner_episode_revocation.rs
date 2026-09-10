//! INV-012: retained authority across owner reduction, conversion and Recovery episodes.
//!
//! Request freshness and standing authority are separate obligations. Even a
//! conversion that leaves the position vector unchanged consumes an episode and
//! revokes its grant. Unrelated grants remain usable. This bounded writer/scope
//! product extends the common event oracle, not the engine's transition proofs.
//! Retained SetMatcherConfig re-delivery belongs to production-fix PR #412.

use super::*;
use percolator_prog::ix::CrankObservationHint;

const PRICE: u64 = 100;
const CAPITAL: u128 = 1_000_000;
const QUANTITY: i128 = 12 * POS_SCALE as i128;
const CLAIM: u128 = 60;
const LP: usize = 1;
const COUNTERPARTY: usize = 3;
const CONSUMER_ASSET: u16 = 1;

#[derive(Clone, Copy, Debug)]
enum OwnerWriter {
    PartialReduction,
    FullReduction,
    ReleasedConversion,
}

#[derive(Default, Debug)]
struct Evidence {
    histories: usize,
    transactions: usize,
    live_simulations: usize,
    stale_requests: usize,
    revoked_current_requests: usize,
    preserved_fills: usize,
    reauthorized_fills: usize,
    max_cu: u64,
    max_writer_cu: u64,
}

fn step(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    evidence: &mut Evidence,
    event: Option<AuthorizationEvent>,
    expected_error: Option<PercolatorError>,
    execute: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) {
    let error = capability_step(env, history, expected_error.is_none(), event, |env| {
        let result = execute(env);
        if let Ok(success) = &result {
            evidence.max_cu = evidence.max_cu.max(success.compute_units);
            if matches!(event, Some(AuthorizationEvent::OwnerEpisode { .. })) {
                evidence.max_writer_cu = evidence.max_writer_cu.max(success.compute_units);
            }
        }
        result
    });
    if let Some(expected) = expected_error {
        let expected = format!("Custom({})", expected as u32);
        let error = error.unwrap();
        assert!(
            error.contains(&expected),
            "expected {expected}, got {error}"
        );
    }
    evidence.transactions += 1;
}

fn simulate_live(
    env: &mut V16Svm,
    history: &AuthorizationHistory,
    evidence: &mut Evidence,
    tx: &Transaction,
) {
    let before = capability_frame(env, history);
    let meta = env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect("the exact retained request must be live before revocation");
    assert!(meta.compute_units_consumed <= TX_CU_LIMIT);
    assert!(capability_frame(env, history) == before, "simulation frame");
    evidence.live_simulations += 1;
}

fn consume(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    evidence: &mut Evidence,
    tx: Transaction,
    request: &[GrantOracle],
    size: i128,
) {
    let current = history.replay();
    let stale = current[0].epoch != request[0].epoch
        || current[LP].epoch != request[LP].epoch
        || current[LP].sequence != request[LP].sequence;
    let error = if stale {
        evidence.stale_requests += 1;
        Some(PercolatorError::EngineStale)
    } else if !current[LP].authorizes(request[LP].scope, request[LP].sequence, env.current_slot()) {
        evidence.revoked_current_requests += 1;
        Some(PercolatorError::Unauthorized)
    } else {
        None
    };
    step(
        env,
        history,
        evidence,
        Some(AuthorizationEvent::Fill {
            taker: 0,
            lp: LP,
            asset: CONSUMER_ASSET as usize,
            size,
            cpi: true,
        }),
        error,
        |env| env.land_retained(tx),
    );
}

fn retain(env: &mut V16Svm, route: CpiRoute, size: i128) -> Transaction {
    match route {
        CpiRoute::Single => env.build_retained_cpi_trade(0, LP, CONSUMER_ASSET, size, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(0, LP, CONSUMER_ASSET, size, 0),
    }
}

fn run_case(
    writer: OwnerWriter,
    subject: usize,
    route: CpiRoute,
    sign: i128,
    evidence: &mut Evidence,
) {
    let mut env = V16Svm::new(
        [0x2c; 32],
        MarketConfig {
            initial_price: PRICE,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [CAPITAL; 5],
            actor_token_balances: [2_000_000; 5],
            ..MarketConfig::default()
        },
    );
    let mut history = AuthorizationHistory::new(&env);
    step(&mut env, &mut history, evidence, None, None, |env| {
        env.configure_auth_mark(false, 0, 1, PRICE)
    });
    let conversion = matches!(writer, OwnerWriter::ReleasedConversion);
    if conversion {
        step(&mut env, &mut history, evidence, None, None, |env| {
            env.top_up_backing_bucket(u16::from(sign > 0), 1_000, 100)
        });
    }
    step(
        &mut env,
        &mut history,
        evidence,
        Some(AuthorizationEvent::Fill {
            taker: subject,
            lp: COUNTERPARTY,
            asset: 0,
            size: sign * QUANTITY,
            cpi: false,
        }),
        None,
        |env| env.trade_no_cpi(subject, COUNTERPARTY, 0, sign * QUANTITY, PRICE, 0),
    );
    if conversion {
        let price = (PRICE as i128 + sign * 5) as u64;
        env.warp_to_slot(2);
        step(&mut env, &mut history, evidence, None, None, |env| {
            env.push_auth_mark(0, 2, price)
        });
        for actor in [COUNTERPARTY, subject] {
            step(&mut env, &mut history, evidence, None, None, |env| {
                env.crank(
                    actor,
                    2,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
            });
        }
        step(
            &mut env,
            &mut history,
            evidence,
            Some(AuthorizationEvent::Fill {
                taker: subject,
                lp: COUNTERPARTY,
                asset: 0,
                size: -sign * QUANTITY,
                cpi: false,
            }),
            None,
            |env| env.trade_no_cpi(subject, COUNTERPARTY, 0, -sign * QUANTITY, price, 0),
        );
        assert_eq!(env.primary_portfolio(subject).pnl.get(), CLAIM as i128);
        assert_eq!(env.primary_portfolio(subject).capital.get(), CAPITAL);
        step(&mut env, &mut history, evidence, None, None, |env| {
            env.push_auth_mark(CONSUMER_ASSET, 2, PRICE)
        });
        for actor in [0, LP] {
            // The first crank advances the sibling asset. A subject LP was
            // already certified at slot 2 by the signed close above.
            if actor == LP && subject == LP {
                continue;
            }
            step(&mut env, &mut history, evidence, None, None, |env| {
                env.crank(
                    actor,
                    2,
                    vec![CrankObservationHint {
                        asset_index: CONSUMER_ASSET,
                        oracle_accounts: 0,
                    }],
                )
            });
        }
    }
    for actor in [0, LP, 2]
        .into_iter()
        .filter(|actor| *actor == subject || *actor == LP)
    {
        grant_capability(&mut env, &mut history, actor, Some(37), 100);
        evidence.transactions += 1;
    }

    // A separate live asset isolates portfolio-wide revocation from the reduced
    // asset's admission locks and stale counterparty cohort.
    let size = sign * POS_SCALE as i128;
    let request = history.replay();
    let retained = retain(&mut env, route, size);
    let retained_bytes = bincode::serialize(&retained).unwrap();
    simulate_live(&mut env, &history, evidence, &retained);
    step(
        &mut env,
        &mut history,
        evidence,
        None,
        Some(PercolatorError::InvalidInstruction),
        |env| {
            if conversion {
                env.convert_released_pnl(subject, 0)
            } else {
                env.rebalance_reduce(subject, 0, 0)
            }
        },
    );
    simulate_live(&mut env, &history, evidence, &retained);

    let reduction = match writer {
        OwnerWriter::PartialReduction => QUANTITY / 4,
        OwnerWriter::FullReduction => QUANTITY,
        OwnerWriter::ReleasedConversion => 0,
    };
    let lp_before = env.primary_portfolio_data(LP);
    let contexts_before = env.all_matcher_context_data();
    step(
        &mut env,
        &mut history,
        evidence,
        Some(AuthorizationEvent::OwnerEpisode {
            actor: subject,
            asset: 0,
            position_delta: -sign * reduction,
        }),
        None,
        |env| {
            if conversion {
                env.convert_released_pnl(subject, CLAIM)
            } else {
                env.rebalance_reduce(subject, 0, reduction as u128)
            }
        },
    );
    assert_eq!(env.all_matcher_context_data(), contexts_before);
    if subject != LP {
        assert_eq!(env.primary_portfolio_data(LP), lp_before, "untouched LP");
    }
    if conversion {
        assert_eq!(env.primary_portfolio(subject).pnl.get(), 0);
        assert_eq!(
            env.primary_portfolio(subject).capital.get(),
            CAPITAL + CLAIM
        );
        assert_eq!(history.replay()[subject].positions, [0; ASSET_COUNT]);
    }
    let after = history.replay();
    assert_eq!(after[subject].epoch, request[subject].epoch + 1);
    assert_eq!(after[subject].sequence, request[subject].sequence);
    assert!(!after[subject].authorizes(
        request[subject].scope,
        request[subject].sequence,
        env.current_slot()
    ));
    assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
    consume(&mut env, &mut history, evidence, retained, &request, size);

    // Refresh only the signed request. In particular, the LP scope must reject
    // as Unauthorized, not hide a surviving grant behind EngineStale.
    let current_request = history.replay();
    let current = retain(&mut env, route, size);
    consume(
        &mut env,
        &mut history,
        evidence,
        current,
        &current_request,
        size,
    );
    if subject == LP {
        grant_capability(&mut env, &mut history, LP, Some(37), 100);
        evidence.transactions += 1;
        let fresh_request = history.replay();
        let fresh = retain(&mut env, route, size);
        consume(
            &mut env,
            &mut history,
            evidence,
            fresh,
            &fresh_request,
            size,
        );
        evidence.reauthorized_fills += 1;
    } else {
        evidence.preserved_fills += 1 + usize::from(subject == 2);
        assert_eq!(history.replay()[LP].sequence, request[LP].sequence);
    }
    history.assert_prefix(&env);
    evidence.histories += 1;
}

#[test]
fn v16_program_retained_capability_cannot_cross_owner_episode_revocation() {
    let mut evidence = Evidence::default();
    for writer in [
        OwnerWriter::PartialReduction,
        OwnerWriter::FullReduction,
        OwnerWriter::ReleasedConversion,
    ] {
        for subject in [0, LP, 2] {
            for route in [CpiRoute::Single, CpiRoute::Batch] {
                for sign in [-1, 1] {
                    eprintln!("owner episode partition: {writer:?}, subject={subject}, {route:?}, sign={sign}");
                    run_case(writer, subject, route, sign, &mut evidence);
                }
            }
        }
    }
    assert_eq!(evidence.histories, 36);
    assert_eq!(evidence.transactions, 392);
    assert_eq!(evidence.live_simulations, 72);
    assert_eq!(evidence.stale_requests, 24);
    assert_eq!(evidence.revoked_current_requests, 12);
    assert_eq!(evidence.preserved_fills, 36);
    assert_eq!(evidence.reauthorized_fills, 12);
    eprintln!("INV-012 owner episode revocation: {evidence:?}; two assets, one consumer leg, zero fees; recovery, cure, liquidation, force-close and generation/incarnation histories remain outside this increment");
}

#[test]
fn v16_program_recovery_forfeit_revokes_retained_live_sibling_capability() {
    let mut evidence = Evidence::default();
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        for sign in [-1, 1] {
            eprintln!("Recovery forfeit partition: {route:?}, sign={sign}");
            let mut env = V16Svm::new(
                [0x6c; 32],
                MarketConfig {
                    initial_price: PRICE,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    actor_deposits: [CAPITAL; 5],
                    actor_token_balances: [2_000_000; 5],
                    ..MarketConfig::default()
                },
            );
            let mut history = AuthorizationHistory::new(&env);
            step(&mut env, &mut history, &mut evidence, None, None, |env| {
                env.configure_permissionless_resolve(1_000, 100)
            });
            step(
                &mut env,
                &mut history,
                &mut evidence,
                Some(AuthorizationEvent::Fill {
                    taker: LP,
                    lp: COUNTERPARTY,
                    asset: 0,
                    size: sign * QUANTITY,
                    cpi: false,
                }),
                None,
                |env| env.trade_no_cpi(LP, COUNTERPARTY, 0, sign * QUANTITY, PRICE, 0),
            );
            grant_capability(&mut env, &mut history, LP, Some(37), 100);
            evidence.transactions += 1;
            let size = sign * POS_SCALE as i128;
            let opening_request = history.replay();
            let opening = retain(&mut env, route, 2 * size);
            consume(
                &mut env,
                &mut history,
                &mut evidence,
                opening,
                &opening_request,
                2 * size,
            );

            let request = history.replay();
            let retained = retain(&mut env, route, size);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            assert!(
                !retained.message.account_keys
                    [..retained.message.header.num_required_signatures as usize]
                    .contains(&env.actors[LP].signer.pubkey()),
                "the retained consumer relies on delegated LP authority"
            );
            simulate_live(&mut env, &history, &mut evidence, &retained);

            // Asset shutdown alone leaves the portfolio grant untouched. The
            // owner forfeit must revoke it across the still-live sibling leg.
            let lp_before = env.primary_portfolio_data(LP);
            let contexts_before = env.all_matcher_context_data();
            let tokens_before = env.all_token_account_data();
            step(&mut env, &mut history, &mut evidence, None, None, |env| {
                env.shutdown_asset(0, 1)
            });
            assert_eq!(env.primary_portfolio_data(LP), lp_before);
            assert_eq!(
                env.primary_market_state().1.assets[0].lifecycle,
                percolator::AssetLifecycleV16::Recovery
            );
            step(
                &mut env,
                &mut history,
                &mut evidence,
                None,
                Some(PercolatorError::InvalidInstruction),
                |env| env.forfeit_recovery_leg(LP, 0, 0),
            );
            step(
                &mut env,
                &mut history,
                &mut evidence,
                Some(AuthorizationEvent::OwnerEpisode {
                    actor: LP,
                    asset: 0,
                    position_delta: -sign * QUANTITY,
                }),
                None,
                |env| env.forfeit_recovery_leg(LP, 0, u128::MAX),
            );
            assert_eq!(env.all_matcher_context_data(), contexts_before);
            assert_eq!(env.all_token_account_data(), tokens_before);
            let current_request = history.replay();
            assert_eq!(current_request[LP].positions[0], 0);
            assert_eq!(
                current_request[LP].positions[CONSUMER_ASSET as usize],
                -2 * size
            );
            assert_eq!(current_request[LP].epoch, request[LP].epoch + 1);
            assert_eq!(current_request[LP].sequence, request[LP].sequence);
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            consume(
                &mut env,
                &mut history,
                &mut evidence,
                retained,
                &request,
                size,
            );

            // A current request under the unrenewed grant must fail as
            // Unauthorized, independently of stale request-epoch rejection.
            let current = retain(&mut env, route, size);
            consume(
                &mut env,
                &mut history,
                &mut evidence,
                current,
                &current_request,
                size,
            );
            grant_capability(&mut env, &mut history, LP, Some(37), 100);
            evidence.transactions += 1;
            let fresh_request = history.replay();
            let fresh = retain(&mut env, route, size);
            consume(
                &mut env,
                &mut history,
                &mut evidence,
                fresh,
                &fresh_request,
                size,
            );
            assert_eq!(
                history.replay()[LP].positions[CONSUMER_ASSET as usize],
                -3 * size
            );
            evidence.reauthorized_fills += 1;
            evidence.histories += 1;
        }
    }
    assert_eq!(evidence.histories, 4);
    assert_eq!(evidence.transactions, 44);
    assert_eq!(evidence.live_simulations, 4);
    assert_eq!(evidence.stale_requests, 4);
    assert_eq!(evidence.revoked_current_requests, 4);
    assert_eq!(evidence.reauthorized_fills, 4);
    eprintln!("INV-012 Recovery forfeit revocation: {evidence:?}; two assets, one consumer leg, unchanged prices, zero fees; cure and automatic keeper detachment remain outside this increment");
}
