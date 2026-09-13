//! INV-012 / row 412: prefix conformance for interleaved grants and position writers.
//! Three-letter words compose a participating matcher, the same program's peer
//! context, bilateral mutation and explicit regrant. Admission comes from the
//! authorization journal; owner debits come from committed fill inputs.

use super::*;
use percolator_prog::ix::BatchTradeLeg;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRICE: u64 = 100;
const SIZE: i128 = 100 * POS_SCALE as i128;
const FEE_BPS: u16 = 1;
const DEPOSITS: [u128; 5] = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000];
const EXPIRY: u64 = 100;

#[derive(Clone, Copy, Debug)]
enum Writer {
    Renew,
    Matched,
    RoleSwitch,
    Bilateral,
}

#[derive(Clone, Copy)]
struct Request {
    taker: usize,
    lp: usize,
    asset: u16,
    size: i128,
    batch: bool,
    cpi: bool,
    bindings: [GrantOracle; 2],
}

impl Request {
    fn current(history: &AuthorizationHistory, taker: usize, lp: usize, batch: bool) -> Self {
        let grants = history.replay();
        Self {
            taker,
            lp,
            asset: 2,
            size: SIZE,
            batch,
            cpi: true,
            bindings: [grants[taker], grants[lp]],
        }
    }

    fn rejection(self, history: &AuthorizationHistory, slot: u64) -> Option<PercolatorError> {
        let current = history.replay();
        for (actor, bound) in [(self.taker, self.bindings[0]), (self.lp, self.bindings[1])] {
            if bound.portfolio_id != current[actor].portfolio_id
                || bound.epoch != current[actor].epoch
            {
                return Some(PercolatorError::EngineStale);
            }
        }
        if self.cpi {
            let bound = self.bindings[1];
            if bound.sequence != current[self.lp].sequence {
                return Some(PercolatorError::EngineStale);
            }
            if !current[self.lp].authorizes(bound.scope, bound.sequence, slot) {
                return Some(PercolatorError::Unauthorized);
            }
        }
        None
    }

    fn sign(self, env: &V16Svm, nonce: u32) -> Transaction {
        let [a, b] = self.bindings;
        let market_id = env.primary_market_state().1.assets[self.asset as usize].market_id;
        let instruction = match (self.cpi, self.batch) {
            (true, false) => ProgInstruction::TradeCpi {
                account_a_portfolio_id: a.portfolio_id,
                account_a_position_epoch: a.epoch,
                account_b_portfolio_id: b.portfolio_id,
                account_b_position_epoch: b.epoch,
                account_b_matcher_sequence: b.sequence,
                asset_index: self.asset,
                market_id,
                size_q: self.size,
                fee_bps: u64::from(FEE_BPS),
                limit_price: PRICE,
                backing_fee_cap_bps: 0,
            },
            (true, true) => ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id: a.portfolio_id,
                account_a_position_epoch: a.epoch,
                account_b_portfolio_id: b.portfolio_id,
                account_b_position_epoch: b.epoch,
                account_b_matcher_sequence: b.sequence,
                max_slippage_atoms: 0,
                max_fee_atoms: 2,
                legs: vec![BatchTradeCpiLeg {
                    asset_index: self.asset,
                    market_id,
                    size_q: self.size,
                    fee_bps: u64::from(FEE_BPS),
                    limit_price: PRICE,
                }],
            },
            (false, false) => ProgInstruction::TradeNoCpi {
                account_a_portfolio_id: a.portfolio_id,
                account_a_position_epoch: a.epoch,
                account_b_portfolio_id: b.portfolio_id,
                account_b_position_epoch: b.epoch,
                asset_index: self.asset,
                market_id,
                size_q: self.size,
                exec_price: PRICE,
                fee_bps: u64::from(FEE_BPS),
                backing_fee_cap_bps: 0,
            },
            (false, true) => ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id: a.portfolio_id,
                account_a_position_epoch: a.epoch,
                account_b_portfolio_id: b.portfolio_id,
                account_b_position_epoch: b.epoch,
                legs: vec![BatchTradeLeg {
                    asset_index: self.asset,
                    market_id,
                    size_q: self.size,
                    exec_price: PRICE,
                    fee_bps: u64::from(FEE_BPS),
                }],
            },
        };
        let payer = &env.actors[4].signer;
        let mut signers = vec![payer, &env.actors[self.taker].signer];
        let mut accounts = vec![AccountMeta::new(a.scope[3], true)];
        if !self.cpi {
            signers.push(&env.actors[self.lp].signer);
            accounts.push(AccountMeta::new(b.scope[3], true));
        }
        accounts.extend([
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.scope[2], false),
            AccountMeta::new(b.scope[2], false),
        ]);
        if self.cpi {
            accounts.extend([
                AccountMeta::new_readonly(b.scope[4], false),
                AccountMeta::new(b.scope[5], false),
                AccountMeta::new_readonly(b.scope[6], false),
            ]);
        }
        let tx = Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - nonce),
                Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: instruction.encode(),
                },
            ],
            Some(&payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert_eq!(tx.signatures.len(), if self.cpi { 2 } else { 3 });
        if self.cpi {
            assert!(
                !tx.message.account_keys[..2].contains(&b.scope[3]),
                "no LP owner signature"
            );
        }
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        tx
    }
}

