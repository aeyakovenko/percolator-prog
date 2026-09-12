//! INV-014 - Delayed policy and policy-epoch safety.
//!
//! Normative obligation: an authorized control that lands after a newer control
//! cannot overwrite it. Independent policy scopes must not block one another,
//! and forward sequence gaps remain valid so retained transactions can land out
//! of order without requiring every intermediate sequence to execute.
//!
//! Evidence in this file uses the public SBF instruction boundary. The first
//! test covers strict monotonicity, gap acceptance, exact rollback, and lane
//! independence. Fee-consent coverage proves a post-sign base-fee policy cannot
//! silently charge either trader beyond the signed fee. The second covers cross-mode oracle supersession: EWMA,
//! authenticated mark, and hybrid configuration all consume one observation
//! lane, so switching instruction variants cannot revive stale consent.
//! Retained CPI price-limit coverage instead changes the oracle mode after the
//! economic request is signed: adverse repricing rolls back a funded prefix and
//! matcher writes, while favorable repricing and fresh bounded consent stay live.
//! Retained activation-fee coverage crosses a market-authority handoff with append
//! and retired-slot reuse, exact signed fee ceilings, funded-prefix rollback, and
//! canonical insurance attribution under the successor's stricter/looser policy.
//! Retained fee/capacity coverage reprices before a flagged partial: single CPI
//! charges actual filled quantity, while exact-fill batch CPI rolls back and an
//! already-signed alternative remains live after public capacity restoration.
//!
//! Guarantee boundary: these tests cover supersession within one live market
//! incarnation. Market recreation and authority A -> B -> A require persistent
//! incarnation identifiers and are tracked by INV-001 and INV-005.

use super::*;

#[path = "inv_014_retained_activation_fee.rs"]
mod retained_activation_fee;

#[path = "inv_014_retained_close_withdrawal.rs"]
mod retained_close_withdrawal;

#[path = "inv_014_retained_single_cpi_taker_fee.rs"]
mod retained_single_cpi_taker_fee;

