//! INV-012 - Capability and delegate scope.
//!
//! A retained CPI trade must bind the exact incarnation of the LP matcher
//! capability it intends to consume. Re-enabling the same program, context,
//! delegate, and fee cap is a new grant, not permission to revive transactions
//! retained under the old grant.
//!
//! This public LiteSVM matrix covers both CPI transports. Each world retains a
//! valid transaction, disables and re-enables the identical matcher tuple, and
//! requires the old transaction to reject with exact program-account, matcher,
//! token-supply, and lamport rollback. A transaction built after re-enable must
//! still execute, excluding an always-rejecting fix.

use crate::support::v16_svm::{MarketConfig, TxSuccess, V16Svm};
use percolator::POS_SCALE;
use percolator_prog::{error::PercolatorError, ix::Instruction as ProgInstruction, state};
use proptest::{
    prelude::*,
    test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

#[derive(Clone, Copy, Debug)]
enum CpiRoute {
    Single,
    Batch,
}

fn run_matcher_capability_aba_case(route: CpiRoute) {
    const TAKER: usize = 0;
    const LP: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut seed = [0x12; 32];
    seed[0] ^= match route {
        CpiRoute::Single => 1,
        CpiRoute::Batch => 2,
    };
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: [DEPOSIT, DEPOSIT, 0, 0, 0],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, 0, 1, PRICE)
        .expect("configure authenticated mark");

    let retained = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    let old_sequence = env.primary_portfolio_matcher_sequence(LP);
    env.set_matcher_config(LP, 0).expect("disable matcher");
    env.set_matcher_config(LP, 1)
        .expect("re-enable identical matcher tuple");
    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        old_sequence + 2,
        "the replacement grant must be a distinct matcher-config incarnation",
    );

    let market_before = env.market_data(false);
    let taker_before = env.primary_portfolio_data(TAKER);
    let lp_before = env.primary_portfolio_data(LP);
    let matcher_before = env.all_matcher_context_data();
    let supply_before = env.token_supply_observed();
    let taker_lamports_before = env.account_lamports(env.actors[TAKER].portfolio);
    let lp_lamports_before = env.account_lamports(env.actors[LP].portfolio);

    let stale_error = env
        .land_retained(retained)
        .expect_err("a transaction retained under the prior matcher grant must reject");
    let expected_error = format!("Custom({})", PercolatorError::EngineStale as u32);
    assert!(
        stale_error.contains(&expected_error),
        "stale matcher grant must fail with {expected_error}, got {stale_error}",
    );
    assert_eq!(env.market_data(false), market_before, "market rollback");
    assert_eq!(
        env.primary_portfolio_data(TAKER),
        taker_before,
        "taker rollback"
    );
    assert_eq!(env.primary_portfolio_data(LP), lp_before, "LP rollback");
    assert_eq!(
        env.all_matcher_context_data(),
        matcher_before,
        "matcher rollback"
    );
    assert_eq!(
        env.token_supply_observed(),
        supply_before,
        "SPL supply rollback"
    );
    assert_eq!(
        env.account_lamports(env.actors[TAKER].portfolio),
        taker_lamports_before,
        "taker lamport rollback",
    );
    assert_eq!(
        env.account_lamports(env.actors[LP].portfolio),
        lp_lamports_before,
        "LP lamport rollback",
    );

    let fresh = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    env.land_retained(fresh)
        .expect("the current matcher-config incarnation must remain live");
    let taker = env.primary_portfolio(TAKER);
    let lp = env.primary_portfolio(LP);
    assert!(
        taker.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh taker transaction must install real exposure",
    );
    assert!(
        lp.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh LP transaction must install real exposure",
    );
}

#[test]
fn v16_program_cpi_trades_bind_matcher_capability_incarnation() {
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        run_matcher_capability_aba_case(route);
    }
}

// Bounded prototype: one writer, one asset/leg, zero fees, and three portfolio roles.
// Recovery/close/keeper writers, tuple substitutions, nonzero fee/limit boundaries,
// multi-writer words and maximum shapes remain unconstructed, not presumed covered.
#[derive(Clone, Copy, Debug)]
enum CapabilityWriter {
    Deposit,
    InvalidRenewal,
    Revoke,
    Renew,
    LowerCap,
    SingleNoCpi,
    BatchNoCpi,
    SingleCpi,
    BatchCpi,
}

