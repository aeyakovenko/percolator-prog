//! Row 426: a paid keeper becomes the next target only after its separate composite refresh.
//! Public economic construction; only Clock, signer SOL and Pyth reports are fixtures.
//! The first reward is retained across rejected recipient refreshes and is included exactly
//! once in the second target's independently checked equity, penalty and final SPL payout.

use super::*;

const ENDOWMENTS: [u128; 4] = [10_000_000, 220_000, 120_000, 10_000_000];

fn observation(
    env: &V16CuEnv,
    target: Pubkey,
    signer: &Keypair,
    reward: Option<Pubkey>,
    reports: [Pubkey; 3],
    assets: &[u16],
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(signer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    let observations = assets
        .iter()
        .map(|&asset_index| {
            let tail: &[Pubkey] = match asset_index {
                0 => &reports[..1],
                2 => &reports[1..],
                _ => &[],
            };
            accounts.extend(
                tail.iter()
                    .map(|key| AccountMeta::new_readonly(*key, false)),
            );
            CrankObservationHint {
                asset_index,
                oracle_accounts: tail.len() as u8,
            }
        })
        .collect();
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations,
        }
        .encode(),
    }
}

fn census(env: &V16CuEnv, portfolios: [Pubkey; 4], tokens: [Pubkey; 4]) {
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "recipient becomes target",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("recipient becomes target", &group, &accounts).unwrap();
    for account in &accounts {
        assert_current_certificate_matches_independent("recipient becomes target", &group, account)
            .unwrap();
    }
    let supply = ENDOWMENTS.iter().sum::<u128>() as u64;
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!((mint.supply, mint.mint_authority), (supply, COption::None));
    assert_eq!(
        env.token_amount(env.vault) + tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
        supply
    );
    assert_eq!(group.mode, MarketModeV16::Live);
}

fn penalty(remaining: u128, price: u64) -> u128 {
    assert!(remaining > 0 && remaining < POS_SCALE);
    ((POS_SCALE - remaining) * u128::from(price))
        .div_ceil(POS_SCALE)
        .div_ceil(100)
        .min(10_000)
}