#[test]
fn v16_retained_fee_terms_bound_partial_and_exact_fill_routes_after_policy_change() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };
    use percolator_prog::matcher_abi::{read_matcher_return, FLAG_PARTIAL_OK};
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const DEPOSITS: [u128; 2] = [100_003, 200_007];
    const PRICE: u64 = 100;
    const OLD_BPS: u64 = 19;
    const SIGNED_BPS: u64 = 100;
    const QUANTITY: i128 = (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
    let fee = |quantity: i128, bps: u64| {
        let quote = (quantity.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
        (quote * u128::from(bps)).div_ceil(10_000)
    };
    let matcher_bytes = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut peak_cu = 0;
    for current_bps in [37, 7] {
        for batch in [false, true] {
            let label = format!("batch={batch}, policy={OLD_BPS}->{current_bps}");
            let mut env = inv018_public_spl_market_with_params(
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
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(
                        &env.payer.pubkey(),
                        &owners[actor].pubkey(),
                        1_000_000,
                    ),
                    &[],
                )
                .unwrap();
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
                tokens[actor] =
                    create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
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
            let control = |env: &mut V16CuEnv, data| {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    Instruction {
                        program_id: matcher,
                        accounts: vec![
                            AccountMeta::new_readonly(owners[1].pubkey(), true),
                            AccountMeta::new(context, false),
                        ],
                        data,
                    },
                    &[&owners[1]],
                )
                .unwrap();
            };
            control(&mut env, vec![10]);
            env.set_matcher_config_with_trade_fee_cap(
                matcher,
                &owners[1],
                portfolios[1],
                context,
                delegate,
                1,
                SIGNED_BPS as u16,
            );
            let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
            let matcher_sequence = env.portfolio_matcher_sequence(portfolios[1]);
            let trade = if batch {
                env.batch_trade_cpi_ix_with_caps(
                    portfolios[0],
                    portfolios[1],
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: QUANTITY,
                        fee_bps: SIGNED_BPS,
                        limit_price: PRICE,
                    }],
                    0,
                    fee(QUANTITY, SIGNED_BPS),
                )
            } else {
                env.trade_cpi_ix(portfolios[0], portfolios[1], 0, QUANTITY, SIGNED_BPS, PRICE)
            };
            let retain = |extra_budget_ix: bool| {
                let mut instructions = vec![heap_ix(), cu_ix()];
                // A zero-price budget instruction distinguishes pre-signed deliveries without
                // changing economic terms or requiring a new signature after policy changes.
                if extra_budget_ix {
                    instructions.push(ComputeBudgetInstruction::set_compute_unit_price(0));
                }
                instructions.push(Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[0].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                        AccountMeta::new_readonly(matcher, false),
                        AccountMeta::new(context, false),
                        AccountMeta::new_readonly(delegate, false),
                    ],
                    data: trade.encode(),
                });
                Transaction::new_signed_with_payer(
                    &instructions,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owners[0]],
                    env.svm.latest_blockhash(),
                )
            };
            let requests = [retain(false), retain(true)];
            let signed_bytes = requests
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            let keys = [
                env.market,
                portfolios[0],
                portfolios[1],
                context,
                delegate,
                env.vault,
                env.mint,
                tokens[0],
                tokens[1],
                owners[0].pubkey(),
                owners[1].pubkey(),
                env.admin.pubkey(),
                matcher,
                env.program_id,
                spl_token::ID,
                solana_sdk::sysvar::clock::ID,
            ];
            let frame = |env: &V16CuEnv| keys.map(|key| env.svm.get_account(&key));
            let initial = frame(&env);
            for (index, tx) in requests.iter().enumerate() {
                tx.verify().unwrap();
                assert!(signed_bytes[index].len() <= solana_sdk::packet::PACKET_DATA_SIZE);
                env.svm.simulate_transaction(tx.clone().into()).unwrap();
                assert_eq!(frame(&env), initial, "{label}: simulation is read-only");
            }
            let check = |env: &V16CuEnv, quantity: i128, fees: u128, paid: [u128; 2]| {
                let accounts = portfolios.map(|key| env.portfolio_state(key));
                for actor in 0..2 {
                    assert_eq!(accounts[actor].owner, owners[actor].pubkey().to_bytes());
                    assert_eq!(
                        accounts[actor].capital.get(),
                        DEPOSITS[actor] - fees - paid[actor]
                    );
                    assert_eq!(
                        accounts[actor].pnl.get(),
                        0,
                        "{label}: no hidden fee in PnL"
                    );
                    assert_eq!(u128::from(env.token_amount(tokens[actor])), paid[actor]);
                    if quantity == 0 {
                        assert!(!has_active_leg_for_asset(&accounts[actor], 0));
                    } else {
                        assert_eq!(
                            active_leg_for_asset(&accounts[actor], 0).basis_pos_q,
                            if actor == 0 { quantity } else { -quantity },
                        );
                    }
                }
                let (_, group) = env.market_state();
                assert_eq!(group.assets[0].effective_price, PRICE);
                assert_eq!(group.assets[0].oi_eff_long_q, quantity.unsigned_abs());
                assert_eq!(group.assets[0].oi_eff_short_q, quantity.unsigned_abs());
                assert_eq!(group.insurance, 2 * fees);
                assert_eq!(&group.insurance_domain_budget[..2], &[fees; 2]);
                assert_eq!(
                    group.c_tot,
                    DEPOSITS.iter().sum::<u128>() - 2 * fees - paid.iter().sum::<u128>()
                );
                assert_eq!(
                    group.vault,
                    DEPOSITS.iter().sum::<u128>() - paid.iter().sum::<u128>()
                );
                assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                assert_market_stock_census(
                    &label,
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    env.token_amount(env.vault).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                for (index, key) in keys.iter().enumerate() {
                    if ![
                        env.market,
                        portfolios[0],
                        portfolios[1],
                        context,
                        env.vault,
                        tokens[0],
                        tokens[1],
                    ]
                    .contains(key)
                    {
                        assert_eq!(
                            env.svm.get_account(key),
                            initial[index],
                            "{label}: passive {key}"
                        );
                    }
                }
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(u128::from(mint.supply), DEPOSITS.iter().sum::<u128>());
                assert_eq!(mint.mint_authority, COption::None);
            };
            check(&env, 0, 0, [0; 2]);
            let policy_sequence = env.control_sequences(0).trade_fee;
            env.update_trade_fee_policy_with_cu(current_bps);
            assert_eq!(env.control_sequences(0).trade_fee, policy_sequence + 1);
            assert_eq!(env.market_state().0.trade_fee_base_bps, current_bps);
            control(&mut env, vec![11, 19, 127]);
            check(&env, 0, 0, [0; 2]);
            assert_eq!(
                env.portfolio_matcher_sequence(portfolios[1]),
                matcher_sequence
            );
            assert_eq!(
                portfolios.map(|key| env.portfolio_position_epoch(key)),
                epochs
            );
            for (index, tx) in requests.iter().enumerate() {
                assert_eq!(bincode::serialize(tx).unwrap(), signed_bytes[index]);
            }

            let send_retained = |env: &mut V16CuEnv, index: usize| {
                let tx = requests[index].clone();
                assert_eq!(bincode::serialize(&tx).unwrap(), signed_bytes[index]);
                tx.verify().unwrap();
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let result = env.svm.send_transaction(tx);
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                result
            };
            if batch {
                let before = frame(&env);
                let failure = send_retained(&mut env, 0).expect_err("a batch requires exact fills");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(2, InstructionError::InvalidAccountData)
                );
                assert!(failure
                    .meta
                    .logs
                    .contains(&format!("Program {matcher} success")));
                assert_eq!(
                    frame(&env),
                    before,
                    "{label}: matcher and economic Account rollback"
                );
                peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
                check(&env, 0, 0, [0; 2]);
                control(&mut env, vec![11, 9, 0]);
                check(&env, 0, 0, [0; 2]);
            }
            let accepted = send_retained(&mut env, usize::from(batch))
                .expect("original signed bounds remain live under the changed policy");
            peak_cu = peak_cu.max(accepted.compute_units_consumed);
            let filled = if batch {
                QUANTITY
            } else {
                QUANTITY * 127 / 255
            };
            let fees = fee(filled, current_bps);
            assert!(fees > 0 && fees < fee(filled, SIGNED_BPS));
            assert_ne!(fees, fee(filled, OLD_BPS));
            if !batch {
                assert!(filled > 0 && filled < QUANTITY);
                assert_ne!(filled.unsigned_abs() % POS_SCALE, 0);
                assert!(fees < fee(QUANTITY, current_bps));
                let context_account = env.svm.get_account(&context).unwrap();
                let response = read_matcher_return(&context_account.data).unwrap();
                assert_eq!(response.exec_size, filled);
                assert_eq!(response.exec_price_e6, PRICE);
                assert_ne!(response.flags & FLAG_PARTIAL_OK, 0);
            }
            assert_eq!(
                portfolios.map(|key| env.portfolio_position_epoch(key)),
                epochs.map(|epoch| epoch + 1)
            );
            check(&env, filled, fees, [0; 2]);

            // Close only the executed quantity, then realize each owner's independently priced entitlement.
            env.update_trade_fee_policy_with_cu(0);
            check(&env, filled, fees, [0; 2]);
            env.send(
                env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, -filled, PRICE, 0),
                vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ],
                &[&owners[0], &owners[1]],
            )
            .unwrap();
            check(&env, 0, fees, [0; 2]);
            let mut paid = [0; 2];
            for actor in 0..2 {
                let amount = DEPOSITS[actor] - fees;
                env.send(
                    env.withdraw_ix(portfolios[actor], amount),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
                paid[actor] = amount;
                check(&env, 0, fees, paid);
            }
            assert_eq!(u128::from(env.token_amount(env.vault)), 2 * fees);
        }
    }
    assert_cu_within("retained repriced partial/exact fill", peak_cu, 1_400_000);
    println!("INV-014 fee/capacity: 4 histories, 2 exact rollbacks, 4 fills, 8 owner payouts; peak CU={peak_cu}");
}

