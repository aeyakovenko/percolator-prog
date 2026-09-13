//! INV-020 / row 426: a paid active maker recertifies through single/batch CPI.
//! Public construction only; the matcher context is System-created and initialized.

use super::*;

fn favorable_trade(
    env: &V16CuEnv,
    owners: &[Keypair; 4],
    portfolios: [Pubkey; 4],
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    assets: &[u16],
    size: i128,
    batch: bool,
) -> Instruction {
    let [_, _, maker, taker] = portfolios;
    let price = |asset| if asset == 2 { KEEPER_PRICE } else { PRICE };
    let mut accounts = vec![AccountMeta::new(owners[3].pubkey(), true)];
    let op = if let Some((program, context, delegate)) = matcher {
        accounts.extend([
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(maker, false),
            AccountMeta::new_readonly(program, false),
            AccountMeta::new(context, false),
            AccountMeta::new_readonly(delegate, false),
        ]);
        if batch {
            env.batch_trade_cpi_ix(
                taker,
                maker,
                assets
                    .iter()
                    .map(|&asset_index| BatchTradeCpiLeg {
                        asset_index,
                        market_id: env.asset_market_id(asset_index),
                        size_q: size,
                        fee_bps: 0,
                        limit_price: price(asset_index),
                    })
                    .collect(),
            )
        } else {
            assert_eq!(assets.len(), 1);
            env.trade_cpi_ix(taker, maker, assets[0], size, 0, price(assets[0]))
        }
    } else {
        accounts.extend([
            AccountMeta::new(owners[2].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(maker, false),
        ]);
        if batch {
            env.batch_trade_no_cpi_ix(
                taker,
                maker,
                assets
                    .iter()
                    .map(|&asset_index| BatchTradeLeg {
                        asset_index,
                        market_id: env.asset_market_id(asset_index),
                        size_q: size,
                        exec_price: price(asset_index),
                        fee_bps: 0,
                    })
                    .collect(),
            )
        } else {
            assert_eq!(assets.len(), 1);
            env.trade_no_cpi_ix(taker, maker, assets[0], size, price(assets[0]), 0)
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: op.encode(),
    }
}

#[test]
fn v16_program_cpi_active_keeper_observations_preserve_admission_and_payout() {
    let mut reference = None;
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for cpi in [false, true] {
        for batch in [false, true] {
            for complete_first in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 4,
                        initial_price: PRICE,
                        min_nonzero_mm_req: 599,
                        min_nonzero_im_req: 600,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 10,
                        max_accrual_dt_slots: 64,
                        min_funding_lifetime_slots: 64,
                        max_abs_funding_e9_per_slot: 0,
                        liquidation_fee_bps: 100,
                        liquidation_fee_cap: 10_000,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 0, 100);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let feed = [0xcb; 32];
                let initial = env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100);
                env.try_configure_hybrid_asset_with_conf_filter_cu(
                    0,
                    1,
                    0,
                    [feed, [0; 32], [0; 32]],
                    &[initial],
                    0,
                    100,
                    0,
                    0,
                    100,
                    0,
                )
                .unwrap();
                for asset in [1, 2, 3] {
                    env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
                }
                let owners = std::array::from_fn(|_| Keypair::new());
                let funded =
                    [0, 1, 2, 3].map(|i| funded_owner(&mut env, &owners[i], ENDOWMENTS[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let [peer, target, keeper, keeper_peer] = portfolios;
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
                for asset in [0, 1] {
                    env.trade_asset_with_cu(
                        asset,
                        &owners[0],
                        peer,
                        &owners[1],
                        target,
                        POS_SCALE as i128,
                        PRICE,
                        0,
                    );
                }
                env.trade_asset_with_cu(
                    2,
                    &owners[3],
                    keeper_peer,
                    &owners[2],
                    keeper,
                    POS_SCALE as i128,
                    PRICE,
                    0,
                );
                let matcher = cpi
                    .then(|| auth_matcher_for_lp_via_system_create(&mut env, &owners[2], keeper));
                let mut tracked = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    initial,
                    env.admin.pubkey(),
                    solana_sdk::sysvar::clock::ID,
                ];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                if let Some((program, context, delegate)) = matcher {
                    tracked.extend([program, context, delegate]);
                }
                set_test_clock(&mut env, 0, 101);
                env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
                env.push_auth_mark_for_asset_as_admin(2, u64::MAX, KEEPER_PRICE);
                let report = env.set_pyth_price_with_conf(&feed, CURRENT[0] as i64, -6, 0, 101);
                tracked.push(report);
                let full = if batch { [3, 2, 1, 0] } else { [0, 1, 2, 3] };
                let stage = observation(&env, target, &owners[2], Some(keeper), report, &full);
                peak = peak.max(transact(&mut env, &[&owners[2]], &[stage], &tracked, None));
                let before = frame(&env, &portfolios);
                set_test_clock(&mut env, 64, 102);
                let stage = observation(&env, target, &owners[2], Some(keeper), report, &full);
                peak = peak.max(transact(&mut env, &[&owners[2]], &[stage], &tracked, None));
                assert_eq!(frame(&env, &portfolios), before);
                assert_eq!(
                    [0, 1, 2].map(|i| env.market_state().1.assets[i].slot_last),
                    [32; 3]
                );
                let refresh = observation(&env, target, &owners[2], Some(keeper), report, &[0, 1]);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[refresh],
                    &tracked,
                    None,
                ));
                assert_current_short(&env, target, 130_000, 209_000);
                assert_eq!(env.svm.get_account(&keeper).unwrap(), before[2].1);
                let keeper_leg = active_leg_for_asset(&env.portfolio_state(keeper), 2);
                let liquidate =
                    observation(&env, target, &owners[2], Some(keeper), report, &[0, 1]);
                let admit = favorable_trade(
                    &env,
                    &owners,
                    portfolios,
                    matcher,
                    &[3],
                    POS_SCALE as i128,
                    batch,
                );
                let stale = observation(&env, keeper, &owners[3], None, initial, &[0]);
                let signers = if cpi {
                    vec![&owners[3]]
                } else {
                    vec![&owners[3], &owners[2]]
                };
                // Failure at index 4 requires liquidation and the route's admission to
                // succeed first, including the single-CPI matcher response write.
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2], &owners[3]],
                    &[liquidate.clone(), admit.clone(), stale.clone()],
                    &tracked,
                    Some((4, PercolatorError::OracleStale)),
                ));
                rollbacks += 1;
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[liquidate],
                    &tracked,
                    None,
                ));
                let remaining = env.market_state().1.assets[0].oi_eff_short_q;
                assert!(remaining > 0 && remaining < POS_SCALE);
                let penalty = ((POS_SCALE - remaining) * u128::from(CURRENT[0]))
                    .div_ceil(POS_SCALE)
                    .div_ceil(100)
                    .min(10_000);
                let reward = penalty * SHARE / 10_000;
                assert!(reward > 0);
                assert_eq!(env.portfolio_state(target).capital.get(), 130_000 - penalty);
                assert_eq!(
                    env.portfolio_state(keeper).capital.get(),
                    ENDOWMENTS[2] + reward
                );
                assert!(!health_cert(&env.portfolio_state(keeper)).valid);
                assert_eq!(
                    active_leg_for_asset(&env.portfolio_state(keeper), 2),
                    keeper_leg
                );
                assert_eq!(env.market_state().1.assets[2].slot_last, 32);
                let paid_target = env.svm.get_account(&target);
                census(&env, portfolios, tokens);

                let omitted = observation(&env, keeper, &owners[3], None, report, &[0, 1]);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[3]],
                    &[omitted],
                    &tracked,
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
                rollbacks += 1;
                if complete_first {
                    for account in [peer, keeper, keeper_peer] {
                        let refresh = observation(&env, account, &owners[3], None, report, &full);
                        peak = peak.max(transact(
                            &mut env,
                            &[&owners[3]],
                            &[refresh],
                            &tracked,
                            None,
                        ));
                    }
                }
                let context_before = matcher.map(|(_, context, _)| env.svm.get_account(&context));
                peak = peak.max(transact(&mut env, &signers, &[admit], &tracked, None));
                if let Some((_, context, _)) = matcher {
                    if batch {
                        assert_eq!(
                            Some(env.svm.get_account(&context)),
                            context_before,
                            "batch uses authenticated return data"
                        );
                    } else {
                        assert_ne!(Some(env.svm.get_account(&context)), context_before);
                        let fill = percolator_prog::matcher_abi::read_matcher_return(
                            &env.svm.get_account(&context).unwrap().data,
                        )
                        .unwrap();
                        assert_eq!(
                            (fill.exec_size, fill.exec_price_e6),
                            (POS_SCALE as i128, PRICE)
                        );
                    }
                }
                let observed = if complete_first {
                    KEEPER_PRICE
                } else {
                    1_032_000
                };
                let loss = u128::from(observed - PRICE);
                let lag = u128::from(KEEPER_PRICE - observed);
                for (key, equity) in [
                    (keeper, ENDOWMENTS[2] + reward - loss),
                    (keeper_peer, ENDOWMENTS[3] + loss),
                ] {
                    let account = env.portfolio_state(key);
                    assert!(assert_current_certificate_matches_independent(
                        "CPI recipient admission",
                        &env.market_state().1,
                        &account
                    )
                    .unwrap());
                    assert_eq!(
                        account.capital.get() as i128 + account.pnl.get(),
                        equity as i128
                    );
                    let cert = health_cert(&account);
                    assert_eq!(cert.certified_equity, equity as i128);
                    assert_eq!(
                        cert.certified_initial_req,
                        u128::from(PRICE + observed) / 10 + if key == keeper { lag } else { 0 }
                    );
                    assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
                    assert_eq!(cert.certified_liq_deficit, 0);
                    for asset in [2, 3] {
                        assert_eq!(
                            active_leg_for_asset(&account, asset).basis_pos_q,
                            if key == keeper {
                                -(POS_SCALE as i128)
                            } else {
                                POS_SCALE as i128
                            }
                        );
                    }
                }
                assert_eq!(
                    env.market_state().1.assets[2].slot_last,
                    if complete_first { 64 } else { 32 }
                );
                census(&env, portfolios, tokens);
                if !complete_first {
                    let refresh = observation(&env, peer, &owners[3], None, report, &full);
                    peak = peak.max(transact(
                        &mut env,
                        &[&owners[3]],
                        &[refresh],
                        &tracked,
                        None,
                    ));
                }
                let chunks = if batch {
                    vec![vec![3, 2]]
                } else {
                    vec![vec![3], vec![2]]
                };
                for assets in chunks {
                    let close = favorable_trade(
                        &env,
                        &owners,
                        portfolios,
                        matcher,
                        &assets,
                        -(POS_SCALE as i128),
                        batch,
                    );
                    peak = peak.max(transact(
                        &mut env,
                        &signers,
                        &[close.clone(), stale.clone()],
                        &tracked,
                        Some((3, PercolatorError::OracleStale)),
                    ));
                    rollbacks += 1;
                    peak = peak.max(transact(&mut env, &signers, &[close], &tracked, None));
                    census(&env, portfolios, tokens);
                }
                let expected = ENDOWMENTS[2] + reward - u128::from(KEEPER_PRICE - PRICE);
                assert_eq!(env.portfolio_state(keeper).capital.get(), expected);
                assert_eq!(env.portfolio_state(keeper).pnl.get(), 0);
                assert_eq!(
                    active_bitmap(&env.portfolio_state(keeper)),
                    active_bitmap_with(&[])
                );
                let withdrawal = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[2].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(keeper, expected).encode(),
                };
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[withdrawal],
                    &tracked,
                    None,
                ));
                assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
                assert_eq!(u128::from(env.token_amount(tokens[2])), expected);
                assert_eq!(env.svm.get_account(&target), paid_target);
                let insurance = penalty - reward;
                assert_eq!(env.market_state().1.insurance, insurance);
                assert_eq!(
                    env.market_state().1.insurance_domain_budget,
                    [insurance / 2, insurance - insurance / 2, 0, 0, 0, 0, 0, 0]
                );
                census(&env, portfolios, tokens);
                let outcome = (
                    remaining,
                    penalty,
                    reward,
                    expected,
                    portfolios.map(|key| {
                        let a = env.portfolio_state(key);
                        (a.capital.get(), a.pnl.get())
                    }),
                    [0, 1, 2, 3].map(|i| {
                        let a = env.market_state().1.assets[i];
                        (a.oi_eff_long_q, a.oi_eff_short_q)
                    }),
                );
                if let Some(reference) = &reference {
                    assert_eq!(
                        &outcome, reference,
                        "cpi={cpi}, batch={batch}, complete_first={complete_first}"
                    );
                } else {
                    reference = Some(outcome);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks), (8, 28));
    println!("CPI active keeper: {worlds} worlds, {rollbacks} exact rollbacks; peak CU={peak}; economics={reference:?}");
}