#[derive(Default, Debug)]
struct Evidence {
    worlds: usize,
    fills: usize,
    stale: usize,
    disabled: usize,
    retained_live: usize,
    withdrawals: usize,
    max_cu: u64,
    nonce: u32,
}

fn values(env: &V16Svm, history: &AuthorizationHistory, paid: [u128; 5]) -> [u128; 5] {
    let mut fees = [0; 5];
    for event in &history.accepted {
        if let AuthorizationEvent::Fill {
            taker, lp, size, ..
        } = *event
        {
            let fee = (size.unsigned_abs() * u128::from(PRICE) * u128::from(FEE_BPS))
                .div_ceil(POS_SCALE * 10_000);
            assert_eq!(
                fee, 1,
                "every committed nonzero fill has an exact one-atom fee per owner"
            );
            fees[taker] += fee;
            fees[lp] += fee;
        }
    }
    let group = env.primary_market_state().1;
    let insurance = fees.iter().sum::<u128>();
    let capital = DEPOSITS.iter().sum::<u128>() - insurance - paid.iter().sum::<u128>();
    assert_eq!(
        (group.c_tot, group.insurance, group.vault),
        (capital, insurance, capital + insurance)
    );
    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
    let grants = history.replay();
    for asset in 0..ASSET_COUNT {
        let long = grants
            .iter()
            .map(|g| g.positions[asset].max(0) as u128)
            .sum::<u128>();
        let short = grants
            .iter()
            .map(|g| (-g.positions[asset]).max(0) as u128)
            .sum::<u128>();
        assert_eq!(long, short);
        assert_eq!(
            (
                group.assets[asset].oi_eff_long_q,
                group.assets[asset].oi_eff_short_q
            ),
            (long, short)
        );
    }
    for (actor, grant) in grants.iter().enumerate() {
        let p = env.primary_portfolio(actor);
        assert_eq!(p.owner, grant.scope[3].to_bytes());
        assert_eq!(p.capital.get(), DEPOSITS[actor] - fees[actor] - paid[actor]);
        assert_eq!(p.pnl.get(), 0);
        assert_eq!(env.token_amount(env.actors[actor].source_token), 0);
        assert_eq!(
            u128::from(env.token_amount(env.actors[actor].destination_token)),
            paid[actor]
        );
        let config =
            state::read_portfolio_matcher_config(&env.primary_portfolio_data(actor)).unwrap();
        assert_eq!(config.matcher_program, grant.scope[4].to_bytes());
        assert_eq!(config.matcher_context, grant.scope[5].to_bytes());
        assert_eq!(config.matcher_delegate, grant.scope[6].to_bytes());
    }
    fees
}

