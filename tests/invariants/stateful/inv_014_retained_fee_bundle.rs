//! INV-014 / row 411: consent is local to each owner pair, even in a retained
//! atomic bundle whose earlier trade is authorized under a successor's policy.
//! All four transports occur in both positions, with independent fee envelopes.
//! Secondary evidence: INV-005/010/011/024/036/047/081 for authority succession,
//! ordering, owner-local bounds, fee attribution, transport endpoints and atomicity.
//! Equal taker/LP caps deliberately do not isolate the single-CPI taker guard.
//! A shared-taker continuation also binds the second instruction to the first
//! instruction's advanced epoch while keeping their fee envelopes separate.
//! No program-owned state is injected or rewritten by this test.

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
    pubkey::Pubkey,
    signature::{Keypair, SeedDerivable, Signer},
    transaction::Transaction,
};

const PRICE: u64 = 100_003;
const CAPS: [u64; 2] = [503, 37];
const SUCCESSOR: usize = 4;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchCpi,
];

fn is_cpi(route: TradeRoute) -> bool {
    matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)
}

fn fee_atoms(size: i128, bps: u64) -> u128 {
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let notional = ceil(size.unsigned_abs() * u128::from(PRICE), POS_SCALE);
    ceil(notional * u128::from(bps), 10_000)
}

fn signed(
    env: &V16Svm,
    payer: &Keypair,
    instructions: Vec<Instruction>,
    owners: &[&Keypair],
    nonce: u64,
) -> Transaction {
    let mut budget = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
        ComputeBudgetInstruction::set_compute_unit_price(nonce),
    ];
    budget.extend(instructions);
    let mut signers = vec![payer];
    signers.extend_from_slice(owners);
    let tx = Transaction::new_signed_with_payer(
        &budget,
        Some(&payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn retain_bundle(
    env: &V16Svm,
    payer: &Keypair,
    routes: [TradeRoute; 2],
    sizes: [i128; 2],
    order: [usize; 2],
    nonce: u64,
) -> Transaction {
    let mut instructions = Vec::new();
    let mut signers = Vec::new();
    for pair in order {
        let (a, b) = (2 * pair, 2 * pair + 1);
        let account_a_portfolio_id = env.primary_portfolio_id(a);
        let account_a_position_epoch = env.primary_portfolio_position_epoch(a);
        let account_b_portfolio_id = env.primary_portfolio_id(b);
        let account_b_position_epoch = env.primary_portfolio_position_epoch(b);
        let account_b_matcher_sequence = env.primary_portfolio_matcher_sequence(b);
        let asset_index = pair as u16;
        let market_id = env.primary_market_state().1.assets[pair].market_id;
        let size_q = sizes[pair];
        let fee_bps = CAPS[pair];
        let instruction = match routes[pair] {
            TradeRoute::NoCpi => ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index,
                market_id,
                size_q,
                exec_price: PRICE,
                fee_bps,
                backing_fee_cap_bps: 0,
            },
            TradeRoute::BatchNoCpi => ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                legs: vec![BatchTradeLeg {
                    asset_index,
                    market_id,
                    size_q,
                    exec_price: PRICE,
                    fee_bps,
                }],
            },
            TradeRoute::Cpi => ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                account_b_matcher_sequence,
                asset_index,
                market_id,
                size_q,
                fee_bps,
                limit_price: PRICE,
                backing_fee_cap_bps: 0,
            },
            TradeRoute::BatchCpi => ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                account_b_matcher_sequence,
                max_slippage_atoms: 0,
                max_fee_atoms: fee_atoms(size_q, fee_bps),
                legs: vec![BatchTradeCpiLeg {
                    asset_index,
                    market_id,
                    size_q,
                    fee_bps,
                    limit_price: PRICE,
                }],
            },
        };
        let mut accounts = vec![AccountMeta::new(env.actors[a].signer.pubkey(), true)];
        signers.push(&env.actors[a].signer);
        if !is_cpi(routes[pair]) {
            accounts.push(AccountMeta::new(env.actors[b].signer.pubkey(), true));
            signers.push(&env.actors[b].signer);
        }
        accounts.extend([
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[a].portfolio, false),
            AccountMeta::new(env.actors[b].portfolio, false),
        ]);
        if is_cpi(routes[pair]) {
            accounts.extend([
                AccountMeta::new_readonly(env.matcher_program, false),
                AccountMeta::new(env.actors[b].matcher_context, false),
                AccountMeta::new_readonly(env.actors[b].matcher_delegate, false),
            ]);
        }
        instructions.push(Instruction {
            program_id: env.program_id,
            accounts,
            data: instruction.encode(),
        });
    }
    let tx = signed(env, payer, instructions, &signers, nonce);
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
    tx
}

