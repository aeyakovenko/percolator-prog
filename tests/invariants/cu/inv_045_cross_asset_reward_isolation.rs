//! Row 422, INV-020/024/036/041/045/061/062/081: mixed Hybrid reward provenance.
//! Net-new: two distinct liquidatable assets share a keeper and observation list,
//! but only one has fresh authenticated evidence; the other has paid discovery.
//! Unlike selected-asset/inactive-sibling and exposed-recipient coverage, BOTH
//! assets collect nonzero penalties. Fresh evidence/retained rewards for one must
//! never make the other's penalty reclaimable, in either liquidation/hint order.
//! Only Clock, external reports and signer SOL are fixtures; economic accounts
//! and all transitions use public System/SPL/wrapper instructions. Bounded evidence.

use super::*;

fn observation(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    reward: Option<Pubkey>,
    assets: [u16; 2],
    reports: [Pubkey; 2],
    reverse: bool,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(signer, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    let observations = (if reverse { [1, 0] } else { [0, 1] })
        .map(|i| {
            accounts.push(AccountMeta::new_readonly(reports[i], false));
            CrankObservationHint {
                asset_index: assets[i],
                oracle_accounts: 1,
            }
        })
        .into();
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

#[test]
fn v16_program_cross_asset_hybrid_liquidations_isolate_paid_and_authenticated_rewards() {
    const DEPOSITS: [u64; 7] = [
        5_100_000,
        100_000_000,
        10_200_000,
        200_000_000,
        10_000_000,
        10_000_000,
        1_000,
    ];
    const SHARE: u128 = 3_333;
    const PRICES: [u64; 2] = [997_600, 1_995_200];
    const FRESH_TARGET: u64 = 1_970_000;
    // The existing two-asset liquidation owners use this per-instruction bound.
    const TWO_ASSET_CU: u64 = 500_000;
    let mut reference = None;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    for paid_asset in [0u16, 1] {
        for reverse in [false, true] {
            for fresh_first in [false, true] {
                let assets = [paid_asset, paid_asset ^ 1];
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        max_abs_funding_e9_per_slot: 0,
                        ..production_risk_params()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let feeds = [[0x91; 32], [0x92; 32]];
                let initial = [0, 1].map(|i| {
                    env.set_pyth_price_with_conf(
                        &feeds[i],
                        (ENTRY * (i as u64 + 1)) as i64,
                        -6,
                        0,
                        100,
                    )
                });
                for i in 0..2 {
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        assets[i],
                        1,
                        0,
                        [feeds[i], [0; 32], [0; 32]],
                        &[initial[i]],
                        1,
                        100,
                        0,
                        0,
                        1,
                        0,
                    )
                    .unwrap();
                }
                let owners: [Keypair; 7] = std::array::from_fn(|_| Keypair::new());
                let funded =
                    std::array::from_fn::<_, 7, _>(|i| fund(&mut env, &owners[i], DEPOSITS[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let keeper = portfolios[6];
                for i in 0..2 {
                    env.trade_asset_with_cu(
                        assets[i],
                        &owners[2 * i],
                        portfolios[2 * i],
                        &owners[2 * i + 1],
                        portfolios[2 * i + 1],
                        (100 * POS_SCALE) as i128,
                        ENTRY * (i as u64 + 1),
                        0,
                    );
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
                let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
                let supply = DEPOSITS.iter().map(|&v| v as u128).sum::<u128>();
                let mut tracked = vec![env.market, env.mint, env.vault, env.admin.pubkey()];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(initial);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                let values = |env: &V16CuEnv| {
                    portfolios.map(|key| {
                        let account = env.portfolio_state(key);
                        account.capital.get() as i128 + account.pnl.get()
                    })
                };
                let census = |env: &V16CuEnv| {
                    let group = env.market_state().1;
                    let accounts = portfolios.map(|key| env.portfolio_state(key));
                    assert_market_stock_census(
                        "cross-asset Hybrid rewards",
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        env.token_amount(env.vault) as u128,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(
                        "cross-asset Hybrid rewards",
                        &group,
                        &accounts,
                    )
                    .unwrap();
                    assert_source_credit_rates("cross-asset Hybrid rewards", &group).unwrap();
                    accounts.each_ref().map(|account| {
                        assert_current_certificate_matches_independent(
                            "cross-asset Hybrid rewards",
                            &group,
                            account,
                        )
                        .unwrap()
                    })
                };
                set_test_clock(&mut env, 5, 1_000);
                let tick = observation(
                    &env,
                    keeper,
                    owners[6].pubkey(),
                    None,
                    assets,
                    initial,
                    reverse,
                );
                submit_with_cu_limit(&mut env, &owners[6], &[tick], &tracked, None, TWO_ASSET_CU);
                env.trade_asset_with_cu(
                    assets[0],
                    &owners[4],
                    portfolios[4],
                    &owners[5],
                    portfolios[5],
                    POS_SCALE as i128,
                    900_000,
                    0,
                );
                let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
                let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
                let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
                let mut expected = DEPOSITS.map(i128::from);
                expected[4] -= (discovery / 2) as i128;
                expected[5] -= (discovery / 2) as i128;
                assert_eq!(values(&env), expected);
                assert_eq!(env.market_state().1.insurance, discovery);
                assert_eq!(
                    env.market_state().1.insurance_domain_budget_remaining_total,
                    0
                );
                assert!(discovery > 0);

                set_test_clock(&mut env, 6, 1_001);
                let fresh =
                    env.set_pyth_price_with_conf(&feeds[1], FRESH_TARGET as i64, -6, 0, 1_001);
                let reports = [initial[0], fresh];
                tracked.push(fresh);
                let crank = |env: &V16CuEnv, actor: usize, reward: Option<Pubkey>| {
                    observation(
                        env,
                        portfolios[actor],
                        owners[6].pubkey(),
                        reward,
                        assets,
                        reports,
                        reverse,
                    )
                };
                let publish = crank(&env, 6, None);
                submit_with_cu_limit(
                    &mut env,
                    &owners[6],
                    &[publish],
                    &tracked,
                    None,
                    TWO_ASSET_CU,
                );
                assert_eq!(values(&env), expected, "publication alone earns nothing");
                let market = env.svm.get_account(&env.market).unwrap();
                let profiles = assets.map(|asset| {
                    state::read_asset_oracle_profile(&market.data, asset as usize).unwrap()
                });
                assert_eq!(profiles.map(|p| p.last_good_oracle_slot), [1, 6]);
                assert_eq!(profiles.map(|p| p.oracle_target_publish_time), [100, 1_001]);
                assert_eq!(profiles[0].mark_ewma_e6, MARK);
                let group = env.market_state().1;
                assert_eq!(
                    assets.map(|a| group.assets[a as usize].effective_price),
                    PRICES
                );
                assert_eq!(
                    assets.map(|a| group.assets[a as usize].raw_oracle_target_price),
                    [MARK, FRESH_TARGET]
                );
                assert_eq!(group.insurance, discovery);
                census(&env);

                let mut penalties = [0u128; 2];
                let mut closed = [0u128; 2];
                let mut reward = 0;
                let mut budgets = group.insurance_domain_budget.clone();
                assert!(budgets.iter().all(|&amount| amount == 0));
                for i in if fresh_first { [1, 0] } else { [0, 1] } {
                    let actor = 2 * i;
                    let asset = assets[i] as usize;
                    let other = assets[i ^ 1] as usize;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        let before_values = values(&env);
                        let ready = census(&env)[actor]
                            && health_cert(&env.portfolio_state(portfolios[actor]))
                                .certified_liq_deficit
                                > 0;
                        let valid = crank(&env, actor, Some(keeper));
                        if ready {
                            // A real liquidation prefix must roll back with every sibling's
                            // existing penalty/reward when the suffix has an unauthorized tail.
                            let bad = crank(&env, actor, Some(portfolios[actor + 1]));
                            peak = peak.max(submit_with_cu_limit(
                                &mut env,
                                &owners[6],
                                &[valid.clone(), bad],
                                &tracked,
                                Some((
                                    3,
                                    InstructionError::Custom(PercolatorError::Unauthorized as u32),
                                )),
                                TWO_ASSET_CU,
                            ));
                            rollbacks += 1;
                        }
                        let foreign = [
                            portfolios[actor ^ 2],
                            portfolios[1],
                            portfolios[3],
                            portfolios[4],
                            portfolios[5],
                        ];
                        let before_foreign = foreign.map(|key| env.svm.get_account(&key));
                        peak = peak.max(submit_with_cu_limit(
                            &mut env,
                            &owners[6],
                            &[valid],
                            &tracked,
                            None,
                            TWO_ASSET_CU,
                        ));
                        let after = env.market_state().1;
                        assert_eq!(
                            after.assets[other], before.assets[other],
                            "sibling asset stays unchanged"
                        );
                        assert_eq!(foreign.map(|key| env.svm.get_account(&key)), before_foreign);
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        let market = env.svm.get_account(&env.market).unwrap();
                        assert_eq!(
                            assets.map(|a| state::read_asset_oracle_profile(
                                &market.data,
                                a as usize
                            )
                            .unwrap()),
                            profiles
                        );
                        census(&env);
                        closed[i] =
                            before.assets[asset].oi_eff_long_q - after.assets[asset].oi_eff_long_q;
                        if closed[i] == 0 {
                            assert_eq!(values(&env)[6], before_values[6]);
                            assert_eq!(after.insurance, before.insurance);
                            assert_eq!(after.insurance_domain_budget, budgets);
                            continue;
                        }
                        assert!(ready && closed[i] < 100 * POS_SCALE);
                        let penalty = fee(closed[i], PRICES[i], 5);
                        assert!(penalty > 0 && penalty * SHARE / 10_000 > 0);
                        assert_ne!(penalty, fee(closed[i], ENTRY * (i as u64 + 1), 5));
                        assert_ne!(penalty, fee(closed[i], [MARK, FRESH_TARGET][i], 5));
                        penalties[i] = penalty;
                        let receipt = if i == 1 { penalty * SHARE / 10_000 } else { 0 };
                        reward += receipt;
                        if i == 1 {
                            budgets[2 * asset] += (penalty - receipt) / 2;
                            budgets[2 * asset + 1] += (penalty - receipt).div_ceil(2);
                        }
                        let mut entitled = before_values;
                        entitled[actor] -= penalty as i128;
                        entitled[6] += receipt as i128;
                        assert_eq!(values(&env), entitled);
                        assert_eq!(
                            after.insurance,
                            discovery + penalties.iter().sum::<u128>() - reward
                        );
                        assert_eq!(
                            after.insurance_domain_budget, budgets,
                            "only the authenticated asset owns reclaimable fees"
                        );
                        assert_eq!(
                            after.insurance_domain_budget_remaining_total,
                            budgets.iter().sum::<u128>()
                        );
                        assert_eq!(
                            after.insurance - after.insurance_domain_budget_remaining_total,
                            discovery + penalties[0]
                        );
                        assert_eq!(
                            after.assets[asset].oi_eff_long_q,
                            after.assets[asset].oi_eff_short_q
                        );
                        assert_eq!(
                            health_cert(&env.portfolio_state(portfolios[actor]))
                                .certified_liq_deficit,
                            0
                        );
                        liquidations += 1;
                        break;
                    }
                    assert!(
                        closed[i] > 0,
                        "both provenance classes must actually liquidate"
                    );
                }
                assert!(reward > 0 && penalties[0] != penalties[1]);
                // Settle every passive counterparty so conservation cannot hide an
                // asset attribution error in a peer's latent K value.
                for actor in [1, 3, 4, 5] {
                    for _ in 0..6 {
                        if census(&env)[actor] {
                            break;
                        }
                        let refresh = crank(&env, actor, None);
                        peak = peak.max(submit_with_cu_limit(
                            &mut env,
                            &owners[6],
                            &[refresh],
                            &tracked,
                            None,
                            TWO_ASSET_CU,
                        ));
                    }
                    assert!(census(&env)[actor]);
                }
                for i in 0..2 {
                    let loss = 100 * i128::from(ENTRY * (i as u64 + 1) - PRICES[i]);
                    expected[2 * i] -= loss + penalties[i] as i128;
                    expected[2 * i + 1] += loss;
                }
                expected[4] -= i128::from(ENTRY - PRICES[0]);
                expected[5] += i128::from(ENTRY - PRICES[0]);
                expected[6] += reward as i128;
                assert_eq!(values(&env), expected);
                let payout = DEPOSITS[6] as u128 + reward;
                peak = peak.max(reward_policy_catchup::withdraw_reward(
                    &mut env, &owners[6], keeper, tokens[6], payout,
                ));
                expected[6] = 0;
                assert_eq!(values(&env), expected);
                assert_eq!(
                    tokens.map(|key| env.token_amount(key)),
                    [0, 0, 0, 0, 0, 0, payout as u64]
                );
                let final_group = env.market_state().1;
                assert_eq!(final_group.vault + payout, supply);
                assert_eq!(
                    final_group.insurance,
                    discovery + penalties.iter().sum::<u128>() - reward
                );
                assert_eq!(final_group.insurance_domain_budget, budgets);
                assert_eq!(
                    expected.iter().sum::<i128>() + final_group.insurance as i128,
                    final_group.vault as i128
                );
                assert_eq!(env.svm.get_account(&env.mint), custody[0]);
                census(&env);
                let normalized_budgets =
                    assets.map(|a| [budgets[2 * a as usize], budgets[2 * a as usize + 1]]);
                let outcome = (
                    closed,
                    penalties,
                    reward,
                    payout,
                    expected,
                    normalized_budgets,
                    final_group.vault,
                );
                if let Some(expected) = &reference {
                    assert_eq!(
                        &outcome, expected,
                        "asset indices and hint/liquidation order preserve entitlement"
                    );
                } else {
                    reference = Some(outcome);
                }
            }
        }
    }
    assert_eq!((liquidations, rollbacks), (16, 16));
    println!("cross-asset Hybrid rewards: worlds=8 liquidations={liquidations} exact_rollbacks={rollbacks} payouts=8 peak_CU={peak}");
}
