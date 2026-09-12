//! INV-014: retained close fees and a pre-signed withdrawal share one atomic budget.
//! CPI uses the live policy within consent; bilateral routes charge the explicit signed rate.
//! Repricing can invalidate the withdrawal amount without invalidating either close fee cap.

use super::super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const DEPOSITS: [u128; 2] = [100_003, 200_007];
const PRICE: u64 = 100;
const OLD_BPS: u64 = 19;
const CAP_BPS: u64 = 100;
const QUANTITY: i128 = (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;

fn fee(bps: u64) -> u128 {
    let notional = (QUANTITY.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(bps)).div_ceil(10_000)
}

#[test]
fn v16_retained_close_withdrawal_reconciles_repriced_fees_across_all_trade_routes() {
    let matcher_bytes = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    for current_bps in [7, 37] {
        let mut route_payouts = Vec::new();
        for cpi in [false, true] {
            for batch in [false, true] {
                let label = format!("cpi={cpi}, batch={batch}, policy={OLD_BPS}->{current_bps}");
                let mut env = inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        trade_fee_base_bps: OLD_BPS,
                        ..V16CuMarketParams::default()
                    },
                );
                env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
                let owners = [Keypair::new(), Keypair::new()];
                let portfolio_keys = [Keypair::new(), Keypair::new()];
                let portfolios = portfolio_keys.each_ref().map(Signer::pubkey);
                let mut tokens = [Pubkey::default(); 2];
                for actor in 0..2 {
                    env.svm.airdrop(&owners[actor].pubkey(), 1_000_000).unwrap();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &portfolio_keys[actor],
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                    env.portfolios.push(portfolios[actor]);
                    tokens[actor] = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        owners[actor].pubkey(),
                        env.mint,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[actor],
                            &env.admin.pubkey(),
                            &[],
                            DEPOSITS[actor] as u64,
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    env.send(
                        env.deposit_ix(portfolios[actor], DEPOSITS[actor]),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                }
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &env.admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                let bilateral_accounts = vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ];
                env.send(
                    env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, QUANTITY, PRICE, OLD_BPS),
                    bilateral_accounts.clone(),
                    &[&owners[0], &owners[1]],
                )
                .unwrap();

                let matcher = Pubkey::new_unique();
                env.svm.add_program(matcher, &matcher_bytes);
                let context_key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &context_key,
                    MATCHER_CONTEXT_LEN,
                    matcher,
                );
                let context = context_key.pubkey();
                let delegate = matcher_delegate_key(
                    &env.program_id,
                    &env.market,
                    &portfolios[1],
                    &owners[1].pubkey(),
                    &matcher,
                    &context,
                );
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    Instruction {
                        program_id: matcher,
                        accounts: vec![
                            AccountMeta::new_readonly(owners[1].pubkey(), true),
                            AccountMeta::new(context, false),
                        ],
                        data: vec![10],
                    },
                    &[&owners[1]],
                )
                .unwrap();
                env.set_matcher_config_with_trade_fee_cap(
                    matcher,
                    &owners[1],
                    portfolios[1],
                    context,
                    delegate,
                    1,
                    CAP_BPS as u16,
                );
                let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
                let sequences = portfolios.map(|key| env.portfolio_matcher_sequence(key));
                let withdrawal = |env: &V16CuEnv, actor: usize, amount| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(portfolios[actor], amount).encode(),
                };
                let close = match (cpi, batch) {
                    (false, false) => env.trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        0,
                        -QUANTITY,
                        PRICE,
                        CAP_BPS,
                    ),
                    (false, true) => env.batch_trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        vec![BatchTradeLeg {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            size_q: -QUANTITY,
                            exec_price: PRICE,
                            fee_bps: CAP_BPS,
                        }],
                    ),
                    (true, false) => {
                        env.trade_cpi_ix(portfolios[0], portfolios[1], 0, -QUANTITY, CAP_BPS, PRICE)
                    }
                    (true, true) => env.batch_trade_cpi_ix_with_caps(
                        portfolios[0],
                        portfolios[1],
                        vec![BatchTradeCpiLeg {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            size_q: -QUANTITY,
                            limit_price: PRICE,
                            fee_bps: CAP_BPS,
                        }],
                        0,
                        fee(CAP_BPS),
                    ),
                };
                let close = Instruction {
                    program_id: env.program_id,
                    accounts: if cpi {
                        vec![
                            AccountMeta::new(owners[0].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(portfolios[1], false),
                            AccountMeta::new_readonly(matcher, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegate, false),
                        ]
                    } else {
                        bilateral_accounts
                    },
                    data: close.encode(),
                };
                let retain = |env: &V16CuEnv,
                              instructions: &[Instruction],
                              bilateral: bool,
                              distinct: bool| {
                    let mut ixs = vec![heap_ix(), cu_ix()];
                    if distinct {
                        ixs.push(ComputeBudgetInstruction::set_compute_unit_price(0));
                    }
                    ixs.extend_from_slice(instructions);
                    let mut signers = vec![&env.payer, &owners[0]];
                    if bilateral {
                        signers.push(&owners[1]);
                    }
                    let tx = Transaction::new_signed_with_payer(
                        &ixs,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(
                        bincode::serialized_size(&tx).unwrap()
                            <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                    );
                    tx
                };
                let quoted_fee = fee(if cpi { OLD_BPS } else { CAP_BPS });
                let quoted_withdrawal = DEPOSITS[0] - fee(OLD_BPS) - quoted_fee;
                let instructions = [close.clone(), withdrawal(&env, 0, quoted_withdrawal)];
                let bundle = retain(&env, &instructions, !cpi, false);
                let alternative = retain(&env, &instructions, !cpi, true);
                let close_only = retain(&env, &[close], !cpi, false);
                let retained_bytes =
                    [&bundle, &alternative, &close_only].map(|tx| bincode::serialize(tx).unwrap());
                let mut keys = vec![
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    tokens[0],
                    tokens[1],
                    env.vault,
                    env.mint,
                    env.admin.pubkey(),
                    context,
                    delegate,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                ];
                for tx in [&bundle, &alternative, &close_only] {
                    keys.extend(tx.message.account_keys.iter().copied());
                }
                keys.retain(|key| *key != env.payer.pubkey());
                keys.sort_unstable();
                keys.dedup();
                let frame = |env: &V16CuEnv| {
                    keys.iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let before = frame(&env);
                for tx in [&bundle, &alternative, &close_only] {
                    env.svm
                        .simulate_transaction(tx.clone().into())
                        .unwrap_or_else(|error| {
                            panic!("{label}: retained request initially executable: {error:?}")
                        });
                    assert_eq!(frame(&env), before);
                }
                let check = |env: &V16CuEnv, closed: bool, fees: u128, paid: [u128; 2]| {
                    for (key, initial) in keys.iter().zip(&before) {
                        if ![
                            env.market,
                            portfolios[0],
                            portfolios[1],
                            tokens[0],
                            tokens[1],
                            env.vault,
                            context,
                        ]
                        .contains(key)
                        {
                            assert_eq!(
                                &env.svm.get_account(key),
                                initial,
                                "{label}: passive Account {key}"
                            );
                        }
                    }
                    let accounts = portfolios.map(|key| env.portfolio_state(key));
                    for actor in 0..2 {
                        assert_eq!(accounts[actor].owner, owners[actor].pubkey().to_bytes());
                        assert_eq!(
                            accounts[actor].capital.get(),
                            DEPOSITS[actor] - fees - paid[actor],
                            "{label}"
                        );
                        assert_eq!(accounts[actor].pnl.get(), 0);
                        assert_eq!(u128::from(env.token_amount(tokens[actor])), paid[actor]);
                        assert_eq!(
                            env.portfolio_position_epoch(portfolios[actor]),
                            epochs[actor] + u64::from(closed)
                        );
                        if closed {
                            assert!(!has_active_leg_for_asset(&accounts[actor], 0));
                        } else {
                            assert_eq!(
                                active_leg_for_asset(&accounts[actor], 0).basis_pos_q,
                                if actor == 0 { QUANTITY } else { -QUANTITY }
                            );
                        }
                    }
                    let (_, group) = env.market_state();
                    let quantity = if closed { 0 } else { QUANTITY.unsigned_abs() };
                    assert_eq!(
                        [
                            group.assets[0].oi_eff_long_q,
                            group.assets[0].oi_eff_short_q
                        ],
                        [quantity; 2]
                    );
                    assert_eq!(group.assets[0].effective_price, PRICE);
                    assert_eq!(group.insurance, 2 * fees);
                    assert_eq!(&group.insurance_domain_budget[..2], &[fees; 2]);
                    assert_eq!(
                        group.c_tot,
                        DEPOSITS.iter().sum::<u128>() - 2 * fees - paid.iter().sum::<u128>()
                    );
                    assert_eq!(group.vault, group.c_tot + group.insurance);
                    assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                    assert_market_stock_census(
                        &label,
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        group.vault,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(u128::from(mint.supply), DEPOSITS.iter().sum::<u128>());
                    assert_eq!(mint.mint_authority, COption::None);
                };
                check(&env, false, fee(OLD_BPS), [0; 2]);
                let policy_sequence = env.control_sequences(0).trade_fee;
                env.update_trade_fee_policy_with_cu(current_bps);
                assert_eq!(env.control_sequences(0).trade_fee, policy_sequence + 1);
                assert_eq!(env.market_state().0.trade_fee_base_bps, current_bps);
                assert_eq!(
                    portfolios.map(|key| env.portfolio_matcher_sequence(key)),
                    sequences
                );
                check(&env, false, fee(OLD_BPS), [0; 2]);
                for (index, tx) in [&bundle, &alternative, &close_only].into_iter().enumerate() {
                    assert_eq!(bincode::serialize(tx).unwrap(), retained_bytes[index]);
                    tx.verify().unwrap();
                }
                let submit = |env: &mut V16CuEnv, tx: Transaction| {
                    tx.verify().unwrap();
                    assert!(
                        bincode::serialized_size(&tx).unwrap()
                            <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                    );
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx);
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    result
                };
                let actual_fee = fee(if cpi { current_bps } else { CAP_BPS });
                assert!(actual_fee > 0 && actual_fee <= fee(CAP_BPS));
                if cpi {
                    assert!(actual_fee < fee(CAP_BPS));
                    assert_ne!(actual_fee, quoted_fee);
                } else {
                    assert_eq!(actual_fee, quoted_fee);
                }
                let total_fee = fee(OLD_BPS) + actual_fee;
                let mut paid = [0; 2];
                if cpi && current_bps > OLD_BPS {
                    assert!(quoted_withdrawal > DEPOSITS[0] - total_fee);
                    let before = frame(&env);
                    let failure = submit(&mut env, bundle)
                        .expect_err("in-cap repricing makes the retained withdrawal unaffordable");
                    println!("{label}: close fee={actual_fee}, retained withdrawal={quoted_withdrawal}, rejection={:?}", failure.err);
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            3,
                            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                        ),
                        "{label}: {failure:?}"
                    );
                    assert_eq!(
                        failure
                            .meta
                            .logs
                            .iter()
                            .filter(|line| *line == &format!("Program {} success", env.program_id))
                            .count(),
                        1
                    );
                    assert!(failure
                        .meta
                        .logs
                        .contains(&format!("Program {matcher} success")));
                    assert_eq!(
                        frame(&env),
                        before,
                        "{label}: close, fee and matcher rollback"
                    );
                    check(&env, false, fee(OLD_BPS), paid);
                    peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
                    rollbacks += 1;
                    let success = submit(&mut env, close_only)
                        .expect("the retained close alone remains authorized");
                    peak_cu = peak_cu.max(success.compute_units_consumed);
                } else {
                    let success = submit(&mut env, bundle)
                        .expect("the retained close and fixed payout remain affordable");
                    peak_cu = peak_cu.max(success.compute_units_consumed);
                    paid[0] = quoted_withdrawal;
                    if cpi {
                        assert!(
                            DEPOSITS[0] - total_fee - paid[0] > 0,
                            "a lower live fee must leave an independently withdrawable remainder"
                        );
                    } else {
                        assert_eq!(DEPOSITS[0] - total_fee, paid[0]);
                    }
                }
                check(&env, true, total_fee, paid);
                let grant = state::read_portfolio_matcher_config(
                    &env.svm.get_account(&portfolios[1]).unwrap().data,
                )
                .unwrap();
                assert_eq!(grant.matcher_program, matcher.to_bytes());
                assert_eq!(grant.matcher_context, context.to_bytes());
                assert_eq!(grant.matcher_delegate, delegate.to_bytes());
                assert_eq!(grant.trade_fee_cap_bps(), CAP_BPS as u16);
                assert_eq!(grant.enabled(), u64::from(cpi));
                assert_eq!(env.portfolio_matcher_sequence(portfolios[1]), sequences[1]);
                let before = frame(&env);
                let stale =
                    submit(&mut env, alternative).expect_err("the consumed close cannot pay twice");
                assert_eq!(
                    stale.err,
                    TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(PercolatorError::EngineStale as u32)
                    )
                );
                assert_eq!(
                    frame(&env),
                    before,
                    "{label}: consumed close/payout alternative"
                );
                peak_cu = peak_cu.max(stale.meta.compute_units_consumed);
                rollbacks += 1;
                for actor in 0..2 {
                    let remainder = DEPOSITS[actor] - total_fee - paid[actor];
                    if remainder == 0 {
                        continue;
                    }
                    let ix = withdrawal(&env, actor, remainder);
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix],
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &owners[actor]],
                        env.svm.latest_blockhash(),
                    );
                    let success =
                        submit(&mut env, tx).expect("fresh exact post-fee withdrawal exits");
                    peak_cu = peak_cu.max(success.compute_units_consumed);
                    paid[actor] += remainder;
                    check(&env, true, total_fee, paid);
                }
                assert_eq!(env.market_state().1.c_tot, 0);
                assert_eq!(env.market_state().0.trade_fee_base_bps, current_bps);
                assert_eq!(env.control_sequences(0).trade_fee, policy_sequence + 1);
                assert_eq!(u128::from(env.token_amount(env.vault)), 2 * total_fee);
                route_payouts.push(paid);
            }
        }
        assert_eq!(
            route_payouts[0], route_payouts[1],
            "bilateral single/batch payouts"
        );
        assert_eq!(
            route_payouts[2], route_payouts[3],
            "CPI single/batch payouts"
        );
        assert_ne!(
            route_payouts[0], route_payouts[2],
            "explicit signed rate and live capped rate differ"
        );
    }
    assert_eq!(rollbacks, 10);
    assert_cu_within("retained repriced close/withdrawal", peak_cu, 325_000);
    println!("INV-014 close/withdrawal: 8 worlds, 2 paid-close rollbacks, 8 consumed alternatives, 16 complete owner exits; peak CU={peak_cu}");
}