fn send_admin_control(env: &mut V16CuEnv, instruction: ProgInstruction) -> Result<u64, String> {
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        instruction,
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
}

#[test]
fn v16_control_sequences_accept_gaps_reject_replays_and_keep_lanes_independent() {
    let mut env = V16CuEnv::new();

    send_admin_control(
        &mut env,
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 123,
            policy_sequence: 17,
            authority_epoch: 0,
        },
    )
    .expect("a forward sequence gap must be accepted");
    assert_eq!(env.control_sequences(0).trade_fee, 17);
    assert_eq!(env.market_state().0.trade_fee_base_bps, 123);

    for stale_sequence in [0, 16, 17] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let result = send_admin_control(
            &mut env,
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: 999,
                policy_sequence: stale_sequence,
                authority_epoch: 0,
            },
        );
        assert!(
            result.is_err(),
            "sequence {stale_sequence} must not overwrite committed sequence 17"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "a rejected stale control must roll the complete market account back"
        );
    }

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 1_000,
            policy_sequence: 1,
            authority_epoch: 0,
        },
    )
    .expect("an unrelated policy lane starts at its own sequence");
    let sequences = env.control_sequences(0);
    assert_eq!(sequences.trade_fee, 17);
    assert_eq!(sequences.liquidation_fee, 1);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.trade_fee_base_bps, 123);
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 1_000);

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 321,
            policy_sequence: 1_000_000,
            authority_epoch: 0,
        },
    )
    .expect("large forward gaps must remain valid for out-of-order landing");
    assert_eq!(env.control_sequences(0).trade_fee, 1_000_000);
    assert_eq!(env.market_state().0.trade_fee_base_bps, 321);
}