const CAPABILITY_WRITERS: [CapabilityWriter; 9] = [
    CapabilityWriter::Deposit,
    CapabilityWriter::InvalidRenewal,
    CapabilityWriter::Revoke,
    CapabilityWriter::Renew,
    CapabilityWriter::LowerCap,
    CapabilityWriter::SingleNoCpi,
    CapabilityWriter::BatchNoCpi,
    CapabilityWriter::SingleCpi,
    CapabilityWriter::BatchCpi,
];

#[derive(Clone, Copy, Debug)]
enum WriterScope {
    Lp,
    Taker,
    Unrelated,
}

#[derive(Clone, Copy, Debug)]
struct CapabilityCase {
    writer: CapabilityWriter,
    scope: WriterScope,
    route: CpiRoute,
    landing_slot: u64,
    size: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GrantOracle {
    // Program, market, portfolio, owner, matcher program, context, delegate.
    scope: [Pubkey; 7],
    portfolio_id: u64,
    epoch: u64,
    sequence: u64,
    enabled: bool,
    cap: u16,
    expiry: u64,
    position: i128,
}

impl GrantOracle {
    fn authorizes(&self, scope: [Pubkey; 7], sequence: u64, slot: u64) -> bool {
        self.scope == scope && self.sequence == sequence && self.enabled && slot < self.expiry
    }
}

#[derive(Clone, Copy, Debug)]
enum AuthorizationEvent {
    Deposit {
        actor: usize,
    },
    Grant {
        actor: usize,
        cap: Option<u16>,
        expiry: u64,
    },
    Fill {
        taker: usize,
        lp: usize,
        size: i128,
        cpi: bool,
    },
}

struct AuthorizationHistory {
    created: Vec<GrantOracle>,
    accepted: Vec<AuthorizationEvent>,
}

impl AuthorizationHistory {
    fn new(env: &V16Svm) -> Self {
        let created = env
            .actors
            .iter()
            .enumerate()
            .map(|(i, actor)| GrantOracle {
                scope: [
                    env.program_id,
                    env.market,
                    actor.portfolio,
                    actor.signer.pubkey(),
                    env.matcher_program,
                    actor.matcher_context,
                    actor.matcher_delegate,
                ],
                portfolio_id: env.primary_portfolio_id(i),
                epoch: env.primary_portfolio_position_epoch(i),
                sequence: 0,
                enabled: false,
                cap: 0,
                expiry: 0,
                position: 0,
            })
            .collect();
        // V16Svm::new publicly creates each portfolio, installs exactly one grant,
        // then deposits. Both grant and deposit consume the shared owner-state sequence.
        let accepted = (0..env.actors.len())
            .flat_map(|actor| {
                [
                    AuthorizationEvent::Grant {
                        actor,
                        cap: Some(10_000),
                        expiry: u64::MAX,
                    },
                    AuthorizationEvent::Deposit { actor },
                ]
            })
            .collect();
        Self { created, accepted }
    }

    fn replay(&self) -> Vec<GrantOracle> {
        let mut grants = self.created.clone();
        for event in &self.accepted {
            match *event {
                AuthorizationEvent::Deposit { actor } => grants[actor].sequence += 1,
                AuthorizationEvent::Grant { actor, cap, expiry } => {
                    let grant = &mut grants[actor];
                    grant.sequence += 1;
                    grant.enabled = cap.is_some();
                    grant.cap = cap.unwrap_or(0);
                    grant.expiry = expiry;
                }
                AuthorizationEvent::Fill {
                    taker,
                    lp,
                    size,
                    cpi,
                } => {
                    for (actor, delta) in [(taker, size), (lp, -size)] {
                        let grant = &mut grants[actor];
                        grant.epoch += 1;
                        grant.position += delta;
                        if !(cpi && actor == lp) {
                            grant.enabled = false;
                            grant.expiry = 0;
                        }
                    }
                }
            }
        }
        grants
    }

