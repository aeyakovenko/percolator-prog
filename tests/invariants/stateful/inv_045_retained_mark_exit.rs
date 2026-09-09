//! INV-045/046/047: retained owner exits across third-party paid mark discovery.
//! Public source claims precede consent; an extreme raw quote then reprices those claims adversely.
//! This is a temporal composition, not a scalar clamp or route-convergence matrix.

use super::*;
use crate::support::fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census};
use num_bigint::BigUint;
use percolator::{BOUND_SCALE, CREDIT_RATE_SCALE};
use solana_sdk::{account::Account, pubkey::Pubkey, signature::Signer, transaction::Transaction};

const INITIAL: u64 = 10_000;
const LOTS: i128 = 10;
const DEPOSIT: u128 = 2_000_000;
const CAP_BPS: u64 = 100;

fn frame(env: &V16Svm) -> Vec<(Pubkey, Option<Account>)> {
    let mut keys = vec![
        env.market,
        env.foreign_market,
        env.foreign_actor.portfolio,
        env.vault,
        env.foreign_vault,
        env.mint,
        env.backing_domain_ledger,
        env.provider_source_token,
        env.provider_destination_token,
    ];
    keys.extend(env.all_token_account_data().into_iter().map(|(key, _)| key));
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
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn settle(env: &mut V16Svm, oracle: Option<Pubkey>, peak_cu: &mut u64) {
    for actor in [1, 0, 3, 2] {
        let mut complete = false;
        for _ in 0..8 {
            let hints = vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: u8::from(oracle.is_some()),
            }];
            let result = if let Some(oracle) = oracle {
                env.crank_with_oracles_if_actionable(actor, env.current_slot(), hints, &[oracle])
            } else {
                env.crank_if_actionable(actor, env.current_slot(), hints)
            }
            .expect("bounded public mark/source settlement");
            match result {
                Some(success) => *peak_cu = (*peak_cu).max(success.compute_units),
                None => {
                    complete = true;
                    break;
                }
            }
        }
        assert!(complete, "source settlement must reach its fixed point");
    }
    assert_public_stock_census("retained mark exit settlement", env).unwrap();
    assert_public_encumbrance_census("retained mark exit settlement", env).unwrap();
}

fn claim(env: &V16Svm, actor: usize) -> u128 {
    env.primary_portfolio(actor)
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .map(|source| source.source_claim_bound_num.get())
        .sum()
}

fn released_support(env: &V16Svm, actor: usize) -> u128 {
    let group = env.primary_market_state().1;
    env.primary_portfolio(actor)
        .source_domains
        .iter()
        .filter(|source| source.is_occupied() && source.source_claim_bound_num.get() != 0)
        .map(|source| {
            assert_eq!(source.source_claim_liened_num.get(), 0);
            assert_eq!(source.source_claim_impaired_num.get(), 0);
            let credit = group.source_credit[source.domain.get() as usize];
            assert_eq!(credit.valid_liened_backing_num, 0);
            assert_eq!(credit.valid_liened_insurance_num, 0);
            assert_eq!(credit.impaired_liened_insurance_num, 0);
            let available =
                credit.fresh_reserved_backing_num + credit.insurance_credit_reserved_num;
            // Reconstruct the bounded domain rate from backing and total claims, without the
            // engine's rate or conversion helpers. Domains round independently to whole atoms.
            let scale = BigUint::from(CREDIT_RATE_SCALE);
            let rate = (BigUint::from(available) * &scale
                / BigUint::from(credit.positive_claim_bound_num))
            .min(scale.clone());
            assert_eq!(BigUint::from(credit.credit_rate_num), rate);
            let support = BigUint::from(source.source_claim_bound_num.get()) * rate
                / scale
                / BigUint::from(BOUND_SCALE);
            u128::try_from(support)
                .unwrap()
                .min(available / BOUND_SCALE)
        })
        .sum()
}

fn retain_exit(env: &mut V16Svm, batch: bool, size: i128, limit: u64) -> Transaction {
    if batch {
        env.build_retained_batch_cpi_trade(0, 1, 0, size, limit)
    } else {
        env.build_retained_cpi_trade(0, 1, 0, size, limit)
    }
}