#[test]
fn v16_oracle_modes_share_one_supersession_sequence() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    send_admin_control(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 2,
            authority_epoch: 0,
        },
    )
    .expect("new EWMA control");

    let ewma_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_auth = send_admin_control(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            observation_sequence: 1,
            authority_epoch: 0,
        },
    );
    assert!(
        stale_auth.is_err(),
        "a stale cross-mode control must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), ewma_before);

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            observation_sequence: 3,
            authority_epoch: 0,
        },
    )
    .expect("newer authenticated-mark control");

    let feed = [7u8; 32];
    let clock = env.svm.get_sysvar::<Clock>();
    let pyth = env.set_pyth_price(&feed, 300, 0, clock.unix_timestamp);
    let mut feeds = [[0u8; 32]; percolator_prog::constants::ORACLE_LEG_CAP];
    feeds[0] = feed;

    let auth_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_hybrid = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: clock.unix_timestamp,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
            observation_sequence: 2,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&env.admin],
    );
    assert!(stale_hybrid.is_err(), "stale hybrid control must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), auth_before);

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: clock.unix_timestamp,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
            observation_sequence: 4,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&env.admin],
    )
    .expect("newer hybrid control");

    let hybrid_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_ewma = send_admin_control(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 400,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 3,
            authority_epoch: 0,
        },
    );
    assert!(stale_ewma.is_err(), "stale EWMA control must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), hybrid_before);

    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS
    );
    assert_eq!(env.control_sequences(0).oracle_observation, 4);
}

// owner's charge. The default market is manual-priced, so only the configured base fee applies.
#[test]
fn v16_program_trade_requires_signed_base_fee_consent() {
    let mut env = V16CuEnv::new();
    env.update_trade_fee_policy_with_cu(500); // config base fee = 5%
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let ins0 = env.market_state().1.insurance;

    let market_before = env.svm.get_account(&env.market).unwrap();
    let a_before = env.svm.get_account(&pa).unwrap();
    let b_before = env.svm.get_account(&pb).unwrap();

    env.svm.expire_blockhash();
    let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    assert!(
        r.is_err(),
        "fee_bps below the live base must reject rather than evade or silently increase: {r:?}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&pa).unwrap(), a_before);
    assert_eq!(env.svm.get_account(&pb).unwrap(), b_before);

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 500);

    let (_, g1) = env.market_state();
    assert!(
        g1.insurance > ins0,
        "a trade that signs the configured base must pay it; \
         insurance {ins0} -> {}",
        g1.insurance
    );
    assert_eq!(
        g1.vault,
        g1.c_tot + g1.insurance,
        "exact conservation after the consented base-fee trade"
    );
}