    fn assert_prefix(&self, env: &V16Svm) {
        for (i, expected) in self.replay().iter().enumerate() {
            let data = env.primary_portfolio_data(i);
            let observed = state::read_portfolio_matcher_config(&data).unwrap();
            assert_eq!(env.primary_portfolio_id(i), expected.portfolio_id);
            assert_eq!(
                observed.position_epoch(),
                expected.epoch,
                "actor {i} episode"
            );
            assert_eq!(
                env.primary_portfolio_matcher_sequence(i),
                expected.sequence,
                "actor {i} grant incarnation"
            );
            assert_eq!(
                observed.enabled(),
                u64::from(expected.enabled),
                "actor {i} grant disposition"
            );
            assert_eq!(
                observed.trade_fee_cap_bps(),
                expected.cap,
                "actor {i} retained cap"
            );
            assert_eq!(
                state::read_portfolio_matcher_expiry(&data).unwrap(),
                expected.expiry,
                "actor {i} expiry"
            );
            if expected.enabled {
                assert_eq!(observed.matcher_program, expected.scope[4].to_bytes());
                assert_eq!(observed.matcher_context, expected.scope[5].to_bytes());
                assert_eq!(observed.matcher_delegate, expected.scope[6].to_bytes());
            }
            assert_eq!(
                env.primary_portfolio(i).legs[0].basis_pos_q.get(),
                expected.position,
                "actor {i} exact signed fill history"
            );
        }
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    }
}

fn capability_frame(env: &V16Svm) -> Vec<(Pubkey, Account)> {
    let mut keys: Vec<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    // Actor 4 pays only for locally constructed grants; network fees are separate.
    keys.extend(env.actors[..4].iter().map(|actor| actor.signer.pubkey()));
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key).unwrap()))
        .collect()
}

fn capability_step(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    succeeds: bool,
    event: Option<AuthorizationEvent>,
    execute: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) -> Option<String> {
    history.assert_prefix(env);
    let before = capability_frame(env);
    env.begin_public_trace();
    let result = execute(env);
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("public-only instruction and rollback trace");
    assert_eq!(trace.steps.len(), 1, "no hidden setup or reauthorization");
    assert_eq!(
        result.is_ok(),
        succeeds,
        "unexpected public result: {result:?}"
    );
    if succeeds {
        if let Some(event) = event {
            history.accepted.push(event);
        }
    } else {
        assert!(
            capability_frame(env) == before,
            "exact account/metadata/CPI/SPL/economic-lamport rollback"
        );
        assert!(
            result.as_ref().unwrap_err().contains("Custom("),
            "runtime failure is not authorization evidence: {result:?}"
        );
    }
    history.assert_prefix(env);
    result.err()
}

fn grant_capability(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    actor: usize,
    cap: Option<u16>,
    expiry: u64,
) {
    let grant = history.replay()[actor];
    let owner = &env.actors[actor].signer;
    let payer = &env.actors[4].signer;
    let mut accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new_readonly(env.market, false),
        AccountMeta::new(grant.scope[2], false),
    ];
    if cap.is_some() {
        accounts.extend(
            grant.scope[4..]
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false)),
        );
    }
    let ix = Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: grant.portfolio_id,
            expected_sequence: grant.sequence,
            enabled: u8::from(cap.is_some()),
            trade_fee_cap_bps: cap.unwrap_or(0),
            expiry_slot: expiry,
        }
        .encode(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, owner],
        env.svm.latest_blockhash(),
    );
    let succeeds = match cap {
        Some(cap) => cap <= 10_000 && env.current_slot() < expiry,
        None => expiry == 0,
    };
    capability_step(
        env,
        history,
        succeeds,
        Some(AuthorizationEvent::Grant { actor, cap, expiry }),
        |env| env.land_retained(tx),
    );
}

