//! INV-014: retained single-trade backing consent across matcher cap changes.
//! Secondary INV-036/047/081: participant-local caps, provider SPL earnings,
//! exact failed-CPI rollback, and same/cross-route retained alternatives.
//! Base-policy repricing also turns a capital-funded increase into a source-
//! backed increase: unused base-fee consent cannot authorize that new fee class.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm},
};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::{
    error::PercolatorError, ix::CrankObservationHint, processor::ASSET_AUTH_BACKING_BUCKET, state,
};
use solana_sdk::{
    account::Account,
    instruction::InstructionError,
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};

const ASSET: u16 = 1;
const DOMAIN: usize = 3;
const PROVIDER: usize = 2;
const RATE: u16 = 3_333;
const DEPOSITS: [u128; 5] = [52_502, 2_000_000, 0, 0, 0];
const OPEN: i128 = 1_000 * POS_SCALE as i128;
const INCREASE: i128 = 50 * POS_SCALE as i128;

fn frame(env: &V16Svm, tx: Option<&Transaction>) -> Vec<(Pubkey, Option<Account>)> {
    let mut keys = vec![
        env.market,
        env.foreign_market,
        env.vault,
        env.foreign_vault,
        env.mint,
        env.backing_domain_ledger,
        env.provider_source_token,
        env.provider_destination_token,
        env.market_admin_destination_token,
        env.foreign_actor.portfolio,
        env.foreign_actor.source_token,
        env.foreign_actor.destination_token,
        env.program_id,
        env.matcher_program,
        spl_token::ID,
    ];
    for actor in &env.actors {
        keys.extend([
            actor.signer.pubkey(),
            actor.portfolio,
            actor.source_token,
            actor.destination_token,
            actor.matcher_context,
            actor.matcher_delegate,
        ]);
    }
    if let Some(tx) = tx {
        keys.extend(tx.message.account_keys.iter().copied());
        // Network fees are outside the protocol's atomic economic frame.
        keys.retain(|key| *key != tx.message.account_keys[0]);
    }
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn simulate(env: &mut V16Svm, tx: &Transaction) {
    tx.verify().unwrap();
    assert!(bincode::serialized_size(tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    let before = frame(env, Some(tx));
    env.svm.simulate_transaction(tx.clone().into()).unwrap();
    assert_eq!(
        frame(env, Some(tx)),
        before,
        "simulation cannot consume consent"
    );
}

fn checkpoint(env: &mut V16Svm) -> usize {
    let trace = env.finish_public_trace();
    if !trace.steps.is_empty() {
        trace.validate_public_execution().unwrap();
    }
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    trace.steps.len()
}

fn set_cap(env: &mut V16Svm, lp: usize, cap: u16) -> usize {
    let steps = checkpoint(env);
    let context = env.actors[lp].matcher_context;
    let unchanged = |env: &V16Svm| {
        frame(env, None)
            .into_iter()
            .filter(|(key, _)| *key != context)
            .collect::<Vec<_>>()
    };
    let before = unchanged(env);
    let mut expected_context = env.svm.get_account(&context).unwrap();
    // The external matcher has its own owner-signed policy API. Frame all
    // protocol/SPL accounts around it, then resume the wrapper-only trace.
    env.set_matcher_backing_fee_cap(lp, cap).unwrap();
    assert_eq!(unchanged(env), before);
    let actual_context = env.svm.get_account(&context).unwrap();
    assert_eq!(actual_context.owner, env.matcher_program);
    assert_eq!(actual_context.data.len(), expected_context.data.len());
    expected_context.data = actual_context.data.clone();
    assert_eq!(actual_context, expected_context);
    env.begin_public_trace();
    steps + 1
}

fn settle(env: &mut V16Svm, slot: u64) {
    for actor in [1, 0] {
        let mut complete = false;
        for _ in 0..8 {
            if env
                .crank_if_actionable(
                    actor,
                    slot,
                    vec![CrankObservationHint {
                        asset_index: ASSET,
                        oracle_accounts: 0,
                    }],
                )
                .unwrap()
                .is_none()
            {
                complete = true;
                break;
            }
        }
        assert!(complete, "bounded public source settlement");
    }
}

fn census(env: &V16Svm, supply: u128) {
    assert_eq!(env.token_supply_observed(), supply);
    assert_public_stock_census("retained backing cap", env).unwrap();
    assert_public_encumbrance_census("retained backing cap", env).unwrap();
}

#[test]
fn v16_program_retained_backing_fee_caps_follow_participant_and_route_consent() {
    let mut endpoint = None;
    let mut worlds = 0;
    let mut rejections = 0;
    let mut transactions = 0;
    let mut peak_cu = 0;
    for fee_payer_is_lp in [false, true] {
        for land_cpi in [false, true] {
            let label = format!("fee_payer_is_lp={fee_payer_is_lp}, land_cpi={land_cpi}");
            let mut env = V16Svm::new(
                [0x7c; 32],
                MarketConfig {
                    initial_price: 100,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    actor_deposits: DEPOSITS,
                    ..MarketConfig::default()
                },
            );
            let supply = env.token_supply_observed();
            env.begin_public_trace();
            let handoff = env.build_retained_asset_authority_handoff_from_admin(
                ASSET,
                ASSET_AUTH_BACKING_BUCKET,
                PROVIDER,
            );
            env.land_retained(handoff).unwrap();
            env.update_backing_fee_policy(DOMAIN as u16, RATE, 0)
                .unwrap();
            env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN as u16, 100_000, 100)
                .unwrap();
            env.trade_no_cpi(0, 1, ASSET, OPEN, 100, 0).unwrap();
            env.warp_to_slot(2);
            env.push_auth_mark(ASSET, 2, 105).unwrap();
            settle(&mut env, 2);
            assert_eq!(env.primary_portfolio(0).pnl.get(), 5_000);
            assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0]);
            assert_eq!(env.primary_portfolio(1).capital.get(), DEPOSITS[1] - 5_000);
            assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
            let (taker, lp, size) = if fee_payer_is_lp {
                (1, 0, -INCREASE)
            } else {
                (0, 1, INCREASE)
            };
            env.set_matcher_config_with_trade_fee_cap(lp, 1, 0).unwrap();
            transactions += set_cap(&mut env, lp, RATE);
            let grant = state::read_portfolio_matcher_config(
                &env.svm.get_account(&env.actors[lp].portfolio).unwrap().data,
            )
            .unwrap();
            let epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));

            // The taker cap is deliberately zero when only the LP will pay a source fee.
            let mut retain_cpi = || {
                env.build_retained_cpi_trade_with_backing_fee_cap(
                    taker,
                    lp,
                    ASSET,
                    size,
                    105,
                    if fee_payer_is_lp { 0 } else { RATE },
                )
            };
            let probe = retain_cpi();
            let cpi = retain_cpi();
            let bilateral = env.build_retained_no_cpi_trade_with_fee_and_backing_cap(
                taker, lp, ASSET, size, 105, 0, RATE,
            );
            let cpi_bytes = bincode::serialize(&cpi).unwrap();
            let bilateral_bytes = bincode::serialize(&bilateral).unwrap();
            assert_eq!(cpi.message.header.num_required_signatures, 2);
            assert!(!cpi.message.account_keys[..2].contains(&env.actors[lp].signer.pubkey()));
            assert_eq!(bilateral.message.header.num_required_signatures, 3);
            for tx in [&probe, &cpi, &bilateral] {
                simulate(&mut env, tx);
            }
            let before_bucket = env.primary_market_state().1.source_backing_buckets[DOMAIN];
            assert_eq!(before_bucket.utilization_fee_earnings, 0);
            census(&env, supply);

            transactions += set_cap(&mut env, lp, RATE - 1);
            assert_eq!(
                state::read_portfolio_matcher_config(
                    &env.svm.get_account(&env.actors[lp].portfolio).unwrap().data,
                )
                .unwrap(),
                grant,
                "{label}: matcher cap changes without rewriting wrapper grant or sequence"
            );
            assert_eq!(
                [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                epochs
            );
            if fee_payer_is_lp {
                let before = frame(&env, Some(&probe));
                let expected = TransactionError::InstructionError(
                    (probe.message.instructions.len() - 1) as u8,
                    InstructionError::Custom(PercolatorError::Unauthorized as u32),
                );
                let error = env.land_retained(probe.clone()).unwrap_err();
                assert!(error.contains(&format!("{expected:?}")), "{label}: {error}");
                assert!(error.contains(&format!("Program {} success", env.matcher_program)));
                assert_eq!(
                    frame(&env, Some(&probe)),
                    before,
                    "{label}: post-CPI rollback"
                );
                rejections += 1;
            } else {
                simulate(&mut env, &probe);
            }
            simulate(&mut env, &bilateral);
            census(&env, supply);

            // A bilateral alternative keeps both signatures. A delegated LP fee needs
            // the LP's current matcher cap restored before its already-signed CPI lands.
            if fee_payer_is_lp && land_cpi {
                transactions += set_cap(&mut env, lp, RATE);
            }
            assert_eq!(bincode::serialize(&cpi).unwrap(), cpi_bytes);
            assert_eq!(bincode::serialize(&bilateral).unwrap(), bilateral_bytes);
            let selected = if land_cpi { cpi } else { bilateral };
            peak_cu = peak_cu.max(env.land_retained(selected).unwrap().compute_units);
            let group = env.primary_market_state().1;
            let bucket = group.source_backing_buckets[DOMAIN];
            let lien_delta =
                bucket.valid_liened_backing_num - before_bucket.valid_liened_backing_num;
            assert!(lien_delta > 0);
            let denominator = BOUND_SCALE * 10_000;
            let numerator = lien_delta * u128::from(RATE);
            let fee = numerator / denominator + u128::from(numerator % denominator != 0);
            assert!(fee > 1, "{label}: exercise a nonzero source fee");
            assert_eq!(bucket.utilization_fee_earnings, fee);
            assert_eq!(group.insurance, 0);
            assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0] - fee);
            assert_eq!(env.primary_portfolio(1).capital.get(), DEPOSITS[1] - 5_000);
            assert_eq!(
                group.assets[ASSET as usize].oi_eff_long_q,
                (OPEN + INCREASE) as u128
            );
            assert_eq!(
                group.assets[ASSET as usize].oi_eff_short_q,
                (OPEN + INCREASE) as u128
            );
            census(&env, supply);

            let destination = env.actors[PROVIDER].destination_token;
            let tokens_before = env.token_amount(destination);
            let vault_before = env.token_amount(env.vault);
            env.withdraw_backing_bucket_earnings_for_actor(PROVIDER, DOMAIN as u16, fee)
                .unwrap();
            assert_eq!(env.token_amount(destination) - tokens_before, fee as u64);
            assert_eq!(vault_before - env.token_amount(env.vault), fee as u64);
            census(&env, supply);

            env.warp_to_slot(3);
            env.push_auth_mark(ASSET, 3, 100).unwrap();
            settle(&mut env, 3);
            if land_cpi {
                env.trade_no_cpi(0, 1, ASSET, -OPEN - INCREASE, 100, 0)
                    .unwrap();
            } else {
                env.set_matcher_config_with_trade_fee_cap(1, 1, 0).unwrap();
                transactions += set_cap(&mut env, 1, 0);
                let close = env.build_retained_cpi_trade(0, 1, ASSET, -OPEN - INCREASE, 100);
                env.land_retained(close).unwrap();
            }
            settle(&mut env, 3);
            let values = [0, 1].map(|actor| {
                let account = env.primary_portfolio(actor);
                account.capital.get() as i128 + account.pnl.get()
            });
            // The new 50 lots lose five atoms each when the mark returns to 100.
            assert_eq!(
                values,
                [
                    DEPOSITS[0] as i128 - fee as i128 - 250,
                    DEPOSITS[1] as i128 + 250
                ]
            );
            let final_group = env.primary_market_state().1;
            assert_eq!(final_group.assets[ASSET as usize].oi_eff_long_q, 0);
            assert_eq!(final_group.assets[ASSET as usize].oi_eff_short_q, 0);
            assert_eq!(
                final_group.source_backing_buckets[DOMAIN].utilization_fee_earnings,
                0
            );
            assert_eq!(final_group.insurance, 0);
            census(&env, supply);
            transactions += checkpoint(&mut env);
            let actual = (
                fee,
                values,
                env.token_amount(destination),
                env.token_amount(env.vault),
            );
            if let Some(expected) = endpoint {
                assert_eq!(actual, expected, "{label}: route/role entitlement");
            } else {
                endpoint = Some(actual);
            }
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert_eq!(rejections, 2);
    eprintln!("INV-014 retained backing caps: worlds={worlds}, public_txs={transactions}, exact_rollbacks={rejections}, peak_trade_cu={peak_cu}, endpoint={endpoint:?}");
}

