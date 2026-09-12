//! INV-045/031/036: paid CPI discovery consumes an existing fee-bearing source lien.
//! Retained owner consent survives catch-up, lien release, and provider earnings settlement.

use super::*;
use crate::support::fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census};
use percolator::{BOUND_SCALE, CREDIT_RATE_SCALE};
use percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::Signer};

const DOMAIN: usize = 1;
const PROVIDER: usize = 4;
const RATE: u16 = 3_333;
const INITIAL: u64 = 100;
const FAVORABLE: u64 = 105;
const OPEN_LOTS: i128 = 1_000;
const TOTAL_LOTS: i128 = 1_050;
const CAP_BPS: u64 = 500;
const DEPOSITS: [u128; 5] = [52_502, 2_000_000, 2_000_000, 2_000_000, 0];

fn frame(env: &V16Svm) -> Vec<(Pubkey, Option<Account>)> {
    let mut keys = vec![
        env.market,
        env.foreign_market,
        env.foreign_actor.portfolio,
        env.backing_domain_ledger,
        env.mint,
        solana_sdk::sysvar::clock::ID,
    ];
    keys.extend(env.all_token_account_data().into_iter().map(|(key, _)| key));
    for actor in &env.actors {
        keys.extend([
            actor.signer.pubkey(),
            actor.portfolio,
            actor.matcher_context,
            actor.matcher_delegate,
        ]);
    }
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn settle(env: &mut V16Svm, oracle: Option<Pubkey>) {
    for actor in [1, 0, 3, 2] {
        let mut complete = false;
        for _ in 0..8 {
            let hints = vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: u8::from(oracle.is_some()),
            }];
            let progress = if let Some(oracle) = oracle {
                env.crank_with_oracles_if_actionable(actor, env.current_slot(), hints, &[oracle])
            } else {
                env.crank_if_actionable(actor, env.current_slot(), hints)
            }
            .expect("bounded public source catch-up");
            if progress.is_none() {
                complete = true;
                break;
            }
        }
        assert!(complete, "source catch-up must reach a fixed point");
    }
    assert_public_stock_census("paid mark source lien", env).unwrap();
    assert_public_encumbrance_census("paid mark source lien", env).unwrap();
}

fn owner_source(env: &V16Svm) -> percolator::PortfolioSourceDomainV16Account {
    env.primary_portfolio(0)
        .source_domains
        .into_iter()
        .find(|source| source.is_occupied() && source.domain.get() as usize == DOMAIN)
        .expect("owner has a real source claim")
}

fn pay_provider(env: &mut V16Svm, payout: solana_sdk::transaction::Transaction, fee: u128) {
    let portfolios = [0, 1, 2, 3].map(|actor| env.primary_portfolio_data(actor));
    let insurance = env.primary_market_state().1.insurance;
    let destination = env.actors[PROVIDER].destination_token;
    let tokens = env.token_amount(destination);
    let vault = env.token_amount(env.vault);
    env.land_retained(payout).unwrap();
    assert_eq!(env.token_amount(destination) - tokens, fee as u64);
    assert_eq!(vault - env.token_amount(env.vault), fee as u64);
    assert_eq!(env.primary_market_state().1.insurance, insurance);
    assert_eq!(
        [0, 1, 2, 3].map(|actor| env.primary_portfolio_data(actor)),
        portfolios
    );
    assert_eq!(
        env.primary_market_state().1.source_backing_buckets[DOMAIN].utilization_fee_earnings,
        0
    );
}