fn retained_capability_trade(env: &mut V16Svm, route: CpiRoute, size: i128) -> Transaction {
    match route {
        CpiRoute::Single => env.build_retained_cpi_trade(0, 1, 0, size, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(0, 1, 0, size, 0),
    }
}

fn land_capability_trade(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    tx: Transaction,
    request: &[GrantOracle],
    size: i128,
) {
    let current = history.replay();
    let fresh = request[0].epoch == current[0].epoch
        && request[1].epoch == current[1].epoch
        && request[1].sequence == current[1].sequence;
    let authorized =
        current[1].authorizes(request[1].scope, request[1].sequence, env.current_slot());
    let error = capability_step(
        env,
        history,
        fresh && authorized,
        Some(AuthorizationEvent::Fill {
            taker: 0,
            lp: 1,
            size,
            cpi: true,
        }),
        |env| env.land_retained(tx),
    );
    if let Some(error) = error {
        let expected = if fresh {
            PercolatorError::Unauthorized
        } else {
            PercolatorError::EngineStale
        };
        assert!(
            error.contains(&format!("Custom({})", expected as u32)),
            "wrong application rejection: {error}"
        );
    }
}

fn run_capability_history(case: CapabilityCase) {
    let mut env = V16Svm::new(
        [0x12; 32],
        MarketConfig {
            initial_price: 100,
            actor_deposits: [1_000_000; 5],
            actor_token_balances: [2_000_000; 5],
            ..MarketConfig::default()
        },
    );
    let mut history = AuthorizationHistory::new(&env);
    history.assert_prefix(&env);
    grant_capability(&mut env, &mut history, 1, Some(1), 4);
    let request = history.replay();
    let retained = retained_capability_trade(&mut env, case.route, case.size);
    // Same application consent, distinct unsent transport: never a duplicate-signature test.
    let sibling = retained_capability_trade(&mut env, case.route, case.size);
    let actor = match case.scope {
        WriterScope::Lp => 1,
        WriterScope::Taker => 0,
        WriterScope::Unrelated => 2,
    };
    match case.writer {
        CapabilityWriter::Deposit => {
            capability_step(
                &mut env,
                &mut history,
                true,
                Some(AuthorizationEvent::Deposit { actor }),
                |env| env.deposit_primary(actor, 1),
            );
        }
        CapabilityWriter::InvalidRenewal => {
            grant_capability(&mut env, &mut history, actor, Some(1), 1)
        }
        CapabilityWriter::Revoke => grant_capability(&mut env, &mut history, actor, None, 0),
        CapabilityWriter::Renew => grant_capability(&mut env, &mut history, actor, Some(1), 4),
        CapabilityWriter::LowerCap => grant_capability(&mut env, &mut history, actor, Some(0), 4),
        writer => {
            let (taker, lp) = match case.scope {
                WriterScope::Lp => (2, 1),
                WriterScope::Taker => (1, 2),
                WriterScope::Unrelated => (2, 3),
            };
            let cpi = matches!(
                writer,
                CapabilityWriter::SingleCpi | CapabilityWriter::BatchCpi
            );
            let tx = match writer {
                CapabilityWriter::SingleNoCpi => {
                    env.build_retained_no_cpi_trade(taker, lp, 0, case.size, 100)
                }
                CapabilityWriter::BatchNoCpi => {
                    env.build_retained_batch_no_cpi_trade(taker, lp, 0, case.size, 100)
                }
                CapabilityWriter::SingleCpi => {
                    env.build_retained_cpi_trade(taker, lp, 0, case.size, 0)
                }
                CapabilityWriter::BatchCpi => {
                    env.build_retained_batch_cpi_trade(taker, lp, 0, case.size, 0)
                }
                _ => unreachable!(),
            };
            capability_step(
                &mut env,
                &mut history,
                true,
                Some(AuthorizationEvent::Fill {
                    taker,
                    lp,
                    size: case.size,
                    cpi,
                }),
                |env| env.land_retained(tx),
            );
        }
    }
    env.warp_to_slot(case.landing_slot);
    history.assert_prefix(&env);
    land_capability_trade(&mut env, &mut history, retained, &request, case.size);
    land_capability_trade(&mut env, &mut history, sibling, &request, case.size);
    // This control changes only request freshness, never the grant under examination.
    let current_request = history.replay();
    let current = retained_capability_trade(&mut env, case.route, case.size);
    land_capability_trade(&mut env, &mut history, current, &current_request, case.size);
    grant_capability(&mut env, &mut history, 1, Some(1), case.landing_slot); // rejected exact-slot renewal
    grant_capability(&mut env, &mut history, 1, Some(1), case.landing_slot + 2);
    let fresh_request = history.replay();
    let fresh = retained_capability_trade(&mut env, case.route, -case.size);
    assert!(fresh_request[1].authorizes(
        fresh_request[1].scope,
        fresh_request[1].sequence,
        env.current_slot()
    ));
    land_capability_trade(&mut env, &mut history, fresh, &fresh_request, -case.size);
}

#[test]
fn v16_program_retained_capability_histories_preserve_authorization_scope() {
    let scopes = [WriterScope::Lp, WriterScope::Taker, WriterScope::Unrelated];
    for writer in CAPABILITY_WRITERS {
        for scope in scopes {
            for route in [CpiRoute::Single, CpiRoute::Batch] {
                for landing_slot in [3, 4, 5] {
                    for sign in [-1, 1] {
                        let case = CapabilityCase {
                            writer,
                            scope,
                            route,
                            landing_slot,
                            size: sign * POS_SCALE as i128,
                        };
                        std::panic::catch_unwind(|| run_capability_history(case))
                            .unwrap_or_else(|_| panic!("capability partition failed: {case:?}"));
                    }
                }
            }
        }
    }
    let strategy = (
        proptest::sample::select(CAPABILITY_WRITERS.to_vec()),
        proptest::sample::select(scopes.to_vec()),
        any::<bool>(),
        3u64..6,
        1i128..4,
        any::<bool>(),
    )
        .prop_map(
            |(writer, scope, batch, landing_slot, size, negative)| CapabilityCase {
                writer,
                scope,
                route: if batch {
                    CpiRoute::Batch
                } else {
                    CpiRoute::Single
                },
                landing_slot,
                size: size * POS_SCALE as i128 * if negative { -1 } else { 1 },
            },
        );
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 12,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x12; 32]),
    );
    runner
        .run(&strategy, |case| {
            run_capability_history(case);
            Ok(())
        })
        .unwrap();
    eprintln!("INV-012: 324 partition histories + 12 seeded shrinkable histories; gaps: lifecycle/keeper writers, tuple substitutions, nonzero fee/limit boundaries, multi-writer/multi-leg/max-shape histories");
}