#[test]
fn v16_retained_cpi_price_limit_survives_oracle_policy_change() {
    use percolator_prog::matcher_abi::read_matcher_return;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const DEPOSIT: u64 = 10_000;
    const PREFIX: u64 = 17;
    const SIGNED_PRICE: u64 = 100;
    const QUANTITY: u128 = 10 * POS_SCALE;
    const BUNDLE_CU_LIMIT: u64 = 400_000;

    // Permit isolated worktrees to reuse the built fixture without changing tests/fixtures.
    let matcher_path = std::env::var_os("PERCOLATOR_AUTH_MATCHER_SBF")
        .map(PathBuf::from)
        .unwrap_or_else(auth_matcher_program_path);
    let matcher_bytes = std::fs::read(matcher_path).expect("read authenticated matcher SBF");
    let mut peak_cu = 0;
    for direction in [-1i128, 1] {
        for adverse in [false, true] {
            let mut env =
                inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market(6);
            let owners = [Keypair::new(), Keypair::new()];
            let mut portfolios = Vec::new();
            let mut sources = Vec::new();
            for owner in &owners {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(&env.payer.pubkey(), &owner.pubkey(), 1_000_000),
                    &[],
                )
                .unwrap();
                let portfolio = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &portfolio,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio.pubkey(), false),
                    ],
                    &[owner],
                )
                .unwrap();
                let source =
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &source,
                        &env.admin.pubkey(),
                        &[],
                        DEPOSIT + PREFIX,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolio.pubkey(), DEPOSIT.into()),
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio.pubkey(), false),
                        AccountMeta::new(source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[owner],
                )
                .unwrap();
                portfolios.push(portfolio.pubkey());
                sources.push(source);
            }
            let (taker, lp) = (portfolios[0], portfolios[1]);
            set_test_clock(&mut env, 1, 100);
            env.configure_auth_mark_with_cu(1, SIGNED_PRICE);
            let matcher = Pubkey::new_unique();
            env.svm.add_program(matcher, &matcher_bytes);
            let (context, delegate, _) =
                env.init_auth_matcher_context_via_system_create(matcher, &owners[1], lp);
            let size = direction * QUANTITY as i128;
            let retained_instruction = env.trade_cpi_ix(taker, lp, 0, size, 0, SIGNED_PRICE);
            let prefix = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(sources[0], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.deposit_ix(taker, PREFIX.into()).encode(),
            };
            let transaction = |env: &V16CuEnv, instruction: &ProgInstruction| {
                Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        cu_ix(),
                        prefix.clone(),
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(owners[0].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(taker, false),
                                AccountMeta::new(lp, false),
                                AccountMeta::new_readonly(matcher, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            data: instruction.encode(),
                        },
                    ],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owners[0]],
                    env.svm.latest_blockhash(),
                )
            };
            let retained = transaction(&env, &retained_instruction);
            retained.verify().unwrap();
            let signed_bytes = bincode::serialize(&retained).unwrap();
            assert!(signed_bytes.len() <= solana_sdk::packet::PACKET_DATA_SIZE);
            let frame_keys = [
                env.market,
                taker,
                lp,
                context,
                delegate,
                env.vault,
                env.mint,
                sources[0],
                sources[1],
                owners[0].pubkey(),
                owners[1].pubkey(),
                env.admin.pubkey(),
                matcher,
                env.program_id,
                spl_token::ID,
            ];
            let frame = |env: &V16CuEnv| frame_keys.map(|key| env.svm.get_account(&key));
            let before = frame(&env);
            env.svm
                .simulate_transaction(retained.clone().into())
                .expect("the signed bundle is executable under the original oracle policy");
            assert_eq!(
                frame(&env),
                before,
                "simulation cannot consume the prefix or fill"
            );

            let price_delta = direction * if adverse { 10 } else { -10 };
            let landing_price = (SIGNED_PRICE as i128 + price_delta) as u64;
            let old_sequence = env.control_sequences(0).oracle_observation;
            // No blockhash expiry or re-signing: only the authorized oracle policy changes.
            env.configure_ewma_mark_with_cu(1, landing_price, 1, 0);
            assert_eq!(
                env.control_sequences(0).oracle_observation,
                old_sequence + 1
            );
            assert_eq!(
                env.market_state().0.oracle_mode,
                percolator_prog::constants::ORACLE_MODE_EWMA_MARK
            );
            assert_eq!(
                env.market_state().1.assets[0].effective_price,
                landing_price
            );
            assert_eq!(transaction(&env, &retained_instruction), retained);
            assert_eq!(bincode::serialize(&retained).unwrap(), signed_bytes);

            let before = frame(&env);
            let mut expected_payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            let network_fee = FeeStructure::default().lamports_per_signature
                * u64::from(retained.message.header.num_required_signatures);
            let result = env.svm.send_transaction(retained);
            let accepted = if adverse {
                let error =
                    result.expect_err("new oracle policy cannot relax the signed price limit");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                    ),
                );
                assert!(error
                    .meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)));
                assert!(error
                    .meta
                    .logs
                    .contains(&format!("Program {} success", env.program_id)));
                assert!(error
                    .meta
                    .logs
                    .contains(&format!("Program {matcher} success")));
                assert_eq!(
                    frame(&env),
                    before,
                    "deposit, custody, matcher response and all wrapper bytes roll back"
                );
                expected_payer.lamports -= network_fee;
                assert_eq!(
                    env.svm.get_account(&env.payer.pubkey()).unwrap(),
                    expected_payer
                );
                assert_cu_within(
                    "retained oracle-policy rejection",
                    error.meta.compute_units_consumed,
                    BUNDLE_CU_LIMIT,
                );
                peak_cu = peak_cu.max(error.meta.compute_units_consumed);

                // Only the price limit changes; rejected prefix/position/matcher sequences remain usable.
                let mut fresh = retained_instruction.clone();
                let ProgInstruction::TradeCpi { limit_price, .. } = &mut fresh else {
                    unreachable!()
                };
                *limit_price = landing_price;
                env.svm
                    .send_transaction(transaction(&env, &fresh))
                    .expect("fresh exact-price consent executes immediately after stale rejection")
            } else {
                result.expect("a newer favorable oracle policy need not invalidate bounded consent")
            };
            assert_cu_within(
                "oracle-policy bounded bundle",
                accepted.compute_units_consumed,
                BUNDLE_CU_LIMIT,
            );
            peak_cu = peak_cu.max(accepted.compute_units_consumed);
            expected_payer.lamports -= network_fee;
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()).unwrap(),
                expected_payer
            );
            let context_account = env.svm.get_account(&context).unwrap();
            let fill = read_matcher_return(&context_account.data).unwrap();
            assert_eq!(fill.exec_price_e6, landing_price);
            assert_eq!(fill.oracle_price_e6, landing_price);
            assert_eq!(fill.exec_size, size);
            let accepted_limit = if adverse { landing_price } else { SIGNED_PRICE };
            assert!(direction * (fill.exec_price_e6 as i128 - accepted_limit as i128) <= 0);
            let (_, group) = env.market_state();
            assert_eq!(group.assets[0].oi_eff_long_q, QUANTITY);
            assert_eq!(group.assets[0].oi_eff_short_q, QUANTITY);
            assert_eq!(group.insurance, 0);
            assert_eq!(group.c_tot, u128::from(2 * DEPOSIT + PREFIX));
            assert_eq!(group.vault, group.c_tot);
            assert_eq!(env.token_amount(env.vault), 2 * DEPOSIT + PREFIX);
            assert_eq!(env.token_amount(sources[0]), 0);
            assert_eq!(env.token_amount(sources[1]), PREFIX);
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply,
                2 * (DEPOSIT + PREFIX)
            );
            for (index, portfolio) in portfolios.iter().copied().enumerate() {
                let account = env.portfolio_state(portfolio);
                assert_eq!(
                    account.capital.get(),
                    u128::from(DEPOSIT + if index == 0 { PREFIX } else { 0 })
                );
                assert_eq!(account.pnl.get(), 0);
                assert_eq!(
                    account.legs[0].basis_pos_q.get(),
                    if index == 0 { size } else { -size }
                );
            }
        }
    }
    eprintln!(
        "INV-014 retained oracle policy: 4 histories, 2 exact rollbacks, peak bundle CU {peak_cu}"
    );
}
