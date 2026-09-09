//! INV-012 / row 412: non-position keeper writers preserve retained authority.
//!
//! Fee debits and healthy mark settlement change economic state, but neither is
//! owner reauthorization or an out-of-matcher position mutation. Cross their
//! order and portfolio scope under the common append-only authorization oracle.
//! This is not owner-episode revocation or retained SetMatcherConfig re-delivery.

use super::*;
use percolator_prog::ix::CrankObservationHint;

const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const FEE: u128 = 7;
const QUANTITY: i128 = 12 * POS_SCALE as i128;
const MARK_MOVE: u64 = 5;
const MARK_VALUE: i128 = QUANTITY / POS_SCALE as i128 * MARK_MOVE as i128;
const LP: usize = 1;
const COUNTERPARTY: usize = 3;
const CONSUMER_ASSET: u16 = 1;

#[derive(Clone, Copy, Debug)]
enum KeeperWriter {
    Fee,
    Settlement,
}

#[derive(Default, Debug)]
struct Evidence {
    histories: usize,
    transactions: usize,
    fee_debits: usize,
    settlements: usize,
    fee_retries: usize,
    live_simulations: usize,
    retained_fills: usize,
    max_cu: u64,
    max_keeper_cu: u64,
    max_simulation_cu: u64,
}

fn step(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    evidence: &mut Evidence,
    event: Option<AuthorizationEvent>,
    keeper: bool,
    execute: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) {
    capability_step(env, history, true, event, |env| {
        let result = execute(env);
        if let Ok(success) = &result {
            evidence.max_cu = evidence.max_cu.max(success.compute_units);
            if keeper {
                evidence.max_keeper_cu = evidence.max_keeper_cu.max(success.compute_units);
            }
        }
        result
    });
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
        .expect("unchanged retained capability must admit a nonzero CPI fill");
    assert!(meta.compute_units_consumed <= TX_CU_LIMIT);
    assert!(capability_frame(env, history) == before, "simulation frame");
    evidence.live_simulations += 1;
    evidence.max_simulation_cu = evidence.max_simulation_cu.max(meta.compute_units_consumed);
}

fn equity(env: &V16Svm, actor: usize) -> i128 {
    let portfolio = env.primary_portfolio(actor);
    i128::try_from(portfolio.capital.get()).unwrap() + portfolio.pnl.get()
}

fn observations(assets: &[u16]) -> Vec<CrankObservationHint> {
    assets
        .iter()
        .map(|asset| CrankObservationHint {
            asset_index: *asset,
            oracle_accounts: 0,
        })
        .collect()
}