#[test]
fn v16_program_reward_recipient_becomes_liquidation_target_after_composite_refresh() {
    let mut reference = None;
    let mut peak = 0;
    let mut rollbacks = 0;
    for omit_after_refresh in [false, true] {
        for batch_exit in [false, true] {
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
                    liquidation_fee_bps: 100,
                    liquidation_fee_cap: 10_000,
                    ..V16CuMarketParams::default()
                },
            );
            set_test_clock(&mut env, 0, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feeds = [[0xcd; 32], [0xce; 32], [0xcf; 32]];
            let initial =
                feeds.map(|feed| env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100));
            for (asset, count, flags, feed_set, reports) in [
                (0, 1, 0, [feeds[0], [0; 32], [0; 32]], &initial[..1]),
                (
                    2,
                    2,
                    ORACLE_LEG_FLAG_DIVIDE_LEG2,
                    [feeds[1], feeds[2], [0; 32]],
                    &initial[1..],
                ),
            ] {
                env.try_configure_hybrid_asset_with_conf_filter_cu(
                    asset, count, flags, feed_set, reports, 0, 100, 0, 0, 100, 0,
                )
                .unwrap();
            }
            for asset in [1, 3] {
                env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
            }
            let owners = std::array::from_fn(|_| Keypair::new());
            let funded = [0, 1, 2, 3].map(|i| funded_owner(&mut env, &owners[i], ENDOWMENTS[i]));
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
            for (asset, long, short) in [(0, 0, 1), (1, 0, 1), (2, 3, 2)] {
                env.trade_asset_with_cu(
                    asset,
                    &owners[long],
                    portfolios[long],
                    &owners[short],
                    portfolios[short],
                    POS_SCALE as i128,
                    PRICE,
                    0,
                );
            }
            let mut tracked = vec![
                env.market,
                env.mint,
                env.vault,
                env.admin.pubkey(),
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            tracked.extend(initial);
            set_test_clock(&mut env, 0, 101);
            env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
            let reports = [0, 1, 2].map(|i| {
                env.set_pyth_price_with_conf(
                    &feeds[i],
                    [CURRENT[0], KEEPER_PRICE, PRICE][i] as i64,
                    -6,
                    0,
                    101,
                )
            });
            tracked.extend(reports);
            for slot in [0, 64] {
                set_test_clock(&mut env, slot, 102);
                let before = frame(&env, &portfolios);
                let ix = observation(
                    &env,
                    target,
                    &owners[2],
                    Some(keeper),
                    reports,
                    &[0, 1, 2, 3],
                );
                peak = peak.max(transact(&mut env, &[&owners[2]], &[ix], &tracked, None));
                if slot == 64 {
                    assert_eq!(frame(&env, &portfolios), before, "market-only prefix");
                }
            }
            assert_eq!(
                [0, 1, 2].map(|i| env.market_state().1.assets[i].slot_last),
                [32; 3]
            );
            let keeper_leg = active_leg_for_asset(&env.portfolio_state(keeper), 2);
            let ix = observation(&env, target, &owners[2], Some(keeper), reports, &[0, 1]);
            peak = peak.max(transact(
                &mut env,
                &[&owners[2]],
                &[ix.clone()],
                &tracked,
                None,
            ));
            assert_current_short(&env, target, 130_000, 209_000);
            peak = peak.max(transact(&mut env, &[&owners[2]], &[ix], &tracked, None));
            let first_remaining = env.market_state().1.assets[0].oi_eff_short_q;
            let first_fee = penalty(first_remaining, CURRENT[0]);
            let first_reward = first_fee * SHARE / 10_000;
            assert!(first_reward > 0);
            assert_eq!(
                env.portfolio_state(target).capital.get(),
                130_000 - first_fee
            );
            assert_eq!(
                env.portfolio_state(keeper).capital.get(),
                ENDOWMENTS[2] + first_reward
            );
            assert!(!health_cert(&env.portfolio_state(keeper)).valid);
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(keeper), 2),
                keeper_leg
            );
            assert_eq!(env.market_state().1.assets[2].slot_last, 32);
            let first_target = env.svm.get_account(&target);
            census(&env, portfolios, tokens);

            let coherent = [0, 1, 2].map(|i| {
                env.set_pyth_price_with_conf(
                    &feeds[i],
                    [CURRENT[0], KEEPER_PRICE, PRICE][i] as i64,
                    -6,
                    0,
                    102,
                )
            });
            tracked.extend(coherent);
            let mixed = [coherent[0], coherent[1], reports[2]];
            // Both component reports are fresh individually; their composite epoch is not.
            for refreshed in [false, true] {
                let incoherent =
                    observation(&env, keeper, &owners[3], Some(keeper_peer), mixed, &[2]);
                peak = peak.max(transact(
                    &mut env,
                    &[&owners[3]],
                    &[incoherent],
                    &tracked,
                    Some((2, PercolatorError::OracleStale)),
                ));
                rollbacks += 1;
                if !refreshed {
                    // Another portfolio commits the complete provider state, then the recipient
                    // consumes it without any oracle-account tail of its own.
                    let recipient_before = env.svm.get_account(&keeper);
                    let update = observation(&env, keeper_peer, &owners[3], None, coherent, &[2]);
                    peak = peak.max(transact(&mut env, &[&owners[3]], &[update], &tracked, None));
                    assert_eq!(env.svm.get_account(&keeper), recipient_before);
                    let market = env.svm.get_account(&env.market).unwrap();
                    let profile = state::read_asset_oracle_profile(&market.data, 2).unwrap();
                    assert_eq!(profile.oracle_leg_publish_times, [102, 102, 0]);
                    assert_eq!(profile.oracle_leg_prices_e6, [KEEPER_PRICE, PRICE, 0]);
                    assert_eq!(profile.last_good_oracle_slot, 64);
                    assert_eq!(env.market_state().1.assets[2].slot_last, 64);
                    let refresh = observation(&env, keeper, &owners[3], None, coherent, &[]);
                    peak = peak.max(transact(
                        &mut env,
                        &[&owners[3]],
                        &[refresh],
                        &tracked,
                        None,
                    ));
                    let account = env.portfolio_state(keeper);
                    let equity = ENDOWMENTS[2] + first_reward - u128::from(KEEPER_PRICE - PRICE);
                    let cert = health_cert(&account);
                    assert!(assert_current_certificate_matches_independent(
                        "second liquidation target",
                        &env.market_state().1,
                        &account,
                    )
                    .unwrap());
                    assert_eq!(account.capital.get(), equity);
                    assert_eq!(cert.certified_equity, equity as i128);
                    assert_eq!(
                        cert.certified_maintenance_req,
                        u128::from(KEEPER_PRICE) / 10
                    );
                    assert_eq!(
                        cert.certified_liq_deficit,
                        u128::from(KEEPER_PRICE) / 10 - equity
                    );
                    assert!(cert.certified_liq_deficit > 0);
                    assert_eq!(env.market_state().1.assets[2].oi_eff_short_q, POS_SCALE);
                    assert_eq!(env.market_state().1.insurance, first_fee - first_reward);
                    census(&env, portfolios, tokens);
                }
            }
            let liquidate = observation(
                &env,
                keeper,
                &owners[3],
                Some(keeper_peer),
                coherent,
                if omit_after_refresh { &[] } else { &[2] },
            );
            peak = peak.max(transact(
                &mut env,
                &[&owners[3]],
                &[liquidate],
                &tracked,
                None,
            ));
            let remaining = env.market_state().1.assets[2].oi_eff_short_q;
            assert_eq!(env.market_state().1.assets[2].oi_eff_long_q, remaining);
            let second_fee = penalty(remaining, KEEPER_PRICE);
            let second_reward = second_fee * SHARE / 10_000;
            assert!(second_reward > 0);
            let expected =
                ENDOWMENTS[2] + first_reward - u128::from(KEEPER_PRICE - PRICE) - second_fee;
            assert_eq!(env.portfolio_state(keeper).capital.get(), expected);
            let cert = health_cert(&env.portfolio_state(keeper));
            assert!(cert.valid);
            assert_eq!(cert.certified_equity, expected as i128);
            assert_eq!(cert.certified_liq_deficit, 0);
            assert_eq!(
                env.portfolio_state(keeper_peer).capital.get(),
                ENDOWMENTS[3] + second_reward
            );
            assert_eq!(
                env.portfolio_state(keeper_peer).pnl.get(),
                i128::from(KEEPER_PRICE - PRICE)
            );
            let insurance = [first_fee - first_reward, second_fee - second_reward];
            assert_eq!(env.market_state().1.insurance, insurance.iter().sum());
            assert_eq!(
                env.market_state().1.insurance_domain_budget,
                [
                    insurance[0] / 2,
                    insurance[0] - insurance[0] / 2,
                    0,
                    0,
                    insurance[1] / 2,
                    insurance[1] - insurance[1] / 2,
                    0,
                    0
                ]
            );
            assert_eq!(env.svm.get_account(&target), first_target);
            census(&env, portfolios, tokens);

            let close = trade(
                &env,
                &owners,
                portfolios,
                &[2],
                remaining as i128,
                batch_exit,
            );
            peak = peak.max(transact(
                &mut env,
                &[&owners[2], &owners[3]],
                &[close],
                &tracked,
                None,
            ));
            assert!(!has_active_leg_for_asset(&env.portfolio_state(keeper), 2));
            assert!(!has_active_leg_for_asset(
                &env.portfolio_state(keeper_peer),
                2
            ));
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
            assert_eq!(env.token_amount(tokens[2]) as u128, expected);
            assert_eq!(env.svm.get_account(&target), first_target);
            census(&env, portfolios, tokens);
            let outcome = (
                first_remaining,
                remaining,
                first_fee,
                first_reward,
                second_fee,
                second_reward,
                expected,
                portfolios.map(|key| {
                    let account = env.portfolio_state(key);
                    (account.capital.get(), account.pnl.get())
                }),
            );
            if let Some(reference) = &reference {
                assert_eq!(&outcome, reference);
            } else {
                reference = Some(outcome);
            }
            assert!(env.portfolio_state(peer).capital.get() >= ENDOWMENTS[0]);
        }
    }
    assert_eq!(rollbacks, 8);
    assert_cu_within("composite recipient liquidation history", peak, 500_000);
    println!("recipient becomes target: 4 worlds, {rollbacks} complete rollbacks; peak {peak} CU; economics={reference:?}");
}
