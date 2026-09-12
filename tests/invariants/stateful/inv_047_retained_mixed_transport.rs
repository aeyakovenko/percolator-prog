//! INV-019/047: retained grants, occupied foreign contexts, and mixed trade transports.
//! A successful prior matcher response cannot expand another LP's grant. Bilateral reductions
//! preserve normalized economics while revoking the capability used by the earlier CPI fill.

use super::*;
use crate::support::fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census};
use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    signature::Keypair,
    transaction::{Transaction, TransactionError},
};

const PRICE: u64 = 100;
const FEE_BPS: u16 = 100;
const DEPOSIT: u128 = 1_000_000;

fn retain(env: &mut V16Svm, route: TradeRoute, size: i128) -> Transaction {
    let tx = match route {
        TradeRoute::NoCpi => {
            env.build_retained_no_cpi_trade_with_fee(0, 1, 0, size, PRICE, u64::from(FEE_BPS))
        }
        TradeRoute::Cpi => env.build_retained_cpi_trade(0, 1, 0, size, PRICE),
        TradeRoute::BatchNoCpi => {
            env.build_retained_batch_no_cpi_trade_with_fee(0, 1, 0, size, PRICE, u64::from(FEE_BPS))
        }
        TradeRoute::BatchCpi => env.build_retained_batch_cpi_trade(0, 1, 0, size, PRICE),
    };
    tx.verify().expect("retained transaction signatures");
    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
    tx
}

fn frame(env: &V16Svm, normalize: bool) -> Vec<(Pubkey, Account)> {
    let mut accounts = hint_route_accounts(env, normalize);
    let clock = solana_sdk::sysvar::clock::ID;
    accounts.push((clock, env.svm.get_account(&clock).expect("public Clock")));
    accounts
}

fn matcher_calls(logs: &[String], matcher: Pubkey) -> usize {
    let invoke = format!("Program {matcher} invoke [2]");
    let success = format!("Program {matcher} success");
    let calls = logs.iter().filter(|log| **log == invoke).count();
    assert_eq!(calls, logs.iter().filter(|log| **log == success).count());
    calls
}

fn reject(env: &mut V16Svm, tx: Transaction, peak_cu: &mut u64) {
    tx.verify().unwrap();
    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
    let before = frame(env, false);
    let instruction_accounts: Vec<_> = tx.message.account_keys[1..]
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let failed = env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect_err("the current-epoch request lacks the required LP capability");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(PercolatorError::Unauthorized as u32)
        )
    );
    assert_eq!(matcher_calls(&failed.meta.logs, env.matcher_program), 0);
    *peak_cu = (*peak_cu).max(failed.meta.compute_units_consumed);
    assert!(frame(env, false) == before, "simulation changed accounts");
    let error = env
        .land_retained(tx)
        .expect_err("public capability refusal");
    assert!(
        error.contains(&format!(
            "InstructionError(3, Custom({}))",
            PercolatorError::Unauthorized as u32
        )),
        "unexpected public error: {error}"
    );
    assert!(!error.contains(&format!("Program {} invoke [2]", env.matcher_program)));
    assert!(
        frame(env, false) == before,
        "exact economic Account rollback"
    );
    for (key, account) in instruction_accounts {
        assert!(
            env.svm.get_account(&key) == account,
            "instruction account {key} changed"
        );
    }
}

fn context_request(
    env: &V16Svm,
    template: &Transaction,
    payer: &Keypair,
    context_actor: usize,
) -> Transaction {
    let maker = &env.actors[1];
    let context = env.actors[context_actor].matcher_context;
    // Derive the correct PDA for the substituted context so rejection must reach the grant
    // tuple check, rather than merely rejecting the other portfolio's delegate PDA.
    let delegate = Pubkey::find_program_address(
        &[
            b"matcher",
            env.market.as_ref(),
            maker.portfolio.as_ref(),
            maker.signer.pubkey().as_ref(),
            env.matcher_program.as_ref(),
            context.as_ref(),
        ],
        &env.program_id,
    )
    .0;
    if context_actor == 1 {
        assert_eq!(delegate, maker.matcher_delegate);
    } else {
        assert_ne!(delegate, maker.matcher_delegate);
        assert_ne!(delegate, env.actors[context_actor].matcher_delegate);
    }
    let payload = template.message.instructions.last().unwrap().data.clone();
    Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
            ComputeBudgetInstruction::set_compute_unit_price(1),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[0].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[0].portfolio, false),
                    AccountMeta::new(maker.portfolio, false),
                    AccountMeta::new_readonly(env.matcher_program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                data: payload,
            },
        ],
        Some(&payer.pubkey()),
        &[payer, &env.actors[0].signer],
        env.svm.latest_blockhash(),
    )
}