fn successor_policy(env: &V16Svm, payer: &Keypair, bps: u64, nonce: u64) -> Transaction {
    let sequences = env.primary_control_sequences(0);
    signed(
        env,
        payer,
        vec![Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.actors[SUCCESSOR].signer.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            data: ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: bps,
                policy_sequence: sequences.trade_fee + 1,
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        }],
        &[&env.actors[SUCCESSOR].signer],
        nonce,
    )
}

#[test]
fn v16_program_retained_shared_taker_fee_bundle_preserves_each_instruction_bound() {
    const TAKER: usize = 0;
    const OLD_BPS: u64 = 19;
    let sizes = [(3 * POS_SCALE + 7) as i128, -((POS_SCALE + 1) as i128)];
    let mut worlds = 0;
    let mut transactions = 0;
    let mut peak_cu = 0;
    let mut endpoint = None;
    for first in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
        for second in [TradeRoute::Cpi, TradeRoute::BatchCpi] {
            let routes = [first, second];
            let label = format!("shared taker, routes={routes:?}");
            let config = MarketConfig {
                initial_price: PRICE,
                ..MarketConfig::default()
            };
            let mut env = V16Svm::new([0x42; 32], config);
            let payer = Keypair::from_seed(&[0x15; 32]).unwrap();
            env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
            let capital: u128 = config.actor_deposits.iter().sum();
            let supply = env.token_supply_observed();
            let mut paid = [0u128; PRIMARY_ACTOR_COUNT];
            let check = |env: &V16Svm,
                         positions: [i128; 2],
                         fees: [u128; 2],
                         paid: &[u128; PRIMARY_ACTOR_COUNT]| {
                assert_public_stock_census(&label, env).unwrap();
                assert_public_encumbrance_census(&label, env).unwrap();
                let group = env.primary_market_state().1;
                assert_eq!(group.vault, capital - paid.iter().sum::<u128>());
                assert_eq!(group.insurance, 2 * fees.iter().sum::<u128>());
                assert_eq!(group.c_tot + group.insurance, group.vault);
                assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                assert_eq!(env.token_supply_observed(), supply);
                assert_eq!(u128::from(env.mint_supply()), supply);
                for actor in 0..PRIMARY_ACTOR_COUNT {
                    let account = env.primary_portfolio(actor);
                    let debit = match actor {
                        TAKER => fees.iter().sum(),
                        1 | 2 => fees[actor - 1],
                        _ => 0,
                    };
                    assert_eq!(account.owner, env.actors[actor].signer.pubkey().to_bytes());
                    assert_eq!(
                        account.capital.get(),
                        config.actor_deposits[actor] - debit - paid[actor],
                        "{label}: actor={actor}"
                    );
                    assert_eq!(account.pnl.get(), 0, "{label}: fees cannot hide in PnL");
                    assert_eq!(
                        u128::from(env.token_amount(env.actors[actor].destination_token)),
                        paid[actor]
                    );
                    assert_eq!(
                        u128::from(env.token_amount(env.actors[actor].source_token)),
                        u128::from(config.actor_token_balances[actor])
                            - config.actor_deposits[actor]
                    );
                    let legs: Vec<_> = account
                        .legs
                        .iter()
                        .map(|leg| leg.try_to_runtime().unwrap())
                        .filter(|leg| leg.active)
                        .collect();
                    let expected = [0, 1].map(|asset| {
                        if actor == TAKER {
                            positions[asset]
                        } else if actor == asset + 1 {
                            -positions[asset]
                        } else {
                            0
                        }
                    });
                    assert_eq!(legs.len(), expected.iter().filter(|q| **q != 0).count());
                    for asset in 0..2 {
                        let actual = legs
                            .iter()
                            .find(|leg| leg.asset_index as usize == asset)
                            .map_or(0, |leg| leg.basis_pos_q);
                        assert_eq!(
                            actual, expected[asset],
                            "{label}: actor={actor}, asset={asset}"
                        );
                    }
                }
                for asset in 0..2 {
                    assert_eq!(group.assets[asset].effective_price, PRICE);
                    assert_eq!(
                        group.assets[asset].oi_eff_long_q,
                        positions[asset].unsigned_abs()
                    );
                    assert_eq!(
                        group.assets[asset].oi_eff_short_q,
                        positions[asset].unsigned_abs()
                    );
                    assert_eq!(
                        &group.insurance_domain_budget[2 * asset..2 * asset + 2],
                        &[fees[asset]; 2]
                    );
                }
            };

            env.begin_public_trace();
            env.update_trade_fee_policy(OLD_BPS).unwrap();
            for leg in 0..2 {
                env.set_matcher_config_with_trade_fee_cap(leg + 1, 1, CAPS[leg] as u16)
                    .unwrap();
            }
            check(&env, [0; 2], [0; 2], &paid);
            let epochs = [0, 1, 2].map(|actor| env.primary_portfolio_position_epoch(actor));
            let sequences = [1, 2].map(|actor| env.primary_portfolio_matcher_sequence(actor));
            let grants = [1, 2].map(|actor| {
                read_portfolio_matcher_config(&env.primary_portfolio_data(actor)).unwrap()
            });
            let policy_sequence = env.primary_control_sequences(0).trade_fee;
            let instructions: Vec<_> = (0..2)
                .map(|leg| {
                    let lp = leg + 1;
                    let market_id = env.primary_market_state().1.assets[leg].market_id;
                    let instruction = if routes[leg] == TradeRoute::Cpi {
                        ProgInstruction::TradeCpi {
                            account_a_portfolio_id: env.primary_portfolio_id(TAKER),
                            // The suffix consumes the prefix's public post-state, signed in advance.
                            account_a_position_epoch: epochs[TAKER] + leg as u64,
                            account_b_portfolio_id: env.primary_portfolio_id(lp),
                            account_b_position_epoch: epochs[lp],
                            account_b_matcher_sequence: sequences[leg],
                            asset_index: leg as u16,
                            market_id,
                            size_q: sizes[leg],
                            fee_bps: CAPS[leg],
                            limit_price: PRICE,
                            backing_fee_cap_bps: 0,
                        }
                    } else {
                        ProgInstruction::BatchTradeCpi {
                            account_a_portfolio_id: env.primary_portfolio_id(TAKER),
                            account_a_position_epoch: epochs[TAKER] + leg as u64,
                            account_b_portfolio_id: env.primary_portfolio_id(lp),
                            account_b_position_epoch: epochs[lp],
                            account_b_matcher_sequence: sequences[leg],
                            max_slippage_atoms: 0,
                            max_fee_atoms: fee_atoms(sizes[leg], CAPS[leg]),
                            legs: vec![BatchTradeCpiLeg {
                                asset_index: leg as u16,
                                market_id,
                                size_q: sizes[leg],
                                fee_bps: CAPS[leg],
                                limit_price: PRICE,
                            }],
                        }
                    };
                    Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(env.actors[TAKER].signer.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.actors[TAKER].portfolio, false),
                            AccountMeta::new(env.actors[lp].portfolio, false),
                            AccountMeta::new_readonly(env.matcher_program, false),
                            AccountMeta::new(env.actors[lp].matcher_context, false),
                            AccountMeta::new_readonly(env.actors[lp].matcher_delegate, false),
                        ],
                        data: instruction.encode(),
                    }
                })
                .collect();
            let retained = [41_301, 41_302, 41_303].map(|nonce| {
                signed(
                    &env,
                    &payer,
                    instructions.clone(),
                    &[&env.actors[TAKER].signer],
                    nonce,
                )
            });
            let retained_bytes = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            let mut keys: Vec<_> = env
                .all_economic_account_lamports()
                .into_iter()
                .map(|(key, _)| key)
                .collect();
            keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
            keys.extend(
                retained[0]
                    .message
                    .account_keys
                    .iter()
                    .copied()
                    .filter(|key| *key != payer.pubkey()),
            );
            keys.sort_unstable();
            keys.dedup();
            let frame = |env: &V16Svm| {
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let before = frame(&env);
            for tx in &retained {
                assert_eq!(tx.message.header.num_required_signatures, 2);
                env.svm
                    .simulate_transaction(tx.clone().into())
                    .unwrap_or_else(|error| {
                        panic!("{label}: initially executable continuation: {error:?}")
                    });
                assert_eq!(frame(&env), before);
            }
            env.update_trade_fee_policy(CAPS[1] + 1).unwrap();
            check(&env, [0; 2], [0; 2], &paid);
            assert!(fee_atoms(sizes[0], CAPS[1] + 1) < fee_atoms(sizes[0], CAPS[0]));
            assert!(fee_atoms(sizes[1], CAPS[1] + 1) > fee_atoms(sizes[1], CAPS[1]));
            let before = frame(&env);
            assert_eq!(bincode::serialize(&retained[0]).unwrap(), retained_bytes[0]);
            let error = env
                .land_retained(retained[0].clone())
                .expect_err("prefix consent cannot fund the shared taker's tighter suffix");
            assert!(
                error.contains(&format!(
                    "InstructionError(4, Custom({}))",
                    PercolatorError::InvalidInstruction as u32
                )),
                "{label}: {error}"
            );
            assert_eq!(
                error
                    .matches(&format!("Program {} success", env.program_id))
                    .count(),
                1,
                "{label}: prefix must execute: {error}"
            );
            assert_eq!(
                error
                    .matches(&format!("Program {} success", env.matcher_program))
                    .count(),
                1,
                "{label}: suffix refusal precedes its matcher: {error}"
            );
            assert_eq!(
                frame(&env),
                before,
                "{label}: shared capital/epoch and both LPs roll back; network payer excluded"
            );
            check(&env, [0; 2], [0; 2], &paid);

            env.update_trade_fee_policy(CAPS[1]).unwrap();
            assert_eq!(
                env.primary_control_sequences(0).trade_fee,
                policy_sequence + 2
            );
            assert_eq!(
                [0, 1, 2].map(|actor| env.primary_portfolio_position_epoch(actor)),
                epochs
            );
            assert_eq!(
                [1, 2].map(|actor| read_portfolio_matcher_config(
                    &env.primary_portfolio_data(actor)
                )
                .unwrap()),
                grants
            );
            check(&env, [0; 2], [0; 2], &paid);
            assert_eq!(bincode::serialize(&retained[1]).unwrap(), retained_bytes[1]);
            peak_cu = peak_cu.max(
                env.land_retained(retained[1].clone())
                    .unwrap()
                    .compute_units,
            );
            let fees = sizes.map(|q| fee_atoms(q, CAPS[1]));
            for leg in 0..2 {
                assert!(fees[leg] > 0 && fees[leg] <= fee_atoms(sizes[leg], CAPS[leg]));
                assert_ne!(fees[leg], fee_atoms(sizes[leg], OLD_BPS));
            }
            assert!(fees[0] < fee_atoms(sizes[0], CAPS[0]));
            assert!(
                fees.iter().sum::<u128>() > fee_atoms(sizes[1], CAPS[1]),
                "a suffix cap bounds its own charge, not the shared portfolio's cumulative fees"
            );
            check(&env, sizes, fees, &paid);
            assert_eq!(
                [0, 1, 2].map(|actor| env.primary_portfolio_position_epoch(actor)),
                [epochs[0] + 2, epochs[1] + 1, epochs[2] + 1]
            );
            assert_eq!(
                [1, 2].map(|actor| env.primary_portfolio_matcher_sequence(actor)),
                sequences
            );
            let before = frame(&env);
            assert_eq!(bincode::serialize(&retained[2]).unwrap(), retained_bytes[2]);
            let error = env
                .land_retained(retained[2].clone())
                .expect_err("the shared continuation is consumed exactly once");
            assert!(
                error.contains(&format!(
                    "InstructionError(3, Custom({}))",
                    PercolatorError::EngineStale as u32
                )),
                "{label}: {error}"
            );
            assert_eq!(frame(&env), before);
            check(&env, sizes, fees, &paid);

            env.update_trade_fee_policy(0).unwrap();
            let mut positions = sizes;
            for leg in 0..2 {
                env.trade_no_cpi(TAKER, leg + 1, leg as u16, -sizes[leg], PRICE, 0)
                    .unwrap();
                positions[leg] = 0;
                check(&env, positions, fees, &paid);
            }
            for actor in 0..PRIMARY_ACTOR_COUNT {
                let debit = match actor {
                    TAKER => fees.iter().sum(),
                    1 | 2 => fees[actor - 1],
                    _ => 0,
                };
                let amount = config.actor_deposits[actor] - debit;
                env.withdraw_primary(actor, amount).unwrap();
                paid[actor] = amount;
                check(&env, [0; 2], fees, &paid);
            }
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                2 * fees.iter().sum::<u128>()
            );
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            assert_eq!(trace.steps.len(), 16);
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 2);
            transactions += trace.steps.len();
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
            let tokens = env.all_token_account_data();
            if let Some(expected) = &endpoint {
                assert_eq!(
                    &tokens, expected,
                    "{label}: shared-owner terminal entitlement"
                );
            } else {
                endpoint = Some(tokens);
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, transactions), (4, 64));
    assert!(peak_cu < TX_CU_LIMIT);
    eprintln!("INV-014 shared taker fee bundle: {worlds} worlds, {transactions} public transactions, 8 exact rollbacks, 4 shared-prefix rollbacks, peak successful CU={peak_cu}");
}