fn deliver(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    request: Request,
    tx: Transaction,
    evidence: &mut Evidence,
) -> bool {
    history.assert_prefix(env);
    values(env, history, [0; 5]);
    let expected = request.rejection(history, env.current_slot());
    let keys = capability_frame(env, history)
        .into_iter()
        .map(|(key, _)| key)
        .chain(tx.message.account_keys.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let before = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let payer = env.actors[4].signer.pubkey();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(ref error) = expected {
        let failure = result.expect_err("journal-denied capability must have no economic effect");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error.clone() as u32))
        );
        assert!(
            !failure
                .meta
                .logs
                .iter()
                .any(|log| log.starts_with(&format!("Program {} invoke", env.matcher_program))),
            "denial precedes matcher invocation"
        );
        match error {
            PercolatorError::EngineStale => evidence.stale += 1,
            PercolatorError::Unauthorized => evidence.disabled += 1,
            _ => unreachable!(),
        }
        failure.meta
    } else {
        let meta = result.expect("journal-authorized nonzero fill must remain executable");
        assert_eq!(
            meta.logs
                .iter()
                .filter(|log| **log == format!("Program {} success", env.matcher_program))
                .count(),
            usize::from(request.cpi)
        );
        history.accepted.push(AuthorizationEvent::Fill {
            taker: request.taker,
            lp: request.lp,
            asset: request.asset as usize,
            size: request.size,
            cpi: request.cpi,
        });
        evidence.fills += 1;
        meta
    };
    let allowed = [
        env.market,
        env.actors[request.taker].portfolio,
        env.actors[request.lp].portfolio,
    ];
    for (key, mut account) in before {
        if key == payer {
            account.as_mut().unwrap().lamports -= fee;
        } else if expected.is_none()
            && (allowed.contains(&key) || (request.cpi && key == request.bindings[1].scope[5]))
        {
            continue;
        }
        assert_eq!(
            env.svm.get_account(&key),
            account,
            "complete scope/rollback Account at {key}"
        );
    }
    assert!(meta.compute_units_consumed <= TX_CU_LIMIT);
    evidence.max_cu = evidence.max_cu.max(meta.compute_units_consumed);
    history.assert_prefix(env);
    values(env, history, [0; 5]);
    expected.is_none()
}

fn execute(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    request: Request,
    evidence: &mut Evidence,
) -> bool {
    evidence.nonce += 1;
    let tx = request.sign(env, evidence.nonce);
    deliver(env, history, request, tx, evidence)
}