#[test]
fn v16_program_retained_grants_bind_context_across_mixed_transport_reductions() {
    let mut worlds = 0;
    let mut rejections = 0;
    let mut successes = 0;
    let mut peak_cu = 0;
    for direction in [-1, 1] {
        let mut expected_prefix = None;
        let mut expected_result = None;
        for route in ROUTES {
            let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
            let cpi_route = match route {
                TradeRoute::BatchCpi | TradeRoute::BatchNoCpi => TradeRoute::BatchCpi,
                _ => TradeRoute::Cpi,
            };
            let label = format!("{route:?}/direction={direction}");
            let mut env = V16Svm::new(
                [0x97; 32],
                MarketConfig {
                    initial_price: PRICE,
                    actor_deposits: [DEPOSIT; 5],
                    actor_token_balances: [2_000_000; 5],
                    ..MarketConfig::default()
                },
            );
            let payer = Keypair::new();
            env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
            env.begin_public_trace();
            env.update_trade_fee_policy(u64::from(FEE_BPS)).unwrap();
            for lp in [1, 3] {
                env.set_matcher_config(lp, 0).unwrap();
            }
            let sequences = [1, 3].map(|lp| env.primary_portfolio_matcher_sequence(lp));
            let grants = [1, 3]
                .map(|lp| env.build_retained_matcher_config_with_trade_fee_cap(lp, 1, FEE_BPS));
            for grant in &grants {
                grant.verify().unwrap();
                assert!(bincode::serialize(grant).unwrap().len() <= 1232);
                let before = frame(&env, false);
                env.svm.simulate_transaction(grant.clone().into()).unwrap();
                assert!(frame(&env, false) == before);
            }
            // Public custody activity separates signing from delivery without changing either LP.
            env.deposit_primary(4, 17).unwrap();
            for (index, grant) in grants.into_iter().enumerate() {
                env.land_retained(grant).unwrap();
                assert_eq!(
                    env.primary_portfolio_matcher_sequence(2 * index + 1),
                    sequences[index] + 1
                );
            }
            for (taker, maker, asset, size) in [
                (0, 1, 0, direction * 7 * POS_SCALE as i128),
                (2, 3, 1, -direction * 3 * POS_SCALE as i128),
            ] {
                let opening = env.build_retained_cpi_trade(taker, maker, asset, size, PRICE);
                opening.verify().unwrap();
                let before = frame(&env, false);
                let simulated = env
                    .svm
                    .simulate_transaction(opening.clone().into())
                    .unwrap();
                assert_eq!(matcher_calls(&simulated.logs, env.matcher_program), 1);
                assert!(frame(&env, false) == before);
                env.land_retained(opening).unwrap();
                let context = env
                    .svm
                    .get_account(&env.actors[maker].matcher_context)
                    .unwrap();
                let response = read_matcher_return(&context.data).unwrap();
                assert_eq!(
                    response.req_id,
                    env.primary_market_state().0.matcher_req_seq
                );
                assert_eq!(response.asset_index, u64::from(asset));
                assert_eq!(response.exec_size, size);
                assert_eq!(response.oracle_price_e6, PRICE);
                assert_eq!(response.exec_price_e6, PRICE);
                assert_eq!(
                    response.lp_account_id,
                    u64::from_le_bytes(
                        env.actors[maker].matcher_delegate.to_bytes()[..8]
                            .try_into()
                            .unwrap()
                    )
                );
            }
            let before = frame(&env, false);
            if let Some(prefix) = &expected_prefix {
                assert!(&before == prefix, "{label}: inputs must be byte-identical");
            } else {
                expected_prefix = Some(before.clone());
            }
            let size = -direction * 2 * POS_SCALE as i128;
            let retained = retain(&mut env, route, size);
            let live = env
                .svm
                .simulate_transaction(retained.clone().into())
                .unwrap();
            assert_eq!(
                matcher_calls(&live.logs, env.matcher_program),
                usize::from(cpi)
            );
            let scoped_cpi = retain(&mut env, cpi_route, size);
            let valid = env
                .svm
                .simulate_transaction(scoped_cpi.clone().into())
                .unwrap();
            assert_eq!(matcher_calls(&valid.logs, env.matcher_program), 1);
            assert!(frame(&env, false) == before);
            let wrong_context = context_request(&env, &scoped_cpi, &payer, 3);
            assert_eq!(
                wrong_context.message.instructions.last().unwrap().data,
                scoped_cpi.message.instructions.last().unwrap().data,
                "only the context/delegate accounts and transport payer differ"
            );
            reject(&mut env, wrong_context, &mut peak_cu);

            let contexts = env.all_matcher_context_data();
            let before_trade = env.primary_market_state().1;
            let landed = env
                .land_retained(retained)
                .expect("original signed reduction remains usable");
            peak_cu = peak_cu.max(landed.compute_units);
            let after = env.primary_market_state().1;
            assert_eq!(after.vault, before_trade.vault);
            assert_eq!(after.c_tot, before_trade.c_tot - 4);
            assert_eq!(after.insurance, before_trade.insurance + 4);
            assert_eq!(after.vault, after.c_tot + after.insurance);
            assert_eq!(u128::from(env.token_amount(env.vault)), after.vault);
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            assert_eq!(
                env.primary_market_state().0.matcher_req_seq,
                2 + u64::from(cpi)
            );
            for actor in [0, 1] {
                let portfolio = env.primary_portfolio(actor);
                assert_eq!(portfolio.capital.get(), DEPOSIT - 9);
                assert_eq!(portfolio.pnl.get(), 0);
                assert_eq!(env.primary_portfolio_position_epoch(actor), 2);
                let active: Vec<_> = portfolio
                    .legs
                    .iter()
                    .map(|leg| leg.try_to_runtime().unwrap())
                    .filter(|leg| leg.active)
                    .map(|leg| (leg.asset_index, leg.basis_pos_q))
                    .collect();
                assert_eq!(
                    active,
                    vec![(
                        0,
                        if actor == 0 { direction } else { -direction } * 5 * POS_SCALE as i128
                    )]
                );
            }
            assert_eq!(after.assets[0].oi_eff_long_q, 5 * POS_SCALE);
            assert_eq!(after.assets[0].oi_eff_short_q, 5 * POS_SCALE);
            let maker = env.primary_portfolio_data(1);
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
            assert_eq!(env.primary_portfolio_matcher_sequence(1), sequences[0] + 1);
            let taker = env.primary_portfolio_data(0);
            assert_eq!(
                state::read_portfolio_matcher_config(&taker)
                    .unwrap()
                    .enabled(),
                0
            );
            assert_eq!(state::read_portfolio_matcher_expiry(&taker).unwrap(), 0);
            let changed = [
                env.market,
                env.actors[0].portfolio,
                env.actors[1].portfolio,
                env.actors[1].matcher_context,
            ];
            for (key, account) in &before {
                if !changed.contains(key) {
                    assert!(
                        env.svm.get_account(key).as_ref() == Some(account),
                        "{label}: unrelated account changed"
                    );
                }
            }
            let current_contexts = env.all_matcher_context_data();
            if matches!(route, TradeRoute::Cpi) {
                assert_eq!(
                    current_contexts[1][MATCHER_RETURN_BYTES..],
                    contexts[1][MATCHER_RETURN_BYTES..]
                );
                let response = read_matcher_return(&current_contexts[1]).unwrap();
                assert_eq!(response.req_id, 3);
                assert_eq!(response.exec_size, size);
            } else {
                assert_eq!(current_contexts, contexts);
            }

            // Refresh only the request's episode binding. The original owner grant remains
            // live after CPI, but bilateral consent revoked it despite the intact prior return.
            let probe = retain(&mut env, cpi_route, -direction * POS_SCALE as i128);
            if cpi {
                let before = frame(&env, false);
                let live = env.svm.simulate_transaction(probe.into()).unwrap();
                assert_eq!(matcher_calls(&live.logs, env.matcher_program), 1);
                assert!(frame(&env, false) == before);
            } else {
                // LiteSVM charges the payer even for failed simulations. Keep those network
                // fees outside the traced economic accounts, just as for the context refusal.
                let probe = context_request(&env, &probe, &payer, 1);
                reject(&mut env, probe, &mut peak_cu);
            }
            assert_public_stock_census(&label, &env).unwrap();
            assert_public_encumbrance_census(&label, &env).unwrap();
            let normalized = frame(&env, true);
            if let Some(expected) = &expected_result {
                assert!(
                    &normalized == expected,
                    "{label}: mixed transport economics diverged"
                );
            } else {
                expected_result = Some(normalized);
            }
            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("public history and exact rollback");
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(
                trace.steps.iter().filter(|step| !step.succeeded).count(),
                if cpi { 1 } else { 2 }
            );
            for step in trace.steps {
                if let Some(cu) = step.compute_units {
                    successes += 1;
                    peak_cu = peak_cu.max(cu);
                } else {
                    rejections += 1;
                    assert_eq!(step.rejected_exact_writable_rollback, Some(true));
                    assert_eq!(step.rejected_no_program_lamport_delta, Some(true));
                }
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, successes, rejections), (8, 72, 12));
    assert!(peak_cu < TX_CU_LIMIT);
    println!("INV-019/047 retained mixed transports: worlds={worlds}, accepted_public_txs={successes}, exact_rejections={rejections}, peak_cu={peak_cu}");
}