#[test]
fn v16_program_paid_cpi_mark_repricing_burns_source_liens_and_preserves_retained_exit() {
    let mut worlds = 0;
    let mut peak_move_cu = 0;
    let mut peak_exit_cu = 0;
    let mut peak_cu = 0;
    let mut canonical = None;
    for hybrid in [false, true] {
        for provider_first in [false, true] {
            let context = format!("hybrid={hybrid}/provider_first={provider_first}");
            let mut env = V16Svm::new(
                [0x7d; 32],
                MarketConfig {
                    initial_price: INITIAL,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: CAP_BPS,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    actor_deposits: DEPOSITS,
                    ..MarketConfig::default()
                },
            );
            // Matcher configuration is public but outside the wrapper-only trace vocabulary.
            env.set_matcher_spreads(1, 0, 0).unwrap();
            env.set_matcher_spreads(3, 9_900, 0).unwrap();
            let supply = env.token_supply_observed();
            let foreign = env.market_data(true);
            env.begin_public_trace();
            env.set_clock(1, 100);
            let feed = [0x7d; 32];
            let oracle = if hybrid {
                let oracle = env.set_pyth_price(&feed, INITIAL as i64, -6, 0, 100);
                env.configure_hybrid_oracle(
                    0,
                    1,
                    100,
                    0,
                    [feed, [0; 32], [0; 32]],
                    &[oracle],
                    1,
                    0,
                )
                .unwrap();
                Some(oracle)
            } else {
                env.configure_ewma_mark(0, 1, INITIAL, 1, 0).unwrap();
                None
            };
            let handoff = env.build_retained_asset_authority_handoff_from_admin(
                0,
                ASSET_AUTH_BACKING_BUCKET,
                PROVIDER,
            );
            env.land_retained(handoff).unwrap();
            env.update_backing_fee_policy(DOMAIN as u16, RATE, 0)
                .unwrap();
            env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN as u16, 100_000, 100)
                .unwrap();
            env.trade_no_cpi(0, 1, 0, OPEN_LOTS * POS_SCALE as i128, INITIAL, 0)
                .unwrap();
            env.set_clock(2, 101);
            let oracle = if hybrid {
                Some(env.set_pyth_price(&feed, FAVORABLE as i64, -6, 0, 101))
            } else {
                env.push_ewma_mark(0, 2, 2 * FAVORABLE - INITIAL).unwrap();
                oracle
            };
            settle(&mut env, oracle);
            let original_profit = OPEN_LOTS * (FAVORABLE - INITIAL) as i128;
            assert_eq!(env.primary_portfolio(0).pnl.get(), original_profit);
            env.set_clock(4, 1_000);
            settle(&mut env, oracle);
            env.trade_no_cpi_with_backing_fee_cap(
                0,
                1,
                0,
                (TOTAL_LOTS - OPEN_LOTS) * POS_SCALE as i128,
                FAVORABLE,
                0,
                RATE,
            )
            .unwrap();
            let lien_source = owner_source(&env);
            let lien = lien_source.source_claim_liened_num.get();
            let claim = original_profit as u128 * BOUND_SCALE;
            let fee = env.primary_market_state().1.source_backing_buckets[DOMAIN]
                .utilization_fee_earnings;
            assert!(lien > 0 && lien < claim, "both free and liened claim stock");
            assert_eq!(lien % BOUND_SCALE, 0);
            assert_eq!(lien_source.source_claim_bound_num.get(), claim);
            assert_eq!(lien_source.source_lien_counterparty_backing_num.get(), lien);
            assert_eq!(lien_source.source_claim_insurance_liened_num.get(), 0);
            assert_eq!(lien_source.source_claim_impaired_num.get(), 0);
            assert_eq!(
                fee,
                (lien / BOUND_SCALE * u128::from(RATE)).div_ceil(10_000)
            );
            assert!(fee > 0);
            assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0] - fee);
            assert_eq!(env.primary_market_state().1.insurance, 0);
            assert_public_stock_census(&context, &env).unwrap();
            assert_public_encumbrance_census(&context, &env).unwrap();

            let size = TOTAL_LOTS * POS_SCALE as i128;
            env.trade_no_cpi(2, 3, 0, 2 * size, FAVORABLE, 0).unwrap();
            env.set_matcher_config_with_trade_fee_cap(1, 1, 0).unwrap();
            env.set_matcher_config_with_trade_fee_cap(3, 1, 10_000)
                .unwrap();
            let strict = env.build_retained_cpi_trade(0, 1, 0, -size, FAVORABLE);
            let permissive = env.build_retained_cpi_trade(0, 1, 0, -size, 1);
            let payout = env.build_retained_backing_bucket_earnings_withdrawal_for_actor(
                PROVIDER,
                DOMAIN as u16,
                fee,
            );
            let retained_bytes = bincode::serialize(&permissive).unwrap();
            for tx in [&strict, &permissive, &payout] {
                tx.verify().unwrap();
                assert!(
                    bincode::serialize(tx).unwrap().len() <= solana_sdk::packet::PACKET_DATA_SIZE
                );
                let before = frame(&env);
                env.svm.simulate_transaction(tx.clone().into()).unwrap();
                assert_eq!(
                    frame(&env),
                    before,
                    "retained requests initially execute read-only"
                );
            }
            let owners = [0, 1, PROVIDER].map(|actor| env.primary_portfolio_data(actor));
            let capital = [2, 3].map(|actor| env.primary_portfolio(actor).capital.get());
            let before = env.primary_market_state().1;
            let old_mark = env.primary_profile(0).mark_ewma_e6;
            assert_eq!(old_mark, FAVORABLE);
            assert_eq!(
                before.source_backing_buckets[DOMAIN].valid_liened_backing_num,
                lien
            );
            env.set_clock(5, 1_001);
            // At mark 105, the authenticated matcher's 99% bid spread quotes one atom.
            let movement = env
                .trade_cpi(2, 3, 0, -size, 0, 1)
                .unwrap_or_else(|error| panic!("{context}: paid CPI reduction: {error}"));
            peak_move_cu = peak_move_cu.max(movement.compute_units);
            let moved = env.primary_profile(0).mark_ewma_e6;
            let after = env.primary_market_state().1;
            let accepted = accepted_mark_reference_clamp(FAVORABLE, 1, CAP_BPS, 1);
            assert!(
                accepted <= moved && moved < FAVORABLE,
                "bounded nonzero adverse movement"
            );
            assert_eq!(after.assets[0].raw_oracle_target_price, moved);
            assert_eq!(after.assets[0].effective_price, FAVORABLE);
            let paid = after.insurance - before.insurance;
            let debits =
                [2, 3].map(|actor| capital[actor - 2] - env.primary_portfolio(actor).capital.get());
            assert!(debits.iter().all(|debit| *debit > 0));
            assert_eq!(debits.iter().sum::<u128>(), paid);
            let required = accepted_mark_required_externality_fee(
                before.assets[0].oi_eff_long_q,
                before.assets[0].oi_eff_short_q,
                FAVORABLE,
                old_mark,
                size.unsigned_abs(),
                accepted,
                moved,
            )
            .unwrap();
            assert!(required > 0 && paid >= required);
            assert_eq!(
                [0, 1, PROVIDER].map(|actor| env.primary_portfolio_data(actor)),
                owners
            );
            assert_eq!(
                after.source_backing_buckets, before.source_backing_buckets,
                "existing backing and earned fees cannot subsidize discovery"
            );
            assert_eq!(after.vault, before.vault);

            env.set_clock(6, 1_002);
            settle(&mut env, oracle);
            assert_eq!(
                env.primary_market_state().1.assets[0].effective_price,
                moved
            );
            let loss = TOTAL_LOTS as u128 * (FAVORABLE - moved) as u128;
            let free_claim = claim - lien;
            assert!(
                loss * BOUND_SCALE > free_claim,
                "repricing must reach the lien"
            );
            assert!(
                loss < original_profit as u128,
                "a positive residual claim remains"
            );
            let residual = original_profit as u128 - loss;
            let remaining_lien = lien - (loss * BOUND_SCALE - free_claim);
            let repriced = owner_source(&env);
            assert!(remaining_lien > 0 && remaining_lien < lien);
            assert_eq!(
                repriced.source_claim_bound_num.get(),
                residual * BOUND_SCALE
            );
            assert_eq!(repriced.source_claim_liened_num.get(), remaining_lien);
            assert_eq!(
                repriced.source_lien_counterparty_backing_num.get(),
                remaining_lien
            );
            assert_eq!(
                repriced.source_lien_effective_reserved.get(),
                remaining_lien / BOUND_SCALE
            );
            assert_eq!(repriced.source_claim_impaired_num.get(), 0);
            let caught_up = env.primary_market_state().1;
            assert_eq!(
                caught_up.source_credit[DOMAIN].valid_liened_backing_num,
                remaining_lien
            );
            assert_eq!(
                caught_up.source_backing_buckets[DOMAIN].valid_liened_backing_num,
                remaining_lien
            );
            assert_eq!(
                caught_up.source_backing_buckets[DOMAIN].utilization_fee_earnings,
                fee
            );
            assert_eq!(caught_up.insurance, paid);
            assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0] - fee);
            assert_eq!(env.primary_portfolio(0).pnl.get(), residual as i128);

            let rollback = frame(&env);
            let expected = solana_sdk::transaction::TransactionError::InstructionError(
                u8::try_from(strict.message.instructions.len() - 1).unwrap(),
                solana_sdk::instruction::InstructionError::Custom(
                    percolator_prog::error::PercolatorError::InvalidInstruction as u32,
                ),
            );
            let error = env
                .land_retained(strict)
                .expect_err("retained strict limit must refuse");
            assert!(
                error.contains(&format!("{expected:?}")),
                "{context}: {error}"
            );
            assert!(
                error.contains(&format!("Program {} success", env.matcher_program)),
                "{context}: {error}"
            );
            assert_eq!(
                frame(&env),
                rollback,
                "strict refusal restores complete economic Accounts"
            );
            if provider_first {
                pay_provider(&mut env, payout.clone(), fee);
            }
            assert_eq!(bincode::serialize(&permissive).unwrap(), retained_bytes);
            let exit = env
                .land_retained(permissive)
                .unwrap_or_else(|error| panic!("{context}: retained owner exit: {error}"));
            peak_exit_cu = peak_exit_cu.max(exit.compute_units);
            if !provider_first {
                pay_provider(&mut env, payout, fee);
            }
            env.trade_no_cpi(2, 3, 0, -size, moved, 0).unwrap();
            settle(&mut env, oracle);
            let flat = env.primary_market_state().1;
            assert_eq!(flat.assets[0].oi_eff_long_q, 0);
            assert_eq!(flat.assets[0].oi_eff_short_q, 0);
            assert_eq!(
                flat.source_backing_buckets[DOMAIN].valid_liened_backing_num,
                0
            );
            assert_eq!(flat.source_credit[DOMAIN].valid_liened_backing_num, 0);
            assert_eq!(owner_source(&env).source_claim_liened_num.get(), 0);
            let values = [0, 1, 2, 3].map(|actor| {
                let account = env.primary_portfolio(actor);
                account.capital.get() as i128 + account.pnl.get()
            });
            assert_eq!(values[0], (DEPOSITS[0] - fee + residual) as i128);
            assert_eq!(values[1], (DEPOSITS[1] - residual) as i128);
            assert_eq!(
                values.iter().sum::<i128>() + paid as i128 + fee as i128,
                DEPOSITS.iter().sum::<u128>() as i128
            );
            // This claimant's released domain is fully backed; no engine conversion helper
            // supplies the expected payout, and no fee earnings enter available support.
            let support = flat.source_credit[DOMAIN];
            assert!(
                support.fresh_reserved_backing_num + support.insurance_credit_reserved_num
                    >= support.positive_claim_bound_num
            );
            assert_eq!(support.credit_rate_num, CREDIT_RATE_SCALE);
            env.convert_released_pnl(0, residual).unwrap();
            let expected_capital = DEPOSITS[0] - fee + residual;
            assert_eq!(env.primary_portfolio(0).capital.get(), expected_capital);
            assert_eq!(env.primary_portfolio(0).pnl.get(), 0);
            assert!(env
                .primary_portfolio(0)
                .source_domains
                .iter()
                .all(|source| !source.is_occupied() || source.source_claim_bound_num.get() == 0));
            let destination = env.actors[0].destination_token;
            let tokens = env.token_amount(destination);
            let vault = env.token_amount(env.vault);
            env.withdraw_primary(0, expected_capital).unwrap();
            assert_eq!(
                env.token_amount(destination) - tokens,
                expected_capital as u64
            );
            assert_eq!(vault - env.token_amount(env.vault), expected_capital as u64);
            assert_eq!(env.primary_portfolio(0).capital.get(), 0);
            let final_group = env.primary_market_state().1;
            assert_eq!(final_group.insurance, paid);
            assert_eq!(
                final_group.source_backing_buckets[DOMAIN].utilization_fee_earnings,
                0
            );
            assert_eq!(final_group.vault, before.vault - fee - expected_capital);
            assert_eq!(env.token_amount(env.vault) as u128, final_group.vault);
            assert_eq!(env.token_supply_observed(), supply);
            assert_eq!(env.market_data(true), foreign);
            assert_public_stock_census(&context, &env).unwrap();
            assert_public_encumbrance_census(&context, &env).unwrap();
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
            let endpoint = (
                moved,
                lien,
                remaining_lien,
                fee,
                paid,
                values,
                expected_capital,
                final_group.vault,
            );
            if let Some(expected) = &canonical {
                assert_eq!(
                    &endpoint, expected,
                    "{context}: mark-mode/settlement-order economics"
                );
            } else {
                canonical = Some(endpoint);
            }
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert!(peak_cu < crate::support::v16_svm::TX_CU_LIMIT);
    println!("paid CPI mark/source lien: worlds={worlds}, strict_refusals={worlds}, owner_withdrawals={worlds}, provider_payouts={worlds}, peak_cu={peak_cu}, peak_move_cu={peak_move_cu}, peak_exit_cu={peak_exit_cu}, endpoint={canonical:?}");
}