fn run_case(
    subject: usize,
    route: CpiRoute,
    sign: i128,
    writers: [KeeperWriter; 2],
    evidence: &mut Evidence,
) {
    let mut env = V16Svm::new(
        [0x6c; 32],
        MarketConfig {
            initial_price: PRICE,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: FEE,
            actor_deposits: [CAPITAL; 5],
            actor_token_balances: [2_000_000; 5],
            ..MarketConfig::default()
        },
    );
    let mut history = AuthorizationHistory::new(&env);
    step(&mut env, &mut history, evidence, None, false, |env| {
        env.configure_auth_mark(false, 0, 1, PRICE)
    });
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
        false,
        |env| env.trade_no_cpi(subject, COUNTERPARTY, 0, sign * QUANTITY, PRICE, 0),
    );
    for actor in [0, LP, 2] {
        grant_capability(&mut env, &mut history, actor, Some(37), 100);
        evidence.transactions += 1;
    }
    assert_eq!(equity(&env, subject), CAPITAL as i128);
    assert_eq!(env.primary_portfolio(subject).last_fee_slot.get(), 1);

    let size = sign * POS_SCALE as i128;
    let retained = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(0, LP, CONSUMER_ASSET, size, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(0, LP, CONSUMER_ASSET, size, 0),
    };
    let retained_bytes = bincode::serialize(&retained).unwrap();
    let request = history.replay();
    let matcher_before = env.all_matcher_context_data();
    let tokens_before = env.all_token_account_data();
    simulate_live(&mut env, &history, evidence, &retained);

    let mut charged = 0;
    let mut settled = false;
    for (index, writer) in writers.into_iter().enumerate() {
        let slot = 2 + index as u64;
        let price = if settled || matches!(writer, KeeperWriter::Settlement) {
            PRICE + MARK_MOVE
        } else {
            PRICE
        };
        env.warp_to_slot(slot);
        step(&mut env, &mut history, evidence, None, false, |env| {
            env.push_auth_mark(0, slot, price)
        });
        step(&mut env, &mut history, evidence, None, false, |env| {
            env.push_auth_mark(CONSUMER_ASSET, slot, PRICE)
        });
        // Fees are bounded by active-asset frontiers, not Clock alone. The
        // unrelated crank advances those frontiers without settling the subject.
        step(&mut env, &mut history, evidence, None, true, |env| {
            env.crank(4, slot, observations(&[0, CONSUMER_ASSET]))
        });
        let other_portfolios: Vec<_> = (0..env.actors.len())
            .filter(|actor| *actor != subject)
            .map(|actor| {
                let key = env.actors[actor].portfolio;
                (key, env.svm.get_account(&key).unwrap())
            })
            .collect();
        let due = u128::from(slot - 1) * FEE - charged;
        match writer {
            KeeperWriter::Fee => {
                let before = equity(&env, subject);
                // Classify this writer as authority-preserving independently of
                // observed grant bits: the authorization journal is unchanged.
                step(&mut env, &mut history, evidence, None, true, |env| {
                    env.sync_maintenance_fee(subject, slot)
                });
                assert_eq!(equity(&env, subject), before - due as i128);
                evidence.fee_debits += 1;

                let before = capability_frame(&env, &history);
                step(&mut env, &mut history, evidence, None, true, |env| {
                    env.sync_maintenance_fee(subject, slot)
                });
                assert!(
                    capability_frame(&env, &history) == before,
                    "fee retry frame"
                );
                evidence.fee_retries += 1;
            }
            KeeperWriter::Settlement => {
                step(&mut env, &mut history, evidence, None, true, |env| {
                    env.crank(subject, slot, observations(&[0]))
                });
                settled = true;
                evidence.settlements += 1;
            }
        }
        // Both public routes collect elapsed maintenance. Settlement additionally
        // realizes the signed mark delta; neither consumes an authority episode.
        charged += due;
        assert_eq!(env.primary_portfolio(subject).last_fee_slot.get(), slot);
        assert_eq!(
            equity(&env, subject),
            CAPITAL as i128 + if settled { sign * MARK_VALUE } else { 0 } - charged as i128,
            "input-derived economic effect after {writer:?}"
        );
        assert_eq!(history.replay(), request, "keepers are not grant events");
        assert_eq!(env.all_matcher_context_data(), matcher_before);
        assert_eq!(env.all_token_account_data(), tokens_before);
        for (key, account) in other_portfolios {
            assert_eq!(
                env.svm.get_account(&key).unwrap(),
                account,
                "untouched scope"
            );
        }
    }

    // Both writer orders reach the same fee horizon, including collection by
    // the settlement crank. Dedicated synchronization must not debit it again.
    let before = capability_frame(&env, &history);
    step(&mut env, &mut history, evidence, None, true, |env| {
        env.sync_maintenance_fee(subject, 3)
    });
    assert!(
        capability_frame(&env, &history) == before,
        "final fee retry frame"
    );
    evidence.fee_retries += 1;
    assert_eq!(env.primary_portfolio(subject).last_fee_slot.get(), 3);
    assert_eq!(
        equity(&env, subject),
        CAPITAL as i128 + sign * MARK_VALUE - 2 * FEE as i128
    );

    // Advance only public certificates before exercising the retained consumer
    // on the sibling asset; no grant or signed episode fields are refreshed.
    step(&mut env, &mut history, evidence, None, true, |env| {
        env.crank(COUNTERPARTY, 3, observations(&[0]))
    });
    if matches!(writers[1], KeeperWriter::Fee) {
        step(&mut env, &mut history, evidence, None, true, |env| {
            env.crank(subject, 3, observations(&[0]))
        });
    }
    for actor in [0, LP].into_iter().filter(|actor| *actor != subject) {
        step(&mut env, &mut history, evidence, None, true, |env| {
            env.crank(actor, 3, observations(&[CONSUMER_ASSET]))
        });
    }
    assert_eq!(history.replay(), request);
    assert_eq!(env.all_matcher_context_data(), matcher_before);
    assert_eq!(env.all_token_account_data(), tokens_before);
    assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
    simulate_live(&mut env, &history, evidence, &retained);
    let unrelated = env.actors[2].portfolio;
    let unrelated_before = env.svm.get_account(&unrelated).unwrap();
    step(
        &mut env,
        &mut history,
        evidence,
        Some(AuthorizationEvent::Fill {
            taker: 0,
            lp: LP,
            asset: CONSUMER_ASSET as usize,
            size,
            cpi: true,
        }),
        false,
        |env| env.land_retained(retained),
    );
    assert_eq!(env.svm.get_account(&unrelated).unwrap(), unrelated_before);
    assert_eq!(env.all_token_account_data(), tokens_before);
    evidence.retained_fills += 1;
    evidence.histories += 1;
}

#[test]
fn v16_program_retained_capability_survives_non_position_keeper_words() {
    let mut evidence = Evidence::default();
    for subject in [0, LP, 2] {
        for route in [CpiRoute::Single, CpiRoute::Batch] {
            for sign in [-1, 1] {
                for writers in [
                    [KeeperWriter::Fee, KeeperWriter::Settlement],
                    [KeeperWriter::Settlement, KeeperWriter::Fee],
                ] {
                    eprintln!("keeper world: subject={subject}, route={route:?}, sign={sign}, writers={writers:?}");
                    run_case(subject, route, sign, writers, &mut evidence);
                }
            }
        }
    }
    assert_eq!(evidence.histories, 24);
    assert_eq!(evidence.transactions, 452);
    assert_eq!(evidence.fee_debits, 24);
    assert_eq!(evidence.settlements, 24);
    assert_eq!(evidence.fee_retries, 48);
    assert_eq!(evidence.live_simulations, 48);
    assert_eq!(evidence.retained_fills, 24);
    assert!(evidence.max_cu <= 300_000);
    assert!(evidence.max_simulation_cu <= 300_000);
    println!("INV-012 non-position keeper product: {evidence:?}");
}
