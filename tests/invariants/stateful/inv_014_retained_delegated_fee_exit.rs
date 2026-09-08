//! INV-014: retained fee-bearing exits with distinct bilateral and delegated consent.
//!
//! A policy above the LP's standing cap but below both owners' separately signed
//! bilateral fee must block delegated execution, not the bilateral exit. Lowering
//! policy to/below the unchanged LP cap restores a pre-signed CPI alternative.
//! The taker's looser fee/aggregate caps cannot authorize the LP's debit.
//!
//! This is a 24-world public composition, not a new engine fee proof or row-411
//! closure. Authenticated constant prices, zero spread/funding/maintenance, one
//! active leg and zero backing fees isolate each owner's exact fee entitlement.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census, TradeRoute},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg, Instruction as ProgInstruction},
    state::read_portfolio_matcher_config,
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    signature::{Keypair, SeedDerivable, Signer},
    transaction::Transaction,
};

const TAKER: usize = 0;
const LP: usize = 1;
const PRICE: u64 = 100_003;
const LP_CAP: u64 = 37;
const SIGNED_BPS: u64 = 503;
const OLD_POLICY: u64 = 19;
const LOWER_POLICY: u64 = 7;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchCpi,
];

fn is_cpi(route: TradeRoute) -> bool {
    matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)
}

fn fee_atoms(quantity: i128, bps: u64) -> u128 {
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let notional = ceil(quantity.unsigned_abs() * u128::from(PRICE), POS_SCALE);
    ceil(notional * u128::from(bps), 10_000)
}

fn retain_exit(
    env: &V16Svm,
    payer: &Keypair,
    route: TradeRoute,
    asset: u16,
    size_q: i128,
    nonce: u64,
) -> Transaction {
    let a_id = env.primary_portfolio_id(TAKER);
    let a_epoch = env.primary_portfolio_position_epoch(TAKER);
    let b_id = env.primary_portfolio_id(LP);
    let b_epoch = env.primary_portfolio_position_epoch(LP);
    let market_id = env.primary_market_state().1.assets[asset as usize].market_id;
    let matcher_sequence = env.primary_portfolio_matcher_sequence(LP);
    let instruction = match route {
        TradeRoute::NoCpi => ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: a_id,
            account_a_position_epoch: a_epoch,
            account_b_portfolio_id: b_id,
            account_b_position_epoch: b_epoch,
            asset_index: asset,
            market_id,
            size_q,
            exec_price: PRICE,
            fee_bps: SIGNED_BPS,
            backing_fee_cap_bps: 0,
        },
        TradeRoute::BatchNoCpi => ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: a_id,
            account_a_position_epoch: a_epoch,
            account_b_portfolio_id: b_id,
            account_b_position_epoch: b_epoch,
            legs: vec![BatchTradeLeg {
                asset_index: asset,
                market_id,
                size_q,
                exec_price: PRICE,
                fee_bps: SIGNED_BPS,
            }],
        },
        TradeRoute::Cpi => ProgInstruction::TradeCpi {
            account_a_portfolio_id: a_id,
            account_a_position_epoch: a_epoch,
            account_b_portfolio_id: b_id,
            account_b_position_epoch: b_epoch,
            account_b_matcher_sequence: matcher_sequence,
            asset_index: asset,
            market_id,
            size_q,
            fee_bps: SIGNED_BPS,
            limit_price: PRICE,
            backing_fee_cap_bps: 0,
        },
        TradeRoute::BatchCpi => ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: a_id,
            account_a_position_epoch: a_epoch,
            account_b_portfolio_id: b_id,
            account_b_position_epoch: b_epoch,
            account_b_matcher_sequence: matcher_sequence,
            max_slippage_atoms: 0,
            max_fee_atoms: fee_atoms(size_q, SIGNED_BPS),
            legs: vec![BatchTradeCpiLeg {
                asset_index: asset,
                market_id,
                size_q,
                fee_bps: SIGNED_BPS,
                limit_price: PRICE,
            }],
        },
    };
    let mut accounts = vec![AccountMeta::new(env.actors[TAKER].signer.pubkey(), true)];
    let mut signers = vec![payer, &env.actors[TAKER].signer];
    if !is_cpi(route) {
        accounts.push(AccountMeta::new(env.actors[LP].signer.pubkey(), true));
        signers.push(&env.actors[LP].signer);
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(env.actors[TAKER].portfolio, false),
        AccountMeta::new(env.actors[LP].portfolio, false),
    ]);
    if is_cpi(route) {
        accounts.extend([
            AccountMeta::new_readonly(env.matcher_program, false),
            AccountMeta::new(env.actors[LP].matcher_context, false),
            AccountMeta::new_readonly(env.actors[LP].matcher_delegate, false),
        ]);
    }
    // Distinct pre-signed deliveries avoid signature-cache rejection masking an
    // instruction's consent/episode guard. No retained message is rebound later.
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
            ComputeBudgetInstruction::set_compute_unit_price(nonce),
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
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    assert_eq!(
        tx.message.header.num_required_signatures,
        if is_cpi(route) { 2 } else { 3 }
    );
    tx
}