#[test]
fn v16_program_retained_exits_survive_extreme_paid_mark_source_repricing() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    let mut peak_move_cu = 0;
    let mut peak_exit_cu = 0;
    let mut discounted_conversions = 0;
    for hybrid in [false, true] {
        for owner_long in [false, true] {
            for mover_batch in [false, true] {
                for exit_batch in [false, true] {
                    let context = format!(
                        "hybrid={hybrid}/long={owner_long}/mover_batch={mover_batch}/exit_batch={exit_batch}"
                    );
                    let direction = if owner_long { 1 } else { -1 };
                    let size = direction * LOTS * POS_SCALE as i128;
                    let favorable_target = if owner_long { 10_100 } else { 9_900 };
                    let raw = if owner_long {
                        1
                    } else {
                        percolator::MAX_ORACLE_PRICE
                    };
                    let mut env = V16Svm::new(
                        [0x4e; 32],
                        MarketConfig {
                            initial_price: INITIAL,
                            initial_margin_bps: 1_000,
                            maintenance_margin_bps: 500,
                            max_price_move_bps_per_slot: CAP_BPS,
                            max_accrual_dt_slots: 1,
                            min_funding_lifetime_slots: 1,
                            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, DEPOSIT, 0],
                            ..MarketConfig::default()
                        },
                    );
                    let supply = env.token_supply_observed();
                    let foreign = env.market_data(true);
                    let feed = [0x4e; 32];
                    let oracle = if hybrid {
                        env.set_clock(1, 100);
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
                    env.trade_no_cpi(0, 1, 0, size, INITIAL, 0).unwrap();
                    env.set_clock(2, 101);
                    let oracle = if oracle.is_some() {
                        Some(env.set_pyth_price(&feed, favorable_target as i64, -6, 0, 101))
                    } else {
                        env.push_ewma_mark(0, 2, favorable_target).unwrap();
                        None
                    };
                    settle(&mut env, oracle, &mut peak_cu);
                    if !hybrid {
                        env.set_clock(3, 102);
                        env.push_ewma_mark(0, 3, favorable_target).unwrap();
                        settle(&mut env, oracle, &mut peak_cu);
                    }
                    let favorable = env.primary_market_state().1.assets[0].effective_price;
                    let initial_profit = direction * LOTS * (favorable as i128 - INITIAL as i128);
                    assert!(initial_profit > 0);
                    assert_eq!(env.primary_portfolio(0).pnl.get(), initial_profit);
                    assert_eq!(claim(&env, 0), initial_profit as u128 * BOUND_SCALE);
                    assert_eq!(
                        env.primary_portfolio(1).capital.get(),
                        DEPOSIT - initial_profit as u128
                    );
                    if hybrid {
                        env.set_clock(4, 1_000);
                        settle(&mut env, oracle, &mut peak_cu);
                    }
                    env.trade_no_cpi(2, 3, 0, 2 * size, favorable, 0).unwrap();
                    env.set_matcher_config_with_trade_fee_cap(1, 1, 0).unwrap();
                    env.set_matcher_spreads(1, 0, 0).unwrap();
                    let strict = retain_exit(&mut env, exit_batch, -size, favorable);
                    let permissive = retain_exit(&mut env, exit_batch, -size, raw);
                    let retained_bytes = bincode::serialize(&permissive).unwrap();
                    for tx in [&strict, &permissive] {
                        tx.verify().unwrap();
                        assert!(
                            bincode::serialize(tx).unwrap().len()
                                <= solana_sdk::packet::PACKET_DATA_SIZE
                        );
                        let before = frame(&env);
                        env.svm
                            .simulate_transaction(tx.clone().into())
                            .unwrap_or_else(|error| {
                                panic!("{context}: retained exit must initially execute: {error:?}")
                            });
                        assert_eq!(frame(&env), before, "simulation is read-only");
                    }
                    let owner_before = [0, 1].map(|actor| env.primary_portfolio_data(actor));
                    let mover_capital_before =
                        [2, 3].map(|actor| env.primary_portfolio(actor).capital.get());
                    let before_group = env.primary_market_state().1;
                    let before_mark = env.primary_profile(0).mark_ewma_e6;
                    let landing = env.current_slot() + 1;
                    env.set_clock(landing, 1_000 + landing as i64);
                    let movement = if mover_batch {
                        env.batch_trade_no_cpi(
                            2,
                            3,
                            vec![BatchTradeLeg {
                                asset_index: 0,
                                market_id: before_group.assets[0].market_id,
                                size_q: -size,
                                exec_price: raw,
                                fee_bps: 0,
                            }],
                        )
                    } else {
                        env.trade_no_cpi(2, 3, 0, -size, raw, 0)
                    }
                    .unwrap_or_else(|error| panic!("{context}: paid extreme reduction: {error}"));
                    peak_move_cu = peak_move_cu.max(movement.compute_units);
                    let moved = env.primary_profile(0).mark_ewma_e6;
                    let moved_group = env.primary_market_state().1;
                    let accepted = accepted_mark_reference_clamp(favorable, raw, CAP_BPS, 1);
                    assert!(moved >= favorable.min(accepted) && moved <= favorable.max(accepted));
                    assert!(
                        direction * (moved as i128 - before_mark as i128) < 0,
                        "{context}: nonzero adverse discovery"
                    );
                    assert_eq!(moved_group.assets[0].raw_oracle_target_price, moved);
                    assert_eq!(moved_group.assets[0].effective_price, favorable);
                    let paid = moved_group.insurance - before_group.insurance;
                    let mover_debits = [2, 3].map(|actor| {
                        mover_capital_before[actor - 2] - env.primary_portfolio(actor).capital.get()
                    });
                    assert!(mover_debits.iter().all(|debit| *debit > 0));
                    assert_eq!(mover_debits.iter().sum::<u128>(), paid);
                    let required = accepted_mark_required_externality_fee(
                        before_group.assets[0].oi_eff_long_q,
                        before_group.assets[0].oi_eff_short_q,
                        before_group.assets[0].effective_price,
                        before_mark,
                        size.unsigned_abs(),
                        accepted,
                        moved,
                    )
                    .unwrap();
                    assert!(
                        required > 0 && paid >= required,
                        "{context}: movement is paid"
                    );
                    assert_eq!(
                        [0, 1].map(|actor| env.primary_portfolio_data(actor)),
                        owner_before,
                        "third-party discovery cannot rewrite retained owners"
                    );
                    assert_eq!(moved_group.vault, before_group.vault);

                    env.set_clock(landing + 1, 1_001 + landing as i64);
                    settle(&mut env, oracle, &mut peak_cu);
                    assert_eq!(
                        env.primary_market_state().1.assets[0].effective_price,
                        moved
                    );
                    let owner_profit = direction * LOTS * (moved as i128 - INITIAL as i128);
                    assert!(
                        owner_profit > 0 && owner_profit < initial_profit,
                        "{context}: adverse move consumes part of a real source claim: {initial_profit}->{owner_profit}, {favorable}->{moved}"
                    );
                    assert_eq!(env.primary_portfolio(0).pnl.get(), owner_profit);
                    assert_eq!(claim(&env, 0), owner_profit as u128 * BOUND_SCALE);
                    assert_eq!(
                        claim(&env, 1),
                        (initial_profit - owner_profit) as u128 * BOUND_SCALE
                    );
                    assert_eq!(
                        env.primary_portfolio(1).capital.get(),
                        DEPOSIT - initial_profit as u128
                    );

                    let before_refusal = frame(&env);
                    let expected = solana_sdk::transaction::TransactionError::InstructionError(
                        u8::try_from(strict.message.instructions.len() - 1).unwrap(),
                        solana_sdk::instruction::InstructionError::Custom(
                            percolator_prog::error::PercolatorError::InvalidInstruction as u32,
                        ),
                    );
                    let error = env
                        .land_retained(strict)
                        .expect_err("retained strict price must refuse adverse repricing");
                    assert!(
                        error.contains(&format!("{expected:?}")),
                        "{context}: {error}"
                    );
                    assert!(error.contains(&format!("Program {} success", env.matcher_program)),
                        "{context}: price refusal must follow successful matcher execution: {error}");
                    assert_eq!(
                        frame(&env),
                        before_refusal,
                        "strict price refusal rolls back every tracked account"
                    );
                    assert_eq!(bincode::serialize(&permissive).unwrap(), retained_bytes);
                    let exit = env.land_retained(permissive).unwrap_or_else(|error| {
                        panic!("{context}: retained permissive exit: {error}")
                    });
                    peak_exit_cu = peak_exit_cu.max(exit.compute_units);
                    assert_eq!(env.primary_profile(0).mark_ewma_e6, moved);
                    assert_eq!(
                        env.primary_market_state().1.insurance,
                        moved_group.insurance
                    );
                    let mover_exit = env.trade_no_cpi(2, 3, 0, -size, moved, 0).unwrap();
                    peak_cu = peak_cu.max(mover_exit.compute_units);
                    settle(&mut env, oracle, &mut peak_cu);

                    let owner_values = [
                        DEPOSIT as i128 + owner_profit,
                        DEPOSIT as i128 - owner_profit,
                    ];
                    let all_values: [i128; 4] = std::array::from_fn(|actor| {
                        let account = env.primary_portfolio(actor);
                        account.capital.get() as i128 + account.pnl.get()
                    });
                    assert_eq!(&all_values[..2], &owner_values);
                    assert_eq!(
                        all_values.iter().sum::<i128>() + moved_group.insurance as i128,
                        4 * DEPOSIT as i128
                    );
                    let vault_before_payouts = env.token_amount(env.vault);
                    let mut payouts = 0u128;
                    for actor in [1, 0, 3, 2] {
                        settle(&mut env, oracle, &mut peak_cu);
                        let before_conversion = env.primary_portfolio(actor);
                        let released = env.primary_portfolio(actor).pnl.get().max(0) as u128;
                        let support = released_support(&env, actor);
                        assert!(support <= released);
                        discounted_conversions += usize::from(support < released);
                        if released != 0 {
                            let conversion = env.convert_released_pnl(actor, released).unwrap();
                            peak_cu = peak_cu.max(conversion.compute_units);
                        }
                        let capital = env.primary_portfolio(actor).capital.get();
                        assert_eq!(capital, before_conversion.capital.get() + support);
                        assert_eq!(claim(&env, actor), 0);
                        let destination = env.actors[actor].destination_token;
                        let before = env.token_amount(destination);
                        let withdrawal = env.withdraw_primary(actor, capital).unwrap();
                        peak_cu = peak_cu.max(withdrawal.compute_units);
                        assert_eq!(env.token_amount(destination) - before, capital as u64);
                        assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
                        assert_eq!(env.primary_portfolio(actor).pnl.get(), 0);
                        payouts += capital;
                    }
                    let final_group = env.primary_market_state().1;
                    assert_eq!(final_group.assets[0].oi_eff_long_q, 0);
                    assert_eq!(final_group.assets[0].oi_eff_short_q, 0);
                    assert_eq!(final_group.source_claim_bound_total_num, 0);
                    assert_eq!(final_group.c_tot, 0);
                    assert_eq!(final_group.insurance, moved_group.insurance);
                    assert_eq!(
                        final_group.vault,
                        u128::from(vault_before_payouts) - payouts
                    );
                    assert_eq!(env.token_amount(env.vault) as u128, final_group.vault);
                    assert_eq!(env.token_supply_observed(), supply);
                    assert_eq!(env.market_data(true), foreign);
                    assert_public_stock_census(&context, &env).unwrap();
                    assert_public_encumbrance_census(&context, &env).unwrap();
                    worlds += 1;
                }
            }
        }
    }
    peak_cu = peak_cu.max(peak_move_cu).max(peak_exit_cu);
    assert_eq!(worlds, 16);
    assert!(
        discounted_conversions > 0,
        "source-limited conversion is nonvacuous"
    );
    assert!(peak_cu < crate::support::v16_svm::TX_CU_LIMIT);
    println!("retained paid-mark exits: worlds={worlds}, strict_refusals={worlds}, withdrawals={}, discounted_conversions={discounted_conversions}, peak_cu={peak_cu}, peak_move_cu={peak_move_cu}, peak_exit_cu={peak_exit_cu}", worlds * 4);
}