fn run_word(word: [Writer; 3], batch: bool, sign: i128, evidence: &mut Evidence) {
    let mut env = V16Svm::new(
        [0x42; 32],
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: DEPOSITS,
            actor_token_balances: DEPOSITS.map(|v| v as u64),
            ..MarketConfig::default()
        },
    );
    let mut history = AuthorizationHistory::new(&env);
    env.update_trade_fee_policy(u64::from(FEE_BPS)).unwrap();
    for actor in [1, 2] {
        grant_capability(&mut env, &mut history, actor, Some(FEE_BPS), EXPIRY);
    }
    let original_grant = history.replay()[1];
    let mut first = Request::current(&history, 0, 1, batch);
    first.asset = 0;
    first.size *= sign;
    assert!(execute(&mut env, &mut history, first, evidence));

    for (index, writer) in word.into_iter().enumerate() {
        let route = batch ^ (index % 2 != 0);
        let mut retained = Request::current(&history, 0, 1, route);
        retained.size *= sign;
        evidence.nonce += 1;
        let tx = retained.sign(&env, evidence.nonce);
        let mut peer = Request::current(&history, 3, 2, !route);
        peer.size *= sign;
        evidence.nonce += 1;
        let peer_tx = peer.sign(&env, evidence.nonce);
        match writer {
            Writer::Renew => grant_capability(&mut env, &mut history, 1, Some(FEE_BPS), EXPIRY),
            _ => {
                let (taker, lp) = if matches!(writer, Writer::RoleSwitch) {
                    (1, 2)
                } else {
                    (0, 1)
                };
                let mut request = Request::current(&history, taker, lp, !route);
                request.asset = (index % 2) as u16;
                request.size *= sign;
                request.cpi = !matches!(writer, Writer::Bilateral);
                execute(&mut env, &mut history, request, evidence);
            }
        }
        evidence.retained_live +=
            usize::from(deliver(&mut env, &mut history, retained, tx, evidence));
        evidence.retained_live +=
            usize::from(deliver(&mut env, &mut history, peer, peer_tx, evidence));

        // Current episodes remove stale-position masking. Keep the original
        // grant sequence, then separately exercise the currently selected grant.
        let mut old_grant = Request::current(&history, 0, 1, !route);
        old_grant.size *= sign;
        old_grant.bindings[1].sequence = original_grant.sequence;
        execute(&mut env, &mut history, old_grant, evidence);
        let mut current = Request::current(&history, 0, 1, route);
        current.size *= sign;
        execute(&mut env, &mut history, current, evidence);
    }

    // A final owner regrant must restore both consumers without reviving the
    // old grant even in the later, freshly used position episode.
    grant_capability(&mut env, &mut history, 1, Some(FEE_BPS), EXPIRY);
    for route in [batch, !batch] {
        let mut current = Request::current(&history, 0, 1, route);
        current.size *= sign;
        assert!(execute(&mut env, &mut history, current, evidence));
        let mut old = Request::current(&history, 0, 1, !route);
        old.bindings[1].sequence = original_grant.sequence;
        assert!(!execute(&mut env, &mut history, old, evidence));
    }

    let fills = history.accepted.clone();
    for event in fills.iter().rev() {
        if let AuthorizationEvent::Fill {
            taker,
            lp,
            asset,
            size,
            ..
        } = *event
        {
            let mut exit = Request::current(&history, taker, lp, !batch);
            exit.cpi = false;
            exit.asset = asset as u16;
            exit.size = -size;
            assert!(execute(&mut env, &mut history, exit, evidence));
        }
    }
    assert!(history
        .replay()
        .iter()
        .all(|g| g.positions == [0; ASSET_COUNT]));
    let fees = values(&env, &history, [0; 5]);
    let mut paid = [0; 5];
    for actor in 0..5 {
        let before = env.primary_portfolio_matcher_sequence(actor);
        env.begin_public_trace();
        env.withdraw_primary(actor, DEPOSITS[actor] - fees[actor])
            .unwrap();
        env.finish_public_trace()
            .validate_public_execution()
            .unwrap();
        paid[actor] = DEPOSITS[actor] - fees[actor];
        assert_eq!(env.primary_portfolio_matcher_sequence(actor), before + 1);
        values(&env, &history, paid);
        evidence.withdrawals += 1;
    }
    assert_eq!(env.primary_market_state().1.c_tot, 0);
    evidence.worlds += 1;
}

#[test]
fn v16_program_interleaved_revocation_words_bind_every_retained_prefix_and_owner_value() {
    let writers = [
        Writer::Renew,
        Writer::Matched,
        Writer::RoleSwitch,
        Writer::Bilateral,
    ];
    let mut evidence = Evidence::default();
    for first in writers {
        for second in writers {
            for third in writers {
                for batch in [false, true] {
                    for sign in [-1, 1] {
                        let word = [first, second, third];
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            run_word(word, batch, sign, &mut evidence)
                        }))
                        .unwrap_or_else(|_| {
                            panic!("revocation word failed: {word:?}, batch={batch}, sign={sign}")
                        });
                    }
                }
            }
        }
    }
    assert_eq!(evidence.worlds, 256);
    assert_eq!(evidence.withdrawals, 1_280);
    assert!(evidence.stale > 0 && evidence.disabled > 0 && evidence.retained_live > 0);
    eprintln!("INV-012 interleaved revocation words: {evidence:?}; row 412 OPEN");
}