#[test]
fn v16_program_retained_lp_fee_cap_preserves_bilateral_and_delegated_exits() {
    let mut worlds = 0;
    let mut transactions = 0;
    let mut rejections = 0;
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            // Same-route single/batch outcomes must have identical SPL entitlements.
            let mut terminal_references = [None, None, None];
            for (route, policy, reference_index) in [
                (TradeRoute::NoCpi, LP_CAP + 1, 0),
                (TradeRoute::BatchNoCpi, LP_CAP + 1, 0),
                (TradeRoute::Cpi, LP_CAP, 1),
                (TradeRoute::BatchCpi, LP_CAP, 1),
                (TradeRoute::Cpi, LOWER_POLICY, 2),
                (TradeRoute::BatchCpi, LOWER_POLICY, 2),
            ] {
                let label = format!(
                    "asset={asset}, direction={direction}, route={route:?}, policy={policy}"
                );
                let config = MarketConfig {
                    initial_price: PRICE,
                    ..MarketConfig::default()
                };
                let mut env = V16Svm::new([0xd4; 32], config);
                let payer = Keypair::from_seed(&[0x14; 32]).unwrap();
                env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                let quantity = direction * (3 * POS_SCALE + 7) as i128;
                let initial_capital: u128 = config.actor_deposits.iter().sum();
                let supply = env.token_supply_observed();
                let mut paid = [0u128; PRIMARY_ACTOR_COUNT];
                let check = |env: &V16Svm,
                             position: i128,
                             fee: u128,
                             paid: &[u128; PRIMARY_ACTOR_COUNT]| {
                    assert_public_stock_census(&label, env).unwrap();
                    assert_public_encumbrance_census(&label, env).unwrap();
                    let group = env.primary_market_state().1;
                    assert_eq!(
                        group.vault,
                        initial_capital - paid.iter().sum::<u128>(),
                        "{label}"
                    );
                    assert_eq!(group.insurance, 2 * fee, "{label}");
                    assert_eq!(group.c_tot, group.vault - group.insurance, "{label}");
                    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                    assert_eq!(env.token_supply_observed(), supply);
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let portfolio = env.primary_portfolio(actor);
                        let debit = if [TAKER, LP].contains(&actor) { fee } else { 0 };
                        assert_eq!(
                            portfolio.capital.get(),
                            config.actor_deposits[actor] - debit - paid[actor],
                            "{label}: actor {actor}"
                        );
                        assert_eq!(portfolio.pnl.get(), 0, "{label}: no PnL disguises a fee");
                        assert_eq!(
                            u128::from(env.token_amount(env.actors[actor].destination_token)),
                            paid[actor]
                        );
                        assert_eq!(
                            u128::from(env.token_amount(env.actors[actor].source_token)),
                            u128::from(config.actor_token_balances[actor])
                                - config.actor_deposits[actor]
                        );
                        let legs: Vec<_> = portfolio
                            .legs
                            .iter()
                            .map(|leg| leg.try_to_runtime().unwrap())
                            .filter(|leg| leg.active)
                            .collect();
                        let expected = if actor == TAKER {
                            position
                        } else if actor == LP {
                            -position
                        } else {
                            0
                        };
                        assert_eq!(legs.len(), usize::from(expected != 0));
                        if expected != 0 {
                            assert_eq!(legs[0].asset_index, u32::from(asset));
                            assert_eq!(legs[0].basis_pos_q, expected);
                        }
                    }
                    for market in 0..2 {
                        let oi = if market == usize::from(asset) {
                            position.unsigned_abs()
                        } else {
                            0
                        };
                        assert_eq!(group.assets[market].oi_eff_long_q, oi);
                        assert_eq!(group.assets[market].oi_eff_short_q, oi);
                        for side in 0..2 {
                            assert_eq!(
                                group.insurance_domain_budget[2 * market + side],
                                if market == usize::from(asset) { fee } else { 0 }
                            );
                        }
                    }
                };

                env.begin_public_trace();
                check(&env, 0, 0, &paid);
                env.trade_no_cpi(TAKER, LP, asset, quantity, PRICE, 0)
                    .unwrap();
                check(&env, quantity, 0, &paid);
                env.set_matcher_config_with_trade_fee_cap(LP, 1, LP_CAP as u16)
                    .unwrap();
                check(&env, quantity, 0, &paid);
                env.update_trade_fee_policy(OLD_POLICY).unwrap();
                check(&env, quantity, 0, &paid);

                let consent =
                    read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap();
                assert_eq!(consent.enabled(), 1);
                assert_eq!(u64::from(consent.trade_fee_cap_bps()), LP_CAP);
                let epochs = [TAKER, LP].map(|actor| env.primary_portfolio_position_epoch(actor));
                let policy_sequence = env.primary_control_sequences(0).trade_fee;
                let alternatives = ROUTES
                    .map(|candidate| retain_exit(&env, &payer, candidate, asset, -quantity, 1));
                let rejected = [TradeRoute::Cpi, TradeRoute::BatchCpi]
                    .map(|candidate| retain_exit(&env, &payer, candidate, asset, -quantity, 2));
                let late = retain_exit(
                    &env,
                    &payer,
                    if is_cpi(route) {
                        TradeRoute::NoCpi
                    } else {
                        TradeRoute::BatchCpi
                    },
                    asset,
                    -quantity,
                    3,
                );
                let retained_bytes = alternatives
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                let mut keys: Vec<_> = env
                    .all_economic_account_lamports()
                    .into_iter()
                    .map(|(key, _)| key)
                    .collect();
                keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
                keys.extend([env.program_id, env.matcher_program, spl_token::ID]);
                let frame = |env: &V16Svm| {
                    keys.iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let before = frame(&env);
                for tx in &alternatives {
                    env.svm.simulate_transaction(tx.clone().into()).unwrap_or_else(|error| panic!("{label}: every retained alternative must initially execute: {error:?}"));
                    assert_eq!(frame(&env), before, "simulation must not consume consent");
                }
                assert!(fee_atoms(quantity, LP_CAP + 1) > fee_atoms(quantity, LP_CAP));
                assert!(fee_atoms(quantity, LP_CAP + 1) < fee_atoms(quantity, SIGNED_BPS));
                env.update_trade_fee_policy(LP_CAP + 1).unwrap();
                check(&env, quantity, 0, &paid);
                assert_eq!(
                    read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap(),
                    consent
                );
                for tx in rejected {
                    let before = frame(&env);
                    let error = env
                        .land_retained(tx)
                        .expect_err("taker's looser cap cannot authorize an LP debit");
                    assert!(
                        error.contains(&format!(
                            "Custom({})",
                            PercolatorError::InvalidInstruction as u32
                        )),
                        "{label}: {error}"
                    );
                    assert!(
                        !error.contains(&format!("Program {} invoke", env.matcher_program)),
                        "{label}: standing LP cap must reject before CPI: {error}"
                    );
                    assert_eq!(
                        frame(&env),
                        before,
                        "{label}: complete rollback apart from transaction fee payer"
                    );
                    check(&env, quantity, 0, &paid);
                    rejections += 1;
                }

                if is_cpi(route) {
                    env.update_trade_fee_policy(policy).unwrap();
                    check(&env, quantity, 0, &paid);
                }
                assert_eq!(
                    env.primary_control_sequences(0).trade_fee,
                    policy_sequence + 1 + u64::from(is_cpi(route))
                );
                assert_eq!(
                    read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap(),
                    consent,
                    "policy must not rewrite LP consent"
                );
                assert_eq!(
                    alternatives
                        .each_ref()
                        .map(|tx| bincode::serialize(tx).unwrap()),
                    retained_bytes
                );
                let chosen = ROUTES
                    .iter()
                    .position(|candidate| *candidate == route)
                    .unwrap();
                let success = env
                    .land_retained(alternatives[chosen].clone())
                    .unwrap_or_else(|error| panic!("{label}: retained exit failed: {error}"));
                peak_cu = peak_cu.max(success.compute_units);
                let fee = fee_atoms(quantity, if is_cpi(route) { policy } else { SIGNED_BPS });
                assert!(fee > 0);
                check(&env, 0, fee, &paid);
                for (index, actor) in [TAKER, LP].into_iter().enumerate() {
                    assert_eq!(
                        env.primary_portfolio_position_epoch(actor),
                        epochs[index] + 1
                    );
                }
                let before = frame(&env);
                let error = env
                    .land_retained(late)
                    .expect_err("successful exit consumes every retained route alternative");
                assert!(
                    error.contains(&format!("Custom({})", PercolatorError::EngineStale as u32)),
                    "{label}: {error}"
                );
                assert_eq!(frame(&env), before);
                check(&env, 0, fee, &paid);
                rejections += 1;
                for actor in [TAKER, LP] {
                    let payout = config.actor_deposits[actor] - fee;
                    let success = env
                        .withdraw_primary(actor, payout)
                        .expect("fee-bounded exit must pay the owner's full remaining capital");
                    peak_cu = peak_cu.max(success.compute_units);
                    paid[actor] = payout;
                    check(&env, 0, fee, &paid);
                }
                let trace = env.finish_public_trace();
                trace.validate_public_execution().unwrap();
                assert_eq!(trace.out_of_band_economic_mutations, 0);
                assert_eq!(trace.steps.len(), 10 + usize::from(is_cpi(route)));
                transactions += trace.steps.len();
                let terminal = env.all_token_account_data();
                if let Some(expected) = &terminal_references[reference_index] {
                    assert_eq!(
                        &terminal, expected,
                        "{label}: transport changed terminal owner entitlement"
                    );
                } else {
                    terminal_references[reference_index] = Some(terminal);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, transactions, rejections), (24, 256, 72));
    assert!(peak_cu < TX_CU_LIMIT);
    eprintln!("INV-014: {worlds} retained exit worlds, {transactions} public transactions, {rejections} exact rejections, peak successful exit/withdrawal CU={peak_cu}");
}