#[test]
fn v16_capability_history_oracle_rejects_scope_invalidation_and_expiry_mistakes() {
    let created = GrantOracle {
        scope: std::array::from_fn(|i| Pubkey::new_from_array([i as u8; 32])),
        portfolio_id: 1,
        epoch: 0,
        sequence: 0,
        enabled: false,
        cap: 0,
        expiry: 0,
        position: 0,
    };
    let mut history = AuthorizationHistory {
        created: vec![created; 2],
        accepted: vec![AuthorizationEvent::Grant {
            actor: 1,
            cap: Some(1),
            expiry: 4,
        }],
    };
    let live = history.replay()[1];
    assert!(live.authorizes(live.scope, 1, 3));
    for lane in 0..7 {
        let mut wrong_scope = live.scope;
        wrong_scope[lane] = Pubkey::new_from_array([0xff; 32]);
        assert!(
            !live.authorizes(wrong_scope, 1, 3),
            "wrong-scope acceptance"
        );
    }
    assert!(
        !live.authorizes(live.scope, 0, 3),
        "old incarnation acceptance"
    );
    for slot in [4, 5] {
        assert!(
            !live.authorizes(live.scope, 1, slot),
            "exact/late expiry acceptance"
        );
    }
    history.accepted.push(AuthorizationEvent::Fill {
        taker: 1,
        lp: 0,
        size: 1,
        cpi: true,
    });
    let invalidated = history.replay()[1];
    assert_eq!(invalidated.epoch, live.epoch + 1);
    assert_eq!(invalidated.cap, live.cap);
    assert_eq!(invalidated.expiry, 0);
    assert!(
        !invalidated.authorizes(live.scope, 1, 3),
        "omitted taker invalidation"
    );
    assert_ne!(
        invalidated, live,
        "dropping the writer cannot preserve the expected history"
    );
}
