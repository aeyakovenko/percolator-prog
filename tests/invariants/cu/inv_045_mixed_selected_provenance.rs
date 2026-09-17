//! Two exposed target legs disagree on reward provenance until Hybrid catchup.
//! Persisted leg order selects the fee source; observation order must not select it.

use super::*;

fn submit(
    env: &mut V16CuEnv,
    signer: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
) -> u64 {
    submit_with_cu_limit(env, signer, instructions, tracked, rejection, 500_000)
}

fn observation(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    report: Pubkey,
    keeper: Option<Pubkey>,
    reverse: bool,
) -> Instruction {
    let mut ix = observe(env, target, signer, Some(report), keeper);
    ix.data = ProgInstruction::PermissionlessCrank {
        now_slot: u64::MAX,
        observations: (if reverse { [1, 0] } else { [0, 1] })
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: u8::from(asset_index == 0),
            })
            .into(),
    }
    .encode();
    ix
}

#[test]
fn v16_program_mixed_exposed_legs_bind_rewards_to_selected_price_lineage() {
    const DEPOSITS: [u64; 5] = [10_200_000, 100_000_000, 10_000_000, 10_000_000, 1_000];
    const SHARE: u128 = 3_333;
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for elapsed in [1u64, 4] {
        let price = (ENTRY - elapsed * 2_400).max(MARK);
        for selected in [0usize, 1] {
            let mut reference = None;
            for reverse in [false, true] {
                for (batch, cpi) in [(false, false), (true, false), (false, true), (true, true)] {
                    let label = format!("elapsed={elapsed} selected={selected} reverse={reverse} batch={batch} cpi={cpi}");
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            max_abs_funding_e9_per_slot: 0,
                            ..production_risk_params()
                        },
                    );
                    set_test_clock(&mut env, 1, 100);
                    env.configure_auth_mark_for_asset_as_admin(1, 1, ENTRY);
                    env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                    let feed = [0x72; 32];
                    let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        0,
                        1,
                        0,
                        [feed, [0; 32], [0; 32]],
                        &[initial],
                        1,
                        100,
                        0,
                        0,
                        1,
                        0,
                    )
                    .unwrap();
                    let coalition = Keypair::new();
                    let owners: [Keypair; 5] = std::array::from_fn(|i| {
                        if i < 2 {
                            Keypair::new()
                        } else {
                            Keypair::from_bytes(&coalition.to_bytes()).unwrap()
                        }
                    });
                    let funded =
                        std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], DEPOSITS[i]));
                    let portfolios = funded.map(|pair| pair.0);
                    let tokens = funded.map(|pair| pair.1);
                    let [target, peer, trader_a, trader_b, keeper] = portfolios;
                    for asset in [selected, selected ^ 1] {
                        peak = peak.max(env.trade_asset_with_cu(
                            asset as u16,
                            &owners[0],
                            target,
                            &owners[1],
                            peer,
                            (100 * POS_SCALE) as i128,
                            ENTRY,
                            0,
                        ));
                    }
                    let mut tracked =
                        vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
                    tracked.extend(portfolios);
                    tracked.extend(tokens);
                    tracked.extend(owners.each_ref().map(Signer::pubkey));
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
                    set_test_clock(&mut env, 5, 1_000);
                    let clock_only =
                        observation(&env, keeper, owners[4].pubkey(), initial, None, reverse);
                    peak = peak.max(submit(&mut env, &owners[4], &[clock_only], &tracked, None));
                    peak = peak.max(paid_origin_routes::discover(
                        &mut env,
                        &owners,
                        portfolios,
                        &mut tracked,
                        batch,
                        cpi,
                    ));
                    let paid = env.market_state().1.insurance;
                    assert!(paid > 0, "{label}");
                    assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
                    let post_discovery = values(&env, portfolios);
                    assert_eq!(post_discovery[0], DEPOSITS[0] as i128);
                    assert_eq!(post_discovery[1], DEPOSITS[1] as i128);
                    assert_eq!(post_discovery[4], DEPOSITS[4] as i128);
                    assert_eq!(post_discovery[2], post_discovery[3]);
                    assert_eq!(DEPOSITS[2] as i128 - post_discovery[2], (paid / 2) as i128);
                    assert_eq!(paid % 2, 0);
                    peak = peak.max(env.push_auth_mark_for_asset_as_admin(1, 5, MARK));

                    // Renew equal-price evidence on the flat keeper. Both target legs stay
                    // exposed and unsettled, so either first leg must size against both losses.
                    let mut report = initial;
                    for step in 0..elapsed {
                        set_test_clock(&mut env, 5 + step, 1_000 + step as i64);
                        report = env.set_pyth_price_with_conf(
                            &feed,
                            MARK as i64,
                            -6,
                            0,
                            1_000 + step as i64,
                        );
                        tracked.push(report);
                        let publish =
                            observation(&env, keeper, owners[4].pubkey(), report, None, reverse);
                        peak = peak.max(submit(&mut env, &owners[4], &[publish], &tracked, None));
                        assert_eq!(values(&env, portfolios), post_discovery);
                    }
                    set_test_clock(&mut env, 5 + elapsed, 1_000 + elapsed as i64);
                    let stale = observation(
                        &env,
                        target,
                        owners[4].pubkey(),
                        report,
                        Some(keeper),
                        reverse,
                    );
                    peak = peak.max(submit(
                        &mut env,
                        &owners[4],
                        &[stale],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                    ));
                    rollbacks += 1;
                    let fresh = env.set_pyth_price_with_conf(
                        &feed,
                        MARK as i64,
                        -6,
                        0,
                        1_000 + elapsed as i64,
                    );
                    let conflicting = env.set_pyth_price_with_conf(
                        &feed,
                        MARK as i64 + 1,
                        -6,
                        0,
                        1_000 + elapsed as i64,
                    );
                    tracked.extend([fresh, conflicting]);
                    let valid = observation(
                        &env,
                        target,
                        owners[4].pubkey(),
                        fresh,
                        Some(keeper),
                        reverse,
                    );
                    let invalid = observation(
                        &env,
                        target,
                        owners[4].pubkey(),
                        conflicting,
                        Some(keeper),
                        reverse,
                    );
                    let budgets = env.market_state().1.insurance_domain_budget;
                    let foreign = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                    let mut liquidation = None;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        peak = peak.max(submit(
                            &mut env,
                            &owners[4],
                            &[valid.clone(), invalid.clone()],
                            &tracked,
                            Some((
                                3,
                                InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                            )),
                        ));
                        rollbacks += 1;
                        peak = peak.max(submit(
                            &mut env,
                            &owners[4],
                            &[valid.clone()],
                            &tracked,
                            None,
                        ));
                        let after = env.market_state().1;
                        for asset in 0..2 {
                            assert_eq!(after.assets[asset].effective_price, price, "{label}");
                            assert_eq!(after.assets[asset].raw_oracle_target_price, MARK);
                            assert_eq!(
                                after.assets[asset].oi_eff_long_q,
                                after.assets[asset].oi_eff_short_q
                            );
                        }
                        let profile = state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap();
                        assert_eq!(profile.last_good_oracle_slot, 5 + elapsed);
                        assert_eq!(profile.oracle_target_publish_time, 1_000 + elapsed as i64);
                        assert_eq!(
                            profile.effective_price_provenance,
                            if elapsed == 4 {
                                percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED
                            } else {
                                percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_TRADE_DRIVEN
                            }
                        );
                        assert_eq!(
                            [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                            foreign
                        );
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        let closed = before.assets[selected].oi_eff_long_q
                            - after.assets[selected].oi_eff_long_q;
                        assert_eq!(
                            before.assets[selected ^ 1].oi_eff_long_q,
                            after.assets[selected ^ 1].oi_eff_long_q
                        );
                        if closed == 0 {
                            assert_eq!(after.insurance, paid);
                            assert_eq!(after.insurance_domain_budget, budgets);
                            continue;
                        }
                        let penalty = fee(closed, price, 5);
                        assert!(closed < 100 * POS_SCALE && penalty > 0, "{label}");
                        assert_ne!(penalty, fee(closed, ENTRY, 5));
                        if elapsed == 1 {
                            assert_ne!(penalty, fee(closed, MARK, 5));
                        }
                        let eligible = selected == 1 || elapsed == 4;
                        let reward = if eligible {
                            penalty * SHARE / 10_000
                        } else {
                            0
                        };
                        assert!(penalty * SHARE / 10_000 > 0);
                        let retained = penalty - reward;
                        assert_eq!(after.insurance, paid + retained, "{label}");
                        let mut expected_budgets = budgets;
                        if eligible {
                            expected_budgets[selected * 2] += retained / 2;
                            expected_budgets[selected * 2 + 1] += retained.div_ceil(2);
                        }
                        assert_eq!(after.insurance_domain_budget, expected_budgets, "{label}");
                        assert_eq!(
                            after.insurance_domain_budget_remaining_total,
                            expected_budgets.iter().sum::<u128>()
                        );
                        let current_values = values(&env, portfolios);
                        assert_eq!(
                            current_values[0],
                            DEPOSITS[0] as i128 - 200 * i128::from(ENTRY - price) - penalty as i128
                        );
                        assert_eq!(current_values[4], DEPOSITS[4] as i128 + reward as i128);
                        assert_eq!(
                            health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                            0
                        );
                        assert!(census(&env, portfolios)[0], "current target certificate");
                        liquidation = Some((closed, penalty, reward, expected_budgets));
                        break;
                    }
                    let (closed, penalty, reward, budgets) =
                        liquidation.expect("bounded mixed-leg liquidation");
                    for account in [peer, trader_a, trader_b] {
                        let refresh =
                            observation(&env, account, owners[4].pubkey(), fresh, None, reverse);
                        peak = peak.max(submit(&mut env, &owners[4], &[refresh], &tracked, None));
                    }
                    let mut expected = post_discovery;
                    let loss = i128::from(ENTRY - price);
                    expected[0] -= 200 * loss + penalty as i128;
                    expected[1] += 200 * loss;
                    expected[2] -= loss;
                    expected[3] += loss;
                    expected[4] += reward as i128;
                    assert_eq!(values(&env, portfolios), expected, "{label}");
                    census(&env, portfolios);
                    let payout = DEPOSITS[4] + reward as u64;
                    let withdraw = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[4].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(keeper, false),
                            AccountMeta::new(tokens[4], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(keeper, payout as u128).encode(),
                    };
                    peak = peak.max(submit(&mut env, &owners[4], &[withdraw], &tracked, None));
                    expected[4] = 0;
                    assert_eq!(values(&env, portfolios), expected);
                    assert_eq!(env.token_amount(tokens[4]), payout);
                    let group = env.market_state().1;
                    assert_eq!(
                        group.vault,
                        DEPOSITS.iter().sum::<u64>() as u128 - payout as u128
                    );
                    assert_eq!(group.vault, env.token_amount(env.vault) as u128);
                    assert_eq!(
                        expected.iter().sum::<i128>() + group.insurance as i128,
                        group.vault as i128
                    );
                    assert_eq!(env.svm.get_account(&env.mint), custody[0]);
                    // The wash pair and reward recipient share one key, but pay the full
                    // discovery cost. Their separate portfolios cannot reclaim that fee.
                    assert_eq!(
                        expected[2] + expected[3] + payout as i128,
                        (DEPOSITS[2] + DEPOSITS[3] + DEPOSITS[4]) as i128 - paid as i128
                            + reward as i128
                    );
                    assert!(reward < paid);
                    census(&env, portfolios);
                    let outcome = (
                        closed,
                        penalty,
                        reward,
                        expected,
                        budgets,
                        group.insurance,
                        group.vault,
                    );
                    if let Some(reference) = &reference {
                        assert_eq!(&outcome, reference, "{label}");
                    } else {
                        reference = Some(outcome);
                    }
                    worlds += 1;
                }
            }
            let (closed, penalty, reward, ..) = reference.unwrap();
            eprintln!("mixed provenance elapsed={elapsed} selected={selected} closed={closed} penalty={penalty} reward={reward}");
        }
    }
    assert_eq!(worlds, 32);
    assert_cu_within("mixed selected provenance", peak, 650_000);
    eprintln!("mixed selected provenance: worlds={worlds} rollbacks={rollbacks} peak_cu={peak}");
}