#[test]
fn v16_program_retained_fee_bundle_route_product_rolls_back_authorized_prefix() {
    let mut worlds = 0;
    let mut transactions = 0;
    let mut rejections = 0;
    let mut peak_cu = 0;
    let mut endpoints = [None, None, None, None];
    for first in ROUTES {
        for second in ROUTES {
            let routes = [first, second];
            for direction in [-1i128, 1] {
                for order in [[0usize, 1], [1, 0]] {
                    let label =
                        format!("routes={routes:?}, direction={direction}, order={order:?}");
                    let config = MarketConfig {
                        initial_price: PRICE,
                        ..MarketConfig::default()
                    };
                    let mut env = V16Svm::new([0x41; 32], config);
                    let payer = Keypair::from_seed(&[0x14; 32]).unwrap();
                    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                    let sizes = [
                        direction * (POS_SCALE + 1) as i128,
                        -direction * (3 * POS_SCALE + 7) as i128,
                    ];
                    let initial_capital: u128 = config.actor_deposits.iter().sum();
                    let supply = env.token_supply_observed();
                    let mut paid = [0u128; PRIMARY_ACTOR_COUNT];
                    let check =
                        |env: &V16Svm,
                         positions: [i128; 2],
                         fees: [u128; 2],
                         paid: &[u128; PRIMARY_ACTOR_COUNT]| {
                            assert_public_stock_census(&label, env).unwrap();
                            assert_public_encumbrance_census(&label, env).unwrap();
                            let group = env.primary_market_state().1;
                            assert_eq!(
                                group.vault,
                                initial_capital - paid.iter().sum::<u128>(),
                                "{label}"
                            );
                            assert_eq!(group.insurance, 2 * fees.iter().sum::<u128>(), "{label}");
                            assert_eq!(group.c_tot + group.insurance, group.vault, "{label}");
                            assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                            assert_eq!(env.token_supply_observed(), supply);
                            assert_eq!(u128::from(env.mint_supply()), supply);
                            for actor in 0..PRIMARY_ACTOR_COUNT {
                                let p = env.primary_portfolio(actor);
                                let fee = if actor < 4 { fees[actor / 2] } else { 0 };
                                assert_eq!(
                                    p.capital.get(),
                                    config.actor_deposits[actor] - fee - paid[actor],
                                    "{label}: owner {actor}"
                                );
                                assert_eq!(p.pnl.get(), 0, "{label}: fee cannot hide in PnL");
                                assert_eq!(
                                    u128::from(
                                        env.token_amount(env.actors[actor].destination_token)
                                    ),
                                    paid[actor]
                                );
                                assert_eq!(
                                    u128::from(env.token_amount(env.actors[actor].source_token)),
                                    u128::from(config.actor_token_balances[actor])
                                        - config.actor_deposits[actor]
                                );
                                let legs: Vec<_> = p
                                    .legs
                                    .iter()
                                    .map(|leg| leg.try_to_runtime().unwrap())
                                    .filter(|leg| leg.active)
                                    .collect();
                                let position = if actor < 4 {
                                    positions[actor / 2] * if actor % 2 == 0 { 1 } else { -1 }
                                } else {
                                    0
                                };
                                assert_eq!(legs.len(), usize::from(position != 0), "{label}");
                                if position != 0 {
                                    assert_eq!(legs[0].asset_index, (actor / 2) as u32);
                                    assert_eq!(legs[0].basis_pos_q, position, "{label}");
                                }
                            }
                            for asset in 0..2 {
                                assert_eq!(
                                    group.assets[asset].oi_eff_long_q,
                                    positions[asset].unsigned_abs()
                                );
                                assert_eq!(
                                    group.assets[asset].oi_eff_short_q,
                                    positions[asset].unsigned_abs()
                                );
                                assert_eq!(group.insurance_domain_budget[2 * asset], fees[asset]);
                                assert_eq!(
                                    group.insurance_domain_budget[2 * asset + 1],
                                    fees[asset]
                                );
                            }
                        };
                    let mut keys: Vec<_> = env
                        .all_economic_account_lamports()
                        .into_iter()
                        .map(|(key, _)| key)
                        .collect();
                    keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
                    keys.extend([
                        Pubkey::new_from_array(env.primary_market_state().0.marketauth),
                        Pubkey::new_from_array(env.foreign_market_state().0.marketauth),
                        env.foreign_actor.signer.pubkey(),
                        env.program_id,
                        env.matcher_program,
                        spl_token::ID,
                    ]);
                    let frame = |env: &V16Svm| {
                        keys.iter()
                            .map(|key| env.svm.get_account(key))
                            .collect::<Vec<_>>()
                    };

                    env.begin_public_trace();
                    check(&env, [0; 2], [0; 2], &paid);
                    env.update_trade_fee_policy(19).unwrap();
                    check(&env, [0; 2], [0; 2], &paid);
                    for pair in 0..2 {
                        env.set_matcher_config_with_trade_fee_cap(
                            2 * pair + 1,
                            1,
                            CAPS[pair] as u16,
                        )
                        .unwrap();
                        check(&env, [0; 2], [0; 2], &paid);
                    }
                    let consent = [1, 3].map(|actor| {
                        read_portfolio_matcher_config(&env.primary_portfolio_data(actor)).unwrap()
                    });
                    let epochs =
                        [0, 1, 2, 3].map(|actor| env.primary_portfolio_position_epoch(actor));
                    let old_authority_epoch = env.primary_control_sequences(0).authority_epoch;
                    // All three envelopes are signed before the handoff/hike. Distinct
                    // compute prices bypass the signature cache without rebinding consent.
                    let rejected = retain_bundle(&env, &payer, routes, sizes, order, 41_101);
                    let accepted = retain_bundle(&env, &payer, routes, sizes, order, 41_102);
                    let consumed = retain_bundle(&env, &payer, routes, sizes, order, 41_103);
                    let retained_bytes = bincode::serialize(&accepted).unwrap();
                    let before = frame(&env);
                    for tx in [&rejected, &accepted, &consumed] {
                        env.svm
                            .simulate_transaction(tx.clone().into())
                            .unwrap_or_else(|error| {
                                panic!("{label}: initially executable: {error:?}")
                            });
                        assert_eq!(frame(&env), before, "simulation must not consume consent");
                    }
                    env.update_market_authority_from_admin(SUCCESSOR).unwrap();
                    assert_eq!(
                        env.primary_control_sequences(0).authority_epoch,
                        old_authority_epoch + 1
                    );
                    check(&env, [0; 2], [0; 2], &paid);
                    let raised = successor_policy(&env, &payer, CAPS[1] + 1, 41_104);
                    env.land_retained(raised).unwrap();
                    check(&env, [0; 2], [0; 2], &paid);
                    for pair in 0..2 {
                        assert_eq!(
                            read_portfolio_matcher_config(
                                &env.primary_portfolio_data(2 * pair + 1)
                            )
                            .unwrap(),
                            consent[pair],
                            "policy authority cannot enlarge owner consent"
                        );
                    }
                    assert!(fee_atoms(sizes[1], CAPS[1] + 1) > fee_atoms(sizes[1], CAPS[1]));
                    assert!(fee_atoms(sizes[0], CAPS[1] + 1) < fee_atoms(sizes[0], CAPS[0]));
                    let before = frame(&env);
                    let error = env.land_retained(rejected).expect_err(
                        "one pair cannot lend its larger consent envelope to another pair",
                    );
                    let failing_index = 3 + order.iter().position(|pair| *pair == 1).unwrap();
                    assert!(
                        error.contains(&format!(
                            "InstructionError({failing_index}, Custom({}))",
                            PercolatorError::InvalidInstruction as u32
                        )),
                        "{label}: {error}"
                    );
                    if order == [0, 1] {
                        assert!(
                            error.contains(&format!("Program {} success", env.program_id)),
                            "{label}: authorized prefix must actually complete: {error}"
                        );
                        if is_cpi(first) {
                            assert!(
                                error.contains(&format!("Program {} success", env.matcher_program)),
                                "{label}: matcher prefix must actually complete: {error}"
                            );
                        }
                    }
                    assert_eq!(frame(&env), before, "{label}: rollback includes the authorized prefix, matcher, signers, SPL, and unrelated state; only network payer excluded");
                    check(&env, [0; 2], [0; 2], &paid);
                    rejections += 1;
                    let restored = successor_policy(&env, &payer, CAPS[1], 41_105);
                    env.land_retained(restored).unwrap();
                    check(&env, [0; 2], [0; 2], &paid);
                    assert_eq!(bincode::serialize(&accepted).unwrap(), retained_bytes);
                    peak_cu = peak_cu.max(
                        env.land_retained(accepted)
                            .unwrap_or_else(|error| panic!("{label}: retained retry: {error}"))
                            .compute_units,
                    );
                    let fees = [0, 1].map(|pair| {
                        fee_atoms(
                            sizes[pair],
                            if is_cpi(routes[pair]) {
                                CAPS[1]
                            } else {
                                CAPS[pair]
                            },
                        )
                    });
                    for pair in 0..2 {
                        assert!(fees[pair] > 0 && fees[pair] <= fee_atoms(sizes[pair], CAPS[pair]));
                    }
                    check(&env, sizes, fees, &paid);
                    for actor in 0..4 {
                        assert_eq!(
                            env.primary_portfolio_position_epoch(actor),
                            epochs[actor] + 1
                        );
                    }
                    let before = frame(&env);
                    let error = env
                        .land_retained(consumed)
                        .expect_err("consumed position episodes cannot authorize a second bundle");
                    assert!(
                        error.contains(&format!(
                            "InstructionError(3, Custom({}))",
                            PercolatorError::EngineStale as u32
                        )),
                        "{label}: {error}"
                    );
                    assert_eq!(frame(&env), before);
                    check(&env, sizes, fees, &paid);
                    rejections += 1;

                    let zero = successor_policy(&env, &payer, 0, 41_106);
                    env.land_retained(zero).unwrap();
                    check(&env, sizes, fees, &paid);
                    let mut positions = sizes;
                    for pair in order {
                        peak_cu = peak_cu.max(
                            env.trade_no_cpi(
                                2 * pair,
                                2 * pair + 1,
                                pair as u16,
                                -sizes[pair],
                                PRICE,
                                0,
                            )
                            .unwrap()
                            .compute_units,
                        );
                        positions[pair] = 0;
                        check(&env, positions, fees, &paid);
                    }
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let amount = config.actor_deposits[actor]
                            - if actor < 4 { fees[actor / 2] } else { 0 };
                        peak_cu =
                            peak_cu.max(env.withdraw_primary(actor, amount).unwrap().compute_units);
                        paid[actor] = amount;
                        check(&env, [0; 2], fees, &paid);
                    }
                    let trace = env.finish_public_trace();
                    trace.validate_public_execution().unwrap();
                    assert_eq!(trace.out_of_band_economic_mutations, 0);
                    assert_eq!(trace.steps.len(), 17);
                    assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 2);
                    transactions += trace.steps.len();
                    // Bilateral fees are exact signed rates; CPI fees are the policy
                    // rate. Compare only economically equivalent single/batch worlds.
                    let class = 2 * usize::from(is_cpi(first)) + usize::from(is_cpi(second));
                    let endpoint = env.all_token_account_data();
                    if let Some(expected) = &endpoints[class] {
                        assert_eq!(
                            &endpoint, expected,
                            "{label}: transport/order changed final SPL entitlement"
                        );
                    } else {
                        endpoints[class] = Some(endpoint);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, transactions, rejections), (64, 1088, 128));
    assert!(peak_cu < TX_CU_LIMIT);
    eprintln!("INV-014 retained fee bundle: worlds={worlds}, public_txs={transactions}, exact_rollbacks={rejections}, authorized_prefix_rollbacks=32, peak_success_cu={peak_cu}");
}
