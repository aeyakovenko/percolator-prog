//! Row 422: mixed target provenance with a distinct, exposed Hybrid beneficiary.
//! Keeper K settlement crosses reward credit; public instructions own all value.

use super::*;

const DEPOSITS: [u64; 5] = [10_200_000, 100_000_000, 10_000_000, 10_000_000, 100_000];
const SHARE: u128 = 3_333;

#[test]
fn v16_program_mixed_selected_rewards_commute_with_exposed_keeper_settlement() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for elapsed in [1u64, 4] {
        let price = (ENTRY - elapsed * 2_400).max(MARK);
        for selected in [0usize, 1] {
            for direction in [-1i128, 1] {
                let mut reference = None;
                for settle_first in [false, true] {
                    let label = format!("elapsed={elapsed} selected={selected} direction={direction} settle_first={settle_first}");
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
                    let feed = [0x73; 32];
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
                    let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
                    let funded =
                        std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], DEPOSITS[i]));
                    let portfolios = funded.map(|pair| pair.0);
                    let tokens = funded.map(|pair| pair.1);
                    let [target, peer, trader_a, trader_b, keeper] = portfolios;
                    assert!(owners[..4]
                        .iter()
                        .all(|owner| owner.pubkey() != owners[4].pubkey()));
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
                    peak = peak.max(env.trade_asset_with_cu(
                        0,
                        &owners[4],
                        keeper,
                        &owners[1],
                        peer,
                        direction * POS_SCALE as i128,
                        ENTRY,
                        0,
                    ));
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
                    let advance =
                        observation(&env, trader_a, owners[4].pubkey(), initial, None, false);
                    peak = peak.max(submit(&mut env, &owners[4], &[advance], &tracked, None));
                    let oi = env.market_state().1.assets[0].oi_eff_long_q / POS_SCALE;
                    peak = peak.max(env.trade_asset_with_cu(
                        0,
                        &owners[2],
                        trader_a,
                        &owners[3],
                        trader_b,
                        POS_SCALE as i128,
                        900_000,
                        0,
                    ));
                    let required = (2 * oi * u128::from(ENTRY) * 77).div_ceil(10_000);
                    let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
                    let paid = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
                    assert_eq!(env.market_state().1.insurance, paid);
                    assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
                    peak = peak.max(env.push_auth_mark_for_asset_as_admin(1, 5, MARK));

                    // Prior-slot publication settles the keeper only to the prior price.
                    // The final K delta must therefore be nonzero in both order worlds.
                    for step in 0..elapsed {
                        set_test_clock(&mut env, 5 + step, 1_000 + step as i64);
                        let report = env.set_pyth_price_with_conf(
                            &feed,
                            MARK as i64,
                            -6,
                            0,
                            1_000 + step as i64,
                        );
                        tracked.push(report);
                        let publish =
                            observation(&env, keeper, owners[4].pubkey(), report, None, false);
                        peak = peak.max(submit(&mut env, &owners[4], &[publish], &tracked, None));
                        assert_eq!(
                            values(&env, portfolios)[4],
                            DEPOSITS[4] as i128 - direction * i128::from(step * 2_400)
                        );
                    }
                    let keeper_prior = env.portfolio_state(keeper);
                    let prior_value = keeper_prior.capital.get() as i128 + keeper_prior.pnl.get();
                    set_test_clock(&mut env, 5 + elapsed, 1_000 + elapsed as i64);
                    let fresh = env.set_pyth_price_with_conf(
                        &feed,
                        MARK as i64,
                        -6,
                        0,
                        1_000 + elapsed as i64,
                    );
                    let conflict = env.set_pyth_price_with_conf(
                        &feed,
                        MARK as i64 + 1,
                        -6,
                        0,
                        1_000 + elapsed as i64,
                    );
                    tracked.extend([fresh, conflict]);
                    let settle = observation(&env, keeper, owners[4].pubkey(), fresh, None, false);
                    if settle_first {
                        peak = peak.max(submit(
                            &mut env,
                            &owners[4],
                            &[settle.clone()],
                            &tracked,
                            None,
                        ));
                        assert_ne!(values(&env, portfolios)[4], prior_value);
                    }
                    let valid =
                        observation(&env, target, owners[4].pubkey(), fresh, Some(keeper), false);
                    let invalid = observation(
                        &env,
                        target,
                        owners[4].pubkey(),
                        conflict,
                        Some(keeper),
                        false,
                    );
                    let foreign = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                    let mut liquidation = None;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        let recipient = env.portfolio_state(keeper);
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
                        assert_eq!(
                            profile.effective_price_provenance,
                            if elapsed == 4 {
                                percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_AUTHENTICATED
                            } else {
                                percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_TRADE_DRIVEN
                            }
                        );
                        let closed = before.assets[selected].oi_eff_long_q
                            - after.assets[selected].oi_eff_long_q;
                        assert_eq!(
                            before.assets[selected ^ 1].oi_eff_long_q,
                            after.assets[selected ^ 1].oi_eff_long_q
                        );
                        let penalty = fee(closed, price, 5);
                        let eligible = selected == 1 || elapsed == 4;
                        let reward = if eligible {
                            penalty * SHARE / 10_000
                        } else {
                            0
                        };
                        let mut expected_recipient = recipient;
                        expected_recipient.capital =
                            percolator::V16PodU128::new(recipient.capital.get() + reward);
                        if reward > 0 {
                            expected_recipient.health_cert.valid = 0;
                        }
                        assert_eq!(
                            env.portfolio_state(keeper),
                            expected_recipient,
                            "credit cannot settle or overwrite keeper K: {label}"
                        );
                        assert_eq!(
                            [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                            foreign
                        );
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        assert_eq!(after.insurance, paid + penalty - reward);
                        let mut budgets = before.insurance_domain_budget;
                        if eligible {
                            budgets[selected * 2] += (penalty - reward) / 2;
                            budgets[selected * 2 + 1] += (penalty - reward).div_ceil(2);
                        }
                        assert_eq!(after.insurance_domain_budget, budgets);
                        assert_eq!(
                            after.insurance_domain_budget_remaining_total,
                            budgets.iter().sum::<u128>()
                        );
                        if closed != 0 {
                            assert!(closed < 100 * POS_SCALE && penalty * SHARE / 10_000 > 0);
                            assert_ne!(penalty, fee(closed, ENTRY, 5));
                            if elapsed == 1 {
                                assert_ne!(penalty, fee(closed, MARK, 5));
                            }
                            assert_eq!(
                                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                                0
                            );
                            assert!(census(&env, portfolios)[0]);
                            liquidation = Some((closed, penalty, reward));
                            break;
                        }
                    }
                    let (closed, penalty, reward) =
                        liquidation.expect("mixed target liquidation with an exposed beneficiary");
                    if !settle_first {
                        assert_eq!(env.portfolio_state(keeper).pnl, keeper_prior.pnl);
                        peak = peak.max(submit(&mut env, &owners[4], &[settle], &tracked, None));
                        assert_ne!(values(&env, portfolios)[4] - reward as i128, prior_value);
                    }
                    for account in [peer, trader_a, trader_b] {
                        let refresh =
                            observation(&env, account, owners[4].pubkey(), fresh, None, false);
                        peak = peak.max(submit(&mut env, &owners[4], &[refresh], &tracked, None));
                    }
                    let loss = i128::from(ENTRY - price);
                    let mut expected = DEPOSITS.map(i128::from);
                    expected[0] -= 200 * loss + penalty as i128;
                    expected[1] += (200 + direction) * loss;
                    expected[2] -= (paid / 2) as i128 + loss;
                    expected[3] += loss - (paid / 2) as i128;
                    expected[4] += reward as i128 - direction * loss;
                    assert_eq!(values(&env, portfolios), expected, "{label}");
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
                        data: env.withdraw_ix(keeper, reward.max(1)).encode(),
                    };
                    peak = peak.max(submit(
                        &mut env,
                        &owners[4],
                        &[withdraw],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                    ));
                    rollbacks += 1;
                    assert!(has_active_leg_for_asset(&env.portfolio_state(keeper), 0));
                    assert!(tokens.iter().all(|key| env.token_amount(*key) == 0));
                    assert_eq!(
                        [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                        custody
                    );
                    let group = env.market_state().1;
                    assert_eq!(group.vault, env.token_amount(env.vault) as u128);
                    assert_eq!(
                        group.vault,
                        DEPOSITS.iter().map(|value| *value as u128).sum::<u128>()
                    );
                    assert_eq!(
                        expected.iter().sum::<i128>() + group.insurance as i128,
                        group.vault as i128
                    );
                    census(&env, portfolios);
                    let accounts = portfolios.map(|key| {
                        let p = env.portfolio_state(key);
                        (p.capital.get(), p.pnl.get(), p.legs)
                    });
                    let outcome = (
                        closed,
                        penalty,
                        reward,
                        expected,
                        accounts,
                        group.insurance,
                        group.insurance_domain_budget,
                        group.vault,
                    );
                    if let Some(reference) = &reference {
                        assert_eq!(
                            &outcome, reference,
                            "settlement order preserves each portfolio: {label}"
                        );
                    } else {
                        reference = Some(outcome);
                    }
                    worlds += 1;
                }
                let (_, penalty, reward, ..) = reference.unwrap();
                eprintln!("exposed selected={selected} elapsed={elapsed} direction={direction} penalty={penalty} reward={reward}");
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_eq!(rollbacks, 48);
    assert_cu_within("mixed selected exposed keeper", peak, 650_000);
    eprintln!(
        "mixed selected exposed keeper: worlds={worlds} rollbacks={rollbacks} peak_cu={peak}"
    );
}