#[test]
fn v16_program_retained_base_fee_consent_cannot_authorize_new_backing_fees() {
    use percolator_prog::ix::Instruction as ProgInstruction;
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        instruction::{AccountMeta, Instruction},
        signature::Keypair,
    };

    const BASE_BPS: u64 = 100;
    const MARK: u64 = 105;
    const BACKING: u128 = 100_000;
    let margin = ((OPEN + INCREASE) as u128 * u128::from(MARK) / POS_SCALE) / 2;
    let deposits = [margin, DEPOSITS[1], 0, 0, 0];
    let base_fee = ((INCREASE as u128 * u128::from(MARK)).div_ceil(POS_SCALE)
        * u128::from(BASE_BPS))
    .div_ceil(10_000);
    // Capital exactly covers the enlarged position before repricing. The base
    // fee creates the entire margin shortfall, funded from counterparty backing.
    let source_fee = (base_fee * u128::from(RATE)).div_ceil(10_000);
    assert_eq!((margin, base_fee, source_fee), (55_125, 53, 18));
    let mut peak_cu = 0;
    for fee_payer_is_lp in [false, true] {
        let mut env = V16Svm::new(
            [0x7d; 32],
            MarketConfig {
                initial_price: 100,
                initial_margin_bps: 5_000,
                maintenance_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_accrual_dt_slots: 1,
                min_funding_lifetime_slots: 1,
                actor_deposits: deposits,
                ..MarketConfig::default()
            },
        );
        let supply = env.token_supply_observed();
        env.begin_public_trace();
        env.update_asset_authority_from_admin(ASSET, ASSET_AUTH_BACKING_BUCKET, PROVIDER)
            .unwrap();
        env.update_backing_fee_policy(DOMAIN as u16, RATE, 0)
            .unwrap();
        env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN as u16, BACKING, 100)
            .unwrap();
        env.trade_no_cpi(0, 1, ASSET, OPEN, 100, 0).unwrap();
        env.warp_to_slot(2);
        env.push_auth_mark(ASSET, 2, MARK).unwrap();
        settle(&mut env, 2);
        let (taker, lp, size) = if fee_payer_is_lp {
            (1, 0, -INCREASE)
        } else {
            (0, 1, INCREASE)
        };
        env.set_matcher_config_with_trade_fee_cap(lp, 1, BASE_BPS as u16)
            .unwrap();
        set_cap(&mut env, lp, 0);
        let payer = Keypair::new();
        env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
        let retain = |env: &V16Svm, cap, nonce| {
            let ix = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[taker].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[taker].portfolio, false),
                    AccountMeta::new(env.actors[lp].portfolio, false),
                    AccountMeta::new_readonly(env.matcher_program, false),
                    AccountMeta::new(env.actors[lp].matcher_context, false),
                    AccountMeta::new_readonly(env.actors[lp].matcher_delegate, false),
                ],
                data: ProgInstruction::TradeCpi {
                    account_a_portfolio_id: env.primary_portfolio_id(taker),
                    account_a_position_epoch: env.primary_portfolio_position_epoch(taker),
                    account_b_portfolio_id: env.primary_portfolio_id(lp),
                    account_b_position_epoch: env.primary_portfolio_position_epoch(lp),
                    account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(lp),
                    asset_index: ASSET,
                    market_id: env.primary_market_state().1.assets[ASSET as usize].market_id,
                    size_q: size,
                    fee_bps: BASE_BPS,
                    limit_price: MARK,
                    backing_fee_cap_bps: cap,
                }
                .encode(),
            };
            Transaction::new_signed_with_payer(
                &[
                    ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                    ComputeBudgetInstruction::set_compute_unit_limit(
                        crate::support::v16_svm::TX_CU_LIMIT as u32 - nonce,
                    ),
                    ix,
                ],
                Some(&payer.pubkey()),
                &[&payer, &env.actors[taker].signer],
                env.svm.latest_blockhash(),
            )
        };
        let probe = retain(&env, 0, 0);
        let retained = retain(&env, 0, 1);
        let bytes = bincode::serialize(&retained).unwrap();
        simulate(&mut env, &retained);
        let epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));
        let grant = state::read_portfolio_matcher_config(&env.primary_portfolio_data(lp)).unwrap();
        let sequence = env.primary_portfolio_matcher_sequence(lp);
        let policy_sequence = env.primary_control_sequences(0).trade_fee;
        let before_tokens = env.all_token_account_data();
        let vault = env.token_amount(env.vault);
        assert_eq!(env.primary_portfolio(0).capital.get(), margin);
        assert_eq!(env.primary_portfolio(0).pnl.get(), 5_000);
        assert_eq!(env.primary_portfolio(1).capital.get(), deposits[1] - 5_000);
        assert_eq!(env.primary_market_state().1.insurance, 0);
        assert_eq!(
            env.primary_market_state().1.source_backing_buckets[DOMAIN].valid_liened_backing_num,
            0
        );

        env.update_trade_fee_policy(BASE_BPS).unwrap();
        let before = frame(&env, Some(&probe));
        let error = env.land_retained(probe.clone()).unwrap_err();
        let expected = TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::Unauthorized as u32),
        );
        assert!(error.contains(&format!("{expected:?}")), "{error}");
        assert!(error.contains(&format!("Program {} success", env.matcher_program)));
        assert_eq!(frame(&env, Some(&probe)), before);
        census(&env, supply);

        // A lower base policy removes source consumption without renewing consent.
        env.update_trade_fee_policy(0).unwrap();
        simulate(&mut env, &retained);
        env.update_trade_fee_policy(BASE_BPS).unwrap();
        let selected = if fee_payer_is_lp {
            set_cap(&mut env, lp, RATE);
            retained.clone()
        } else {
            retain(&env, RATE, 1)
        };
        assert_eq!(bincode::serialize(&retained).unwrap(), bytes);
        assert_eq!(env.primary_portfolio_matcher_sequence(lp), sequence);
        assert_eq!(
            state::read_portfolio_matcher_config(&env.primary_portfolio_data(lp)).unwrap(),
            grant
        );
        let before_success = frame(&env, Some(&selected));
        peak_cu = peak_cu.max(env.land_retained(selected).unwrap().compute_units);
        for (key, account) in before_success {
            if ![
                env.market,
                env.actors[0].portfolio,
                env.actors[1].portfolio,
                env.actors[lp].matcher_context,
            ]
            .contains(&key)
            {
                assert_eq!(
                    env.svm.get_account(&key),
                    account,
                    "untouched account: {key}"
                );
            }
        }
        let group = env.primary_market_state().1;
        let bucket = group.source_backing_buckets[DOMAIN];
        assert_eq!(bucket.valid_liened_backing_num, base_fee * BOUND_SCALE);
        assert_eq!(bucket.utilization_fee_earnings, source_fee);
        assert_eq!(group.insurance, 2 * base_fee);
        assert_eq!(
            &group.insurance_domain_budget[..4],
            &[0, 0, base_fee, base_fee]
        );
        assert_eq!(
            env.primary_portfolio(0).capital.get(),
            margin - base_fee - source_fee
        );
        assert_eq!(env.primary_portfolio(0).pnl.get(), 5_000);
        assert_eq!(
            env.primary_portfolio(1).capital.get(),
            deposits[1] - 5_000 - base_fee
        );
        assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
        assert_eq!(
            group.c_tot,
            deposits.iter().sum::<u128>() - 5_000 - 2 * base_fee - source_fee
        );
        assert_eq!(
            group.assets[ASSET as usize].oi_eff_long_q,
            (OPEN + INCREASE) as u128
        );
        assert_eq!(
            group.assets[ASSET as usize].oi_eff_short_q,
            (OPEN + INCREASE) as u128
        );
        assert_eq!(
            [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
            epochs.map(|e| e + 1)
        );
        assert_eq!(
            env.primary_control_sequences(0).trade_fee,
            policy_sequence + 3
        );
        assert_eq!(env.all_token_account_data(), before_tokens);
        assert_eq!(u128::from(vault), group.vault);
        census(&env, supply);

        let destination = env.actors[PROVIDER].destination_token;
        let paid_before = env.token_amount(destination);
        env.withdraw_backing_bucket_earnings_for_actor(PROVIDER, DOMAIN as u16, source_fee)
            .unwrap();
        assert_eq!(
            env.token_amount(destination) - paid_before,
            source_fee as u64
        );
        assert_eq!(vault - env.token_amount(env.vault), source_fee as u64);
        assert_eq!(
            env.primary_market_state().1.source_backing_buckets[DOMAIN].utilization_fee_earnings,
            0
        );
        census(&env, supply);
        checkpoint(&mut env);
    }
    eprintln!("INV-014 base-fee-induced backing consent: worlds=2, exact_rollbacks=2, source_fee={source_fee}, base_fee_per_owner={base_fee}, peak_CU={peak_cu}");
}
