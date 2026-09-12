//! INV-045: batching must preserve each asset's elapsed discovery capacity.
//! Unlike the uniform-age maximum-shape case, one mark predates the accrual horizon
//! while the other is younger. Setup and reductions use only public instructions.
//! The mixed-capacity suffix also puts an already-updated asset and an unspent
//! asset in one batch, with opposed reports and a batch-first economic control.

use super::*;

const MARKS: [u64; 2] = [1_000_000, 2_000_000];
const MARK_SLOTS: [u64; 2] = [1, 4];
const LANDING_SLOT: u64 = 6;
const CAP_BPS: u64 = 100;
const MAX_DT: u64 = 3;
const DEPOSIT: u128 = 50_000_000;
const OPEN_Q: i128 = 4 * POS_SCALE as i128;

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let account = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|index| state::read_asset_oracle_profile(&account.data, index).unwrap())
}

fn batch(
    env: &mut V16CuEnv,
    owners: [&Keypair; 2],
    portfolios: [Pubkey; 2],
    size_q: i128,
    prices: [u64; 2],
) -> u64 {
    let legs = (0..2)
        .map(|index| BatchTradeLeg {
            asset_index: index as u16,
            market_id: env.asset_market_id(index as u16),
            size_q,
            exec_price: prices[index],
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_no_cpi_ix(portfolios[0], portfolios[1], legs),
        vec![
            AccountMeta::new(owners[0].pubkey(), true),
            AccountMeta::new(owners[1].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[0], false),
            AccountMeta::new(portfolios[1], false),
        ],
        &owners,
    )
    .expect("public two-asset batch")
}

#[test]
fn v16_program_batch_and_single_marks_preserve_staggered_elapsed_envelopes() {
    let raw_prices = [percolator::MAX_ORACLE_PRICE, 1];
    let elapsed = MARK_SLOTS.map(|slot| LANDING_SLOT - slot);
    assert_eq!(elapsed, [5, 2]);
    assert!(elapsed[0] > MAX_DT && elapsed[1] < MAX_DT);

    // Independent bounded integer oracle, not the wrapper's clamp/EWMA/fee helpers.
    let accepted_prices = [0, 1].map(|index| {
        let delta = MARKS[index] * CAP_BPS * elapsed[index].min(MAX_DT) / 10_000;
        raw_prices[index].clamp(MARKS[index] - delta, MARKS[index] + delta)
    });
    assert_eq!(accepted_prices, [1_030_000, 1_960_000]);
    let expected_marks = [0, 1].map(|index| {
        let alpha_bps = 10_000 * elapsed[index] / (elapsed[index] + 1);
        let delta = MARKS[index].abs_diff(accepted_prices[index]) * alpha_bps / 10_000;
        if accepted_prices[index] > MARKS[index] {
            MARKS[index] + delta
        } else {
            MARKS[index] - delta
        }
    });
    assert_eq!(expected_marks, [1_024_999, 1_973_336]);
    let expected_fees = [0, 1].map(|index| {
        let move_bps = (u128::from(MARKS[index].abs_diff(expected_marks[index])) * 10_000)
            .div_ceil(u128::from(MARKS[index]));
        let externality_notional = 2 * OPEN_Q.unsigned_abs() * u128::from(MARKS[index]) / POS_SCALE;
        let required = (externality_notional * move_bps).div_ceil(10_000);
        let notional = u128::from(accepted_prices[index]); // reduction is exactly POS_SCALE
        (0..=10_000u128)
            .map(|bps| 2 * (notional * bps).div_ceil(10_000))
            .find(|paid| *paid >= required)
            .expect("funded movement fits configured fee ceiling")
    });
    let mut outcomes = Vec::new();

    for route in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: MARKS[0],
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..V16CuMarketParams::default()
        });
        for index in 0..2 {
            env.svm.warp_to_slot(MARK_SLOTS[index]);
            configure_max_shape_ewma_asset(&mut env, index as u16, MARK_SLOTS[index], MARKS[index]);
        }
        let owners = [Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        for index in 0..2 {
            env.deposit(&owners[index], portfolios[index], DEPOSIT);
        }
        batch(&mut env, owners.each_ref(), portfolios, OPEN_Q, MARKS);
        for portfolio in portfolios {
            env.svm.expire_blockhash();
            env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: MARK_SLOTS[1],
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            );
        }
        let before = env.market_state().1;
        assert_eq!(before.insurance, 0);
        assert_eq!(profiles(&env).map(|profile| profile.mark_ewma_e6), MARKS);
        assert_eq!(
            profiles(&env).map(|profile| profile.mark_ewma_last_slot),
            MARK_SLOTS
        );
        assert_eq!([0, 1].map(|index| before.assets[index].slot_last), [4, 4]);
        assert_eq!(before.config.max_price_move_bps_per_slot, CAP_BPS);
        assert_eq!(before.config.max_accrual_dt_slots, MAX_DT);

        env.svm.warp_to_slot(LANDING_SLOT);
        let mut cus = Vec::new();
        match route {
            NoCpiReportedPricePath::Single => {
                for index in 0..2 {
                    let untouched = profiles(&env)[1 - index];
                    let insurance_before = env.market_state().1.insurance;
                    env.svm.expire_blockhash();
                    cus.push(env.trade_asset_with_cu(
                        index as u16,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        -(POS_SCALE as i128),
                        raw_prices[index],
                        0,
                    ));
                    assert_eq!(
                        env.market_state().1.insurance - insurance_before,
                        expected_fees[index],
                        "{route:?} asset {index}: exact collected movement fee"
                    );
                    let after = profiles(&env);
                    assert_eq!(after[index].mark_ewma_e6, expected_marks[index]);
                    assert_eq!(after[1 - index].mark_ewma_e6, untouched.mark_ewma_e6);
                    assert_eq!(
                        after[1 - index].mark_ewma_last_slot,
                        untouched.mark_ewma_last_slot
                    );
                }
            }
            NoCpiReportedPricePath::Batch => cus.push(batch(
                &mut env,
                owners.each_ref(),
                portfolios,
                -(POS_SCALE as i128),
                raw_prices,
            )),
        }
        let after = env.market_state().1;
        let after_profiles = profiles(&env);
        for index in 0..2 {
            let mark = after_profiles[index].mark_ewma_e6;
            let max_delta = MARKS[index] * CAP_BPS * elapsed[index].min(MAX_DT) / 10_000;
            assert!(
                mark.abs_diff(MARKS[index]) > 0,
                "{route:?} asset {index}: real movement"
            );
            assert!(
                mark.abs_diff(MARKS[index]) <= max_delta,
                "{route:?} asset {index}: movement exceeds its own elapsed-time envelope"
            );
            assert_eq!(
                mark, expected_marks[index],
                "{route:?} asset {index}: exact bounded EWMA"
            );
            assert_eq!(after_profiles[index].mark_ewma_last_slot, LANDING_SLOT);
            assert_eq!(after_profiles[index].oracle_target_price_e6, mark);
            assert_eq!(after.assets[index].raw_oracle_target_price, mark);
            assert_eq!(after.assets[index].oi_eff_long_q, 3 * POS_SCALE);
            assert_eq!(after.assets[index].oi_eff_short_q, 3 * POS_SCALE);
        }
        assert_eq!(after.insurance, expected_fees.iter().sum::<u128>());
        assert_eq!(after.vault, 2 * DEPOSIT);
        assert_eq!(u128::from(env.token_amount(env.vault)), after.vault);
        assert_eq!(after.insurance_domain_budget.iter().sum::<u128>(), 0);
        outcomes.push((
            after_profiles.map(|profile| (profile.mark_ewma_e6, profile.mark_ewma_last_slot)),
            [0, 1].map(|index| after.assets[index].raw_oracle_target_price),
            after.insurance,
            after.vault,
        ));
        eprintln!(
            "INV-045 staggered elapsed envelopes {route:?}: CU={cus:?}, fees={expected_fees:?}"
        );
        for cu in cus {
            assert_cu_within(
                "staggered two-asset reduction",
                cu,
                MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
            );
        }
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "batch and singles preserve per-asset paid marks"
    );
}

fn mixed_capacity_pair() -> (V16CuEnv, [Keypair; 2], [Pubkey; 2], [Pubkey; 2]) {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: MARKS[0],
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..V16CuMarketParams::default()
        },
    );
    for asset in 0..2 {
        env.svm.warp_to_slot(MARK_SLOTS[asset]);
        configure_max_shape_ewma_asset(&mut env, asset as u16, MARK_SLOTS[asset], MARKS[asset]);
    }
    let owners: [Keypair; 2] = std::array::from_fn(|_| Keypair::new());
    let mut portfolios = [Pubkey::default(); 2];
    let mut wallets = [Pubkey::default(); 2];
    for actor in 0..2 {
        env.svm
            .airdrop(&owners[actor].pubkey(), 1_000_000_000)
            .unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        portfolios[actor] = portfolio.pubkey();
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owners[actor]],
        )
        .expect("public portfolio initialization");
        env.portfolios.push(portfolio.pubkey());
        wallets[actor] =
            create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &wallets[actor],
                &env.admin.pubkey(),
                &[],
                DEPOSIT as u64,
            )
            .unwrap(),
            &[&env.admin],
        )
        .expect("public SPL funding");
        env.send(
            env.deposit_ix(portfolios[actor], DEPOSIT),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(wallets[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[actor]],
        )
        .expect("public deposit");
    }
    batch(&mut env, owners.each_ref(), portfolios, OPEN_Q, MARKS);
    for portfolio in portfolios {
        env.svm.expire_blockhash();
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: MARK_SLOTS[1],
                observations: crank_observations_for_assets(&[0, 1]),
            },
        );
    }
    assert_eq!(profiles(&env).map(|p| p.mark_ewma_e6), MARKS);
    assert_eq!(profiles(&env).map(|p| p.mark_ewma_last_slot), MARK_SLOTS);
    assert_eq!(
        [0, 1].map(|i| env.market_state().1.assets[i].slot_last),
        [4, 4]
    );
    assert_eq!(env.market_state().1.insurance, 0);
    env.svm.warp_to_slot(LANDING_SLOT);
    (env, owners, portfolios, wallets)
}

#[test]
fn v16_program_mixed_spent_unspent_batch_capacity_is_asset_local() {
    let mut peak_cu = [0; 2];
    let mut worlds = 0;
    let mut prefixes = 0;
    let mut mixed_batches = 0;
    for rises in [false, true] {
        let reports = [0, 1].map(|i| {
            if (i == 0) == rises {
                percolator::MAX_ORACLE_PRICE
            } else {
                1
            }
        });
        let reversals = [reports[1], reports[0]];
        // Independent input-only oracle. Both first discoveries have four units
        // of pre-trade OI; every later reduction has zero elapsed discovery time.
        let accepted = [0, 1].map(|i| {
            let dt = LANDING_SLOT - MARK_SLOTS[i];
            let cap = MARKS[i] * CAP_BPS * dt.min(MAX_DT) / 10_000;
            reports[i].clamp(MARKS[i] - cap, MARKS[i] + cap)
        });
        let paid_marks = [0, 1].map(|i| {
            let dt = LANDING_SLOT - MARK_SLOTS[i];
            let alpha = 10_000 * dt / (dt + 1);
            let movement = MARKS[i].abs_diff(accepted[i]) * alpha / 10_000;
            if accepted[i] > MARKS[i] {
                MARKS[i] + movement
            } else {
                MARKS[i] - movement
            }
        });
        let fees = [0, 1].map(|i| {
            let bps = (u128::from(MARKS[i].abs_diff(paid_marks[i])) * 10_000)
                .div_ceil(u128::from(MARKS[i]));
            let externality = 2 * OPEN_Q.unsigned_abs() * u128::from(MARKS[i]) / POS_SCALE;
            let required = (externality * bps).div_ceil(10_000);
            (0..=10_000u128)
                .map(|bps| 2 * (u128::from(accepted[i]) * bps).div_ceil(10_000))
                .find(|paid| *paid >= required)
                .expect("fully collectible discovery fee")
        });
        assert!(fees.iter().all(|fee| *fee > 0));
        assert!(paid_marks.iter().zip(MARKS).all(|(mark, old)| *mark != old));
        let mut outcomes = Vec::new();
        for first in 0..2 {
            for reverse_batch in [false, true] {
                for batch_first in [false, true] {
                    let (mut env, owners, portfolios, wallets) = mixed_capacity_pair();
                    let batch_order = if reverse_batch {
                        vec![1, 0]
                    } else {
                        vec![0, 1]
                    };
                    let word = if batch_first {
                        vec![
                            batch_order.clone(),
                            vec![first],
                            vec![1 - first],
                            batch_order,
                        ]
                    } else {
                        vec![
                            vec![first],
                            batch_order.clone(),
                            vec![1 - first],
                            batch_order,
                        ]
                    };
                    let custody_keys = [
                        env.mint,
                        env.vault,
                        wallets[0],
                        wallets[1],
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                    ];
                    let custody = custody_keys.map(|key| env.svm.get_account(&key));
                    let mut moved = [false; 2];
                    let mut remaining = [OPEN_Q.unsigned_abs(); 2];
                    let mut total_fee = 0;
                    for order in word {
                        let raw = [0, 1].map(|i| if moved[i] { reversals[i] } else { reports[i] });
                        let mixed = order.len() == 2 && moved[0] != moved[1];
                        let before_profiles = profiles(&env);
                        let before_fee = total_fee;
                        let expected_fee: u128 =
                            order.iter().filter(|&&i| !moved[i]).map(|&i| fees[i]).sum();
                        env.svm.expire_blockhash();
                        let cu = if order.len() == 1 {
                            let i = order[0];
                            env.trade_asset_with_cu(
                                i as u16,
                                &owners[0],
                                portfolios[0],
                                &owners[1],
                                portfolios[1],
                                -(POS_SCALE as i128),
                                raw[i],
                                0,
                            )
                        } else {
                            let legs = order
                                .iter()
                                .map(|&i| BatchTradeLeg {
                                    asset_index: i as u16,
                                    market_id: env.asset_market_id(i as u16),
                                    size_q: -(POS_SCALE as i128),
                                    exec_price: raw[i],
                                    fee_bps: 0,
                                })
                                .collect();
                            env.send(
                                env.batch_trade_no_cpi_ix(portfolios[0], portfolios[1], legs),
                                vec![
                                    AccountMeta::new(owners[0].pubkey(), true),
                                    AccountMeta::new(owners[1].pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolios[0], false),
                                    AccountMeta::new(portfolios[1], false),
                                ],
                                &owners.each_ref(),
                            )
                            .expect("public mixed-capacity batch")
                        };
                        peak_cu[order.len() - 1] = peak_cu[order.len() - 1].max(cu);
                        for &i in &order {
                            moved[i] = true;
                            remaining[i] -= POS_SCALE;
                        }
                        total_fee += expected_fee;
                        let p = profiles(&env);
                        let group = env.market_state().1;
                        for i in 0..2 {
                            assert_eq!(
                                p[i].mark_ewma_e6,
                                if moved[i] { paid_marks[i] } else { MARKS[i] }
                            );
                            assert_eq!(
                                p[i].mark_ewma_last_slot,
                                if moved[i] {
                                    LANDING_SLOT
                                } else {
                                    MARK_SLOTS[i]
                                }
                            );
                            assert_eq!(p[i].oracle_target_price_e6, p[i].mark_ewma_e6);
                            assert_eq!(group.assets[i].raw_oracle_target_price, p[i].mark_ewma_e6);
                            assert_eq!(group.assets[i].effective_price, MARKS[i]);
                            assert_eq!(group.assets[i].oi_eff_long_q, remaining[i]);
                            assert_eq!(group.assets[i].oi_eff_short_q, remaining[i]);
                            if !order.contains(&i)
                                || before_profiles[i].mark_ewma_last_slot == LANDING_SLOT
                            {
                                assert_eq!(
                                    p[i].mark_ewma_e6, before_profiles[i].mark_ewma_e6,
                                    "unrelated or spent asset must not move"
                                );
                                assert_eq!(
                                    p[i].mark_ewma_last_slot,
                                    before_profiles[i].mark_ewma_last_slot
                                );
                            }
                        }
                        assert_eq!(group.insurance - before_fee, expected_fee);
                        assert_eq!(group.insurance, total_fee);
                        for (actor, portfolio) in portfolios.into_iter().enumerate() {
                            let account = env.portfolio_state(portfolio);
                            assert_eq!(account.capital.get(), DEPOSIT - total_fee / 2);
                            assert_eq!(account.pnl.get(), 0);
                            for i in 0..2 {
                                let position = active_leg_for_asset(&account, i).basis_pos_q;
                                assert_eq!(
                                    position,
                                    if actor == 0 {
                                        remaining[i] as i128
                                    } else {
                                        -(remaining[i] as i128)
                                    }
                                );
                            }
                        }
                        assert_eq!(group.c_tot + total_fee, 2 * DEPOSIT);
                        assert_eq!(group.vault, 2 * DEPOSIT);
                        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                        assert_eq!(group.insurance_domain_budget.iter().sum::<u128>(), 0);
                        assert_eq!(custody_keys.map(|key| env.svm.get_account(&key)), custody);
                        prefixes += 1;
                        mixed_batches += usize::from(mixed);
                    }
                    assert_eq!(remaining, [POS_SCALE; 2]);
                    assert_eq!(total_fee, fees.iter().sum::<u128>());
                    outcomes.push((
                        // Independent public worlds have different authority keys.
                        profiles(&env).map(|p| {
                            (
                                p.mark_ewma_e6,
                                p.mark_ewma_last_slot,
                                p.oracle_target_price_e6,
                                p.price_move_remainder_bps_num,
                            )
                        }),
                        portfolios.map(|p| {
                            let a = env.portfolio_state(p);
                            (a.capital.get(), a.pnl.get())
                        }),
                        env.market_state().1.insurance,
                        env.token_amount(env.vault),
                    ));
                    worlds += 1;
                }
            }
        }
        assert!(
            outcomes.windows(2).all(|w| w[0] == w[1]),
            "mixed-capacity and batch-first schedules converge per asset and owner"
        );
    }
    assert_eq!((worlds, prefixes, mixed_batches), (16, 64, 8));
    for cu in peak_cu {
        assert!(cu > 0);
        assert_cu_within(
            "mixed spent/unspent capacity",
            cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
    }
    eprintln!("INV-045 mixed capacity: worlds={worlds}, prefixes={prefixes}, mixed batches={mixed_batches}, peak single/batch CU={peak_cu:?}");
}
