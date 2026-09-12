//! INV-036/045/061, row422: earned rewards across policy succession during catchup.
//! A retained liquidation instruction crosses the policy update after certification;
//! paying the earlier receipt before or after that update must preserve entitlement.

use super::*;

fn withdraw_reward(
    env: &mut V16CuEnv,
    owner: &Keypair,
    keeper: Pubkey,
    tokens: Pubkey,
    amount: u128,
) -> u64 {
    let before = env.market_state().1;
    let token_before = env.token_amount(tokens);
    env.svm.expire_blockhash();
    let cu = env
        .send(
            env.withdraw_ix(keeper, amount),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(keeper, false),
                AccountMeta::new(tokens, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("earned reward remains payable across policy succession");
    let after = env.market_state().1;
    assert_eq!(
        env.token_amount(tokens) as u128,
        token_before as u128 + amount
    );
    assert_eq!(after.vault, before.vault - amount);
    assert_eq!(env.token_amount(env.vault) as u128, after.vault);
    assert_eq!(after.insurance, before.insurance);
    assert_eq!(
        after.insurance_domain_budget,
        before.insurance_domain_budget
    );
    assert_cu_within("policy catchup keeper payout", cu, CUSTODY_CU_LIMIT);
    cu
}

#[test]
fn v16_program_reward_policy_succession_preserves_receipts_and_effective_price_catchup() {
    const SHARES: [u16; 2] = [3_333, 7_777];
    const FINAL_TARGET: u64 = 980_000;
    let first_price = ENTRY - ENTRY * 24 / 10_000;
    let second_price = first_price - first_price * 24 / 10_000;
    let mut reference = None;
    let mut peak_crank = 0;
    let mut peak_payout = 0;
    let mut peak_policy = 0;
    for pay_first in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_abs_funding_e9_per_slot: 0,
                ..production_risk_params()
            },
        );
        set_test_clock(&mut env, 1, 100);
        env.update_liquidation_fee_policy_with_cu(SHARES[0]);
        let feed = [0x6e; 32];
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
        .expect("public direct-price Hybrid configuration");
        let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
        let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
        let portfolios = funded.map(|pair| pair.0);
        let tokens = funded.map(|pair| pair.1);
        let [target, peer, trader_a, trader_b, keeper] = portfolios;
        env.trade_asset_with_cu(
            0,
            &owners[0],
            target,
            &owners[1],
            peer,
            (100 * POS_SCALE) as i128,
            ENTRY,
            0,
        );
        let mut tracked = vec![env.market, env.mint, env.vault, initial];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
        let mint = env.svm.get_account(&env.mint);
        let supply = FUNDS.iter().map(|value| *value as u128).sum::<u128>();
        set_test_clock(&mut env, 5, 1_000);
        let clock_only = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
        submit(&mut env, &owners[4], &[clock_only], &tracked, None);
        env.trade_asset_with_cu(
            0,
            &owners[2],
            trader_a,
            &owners[3],
            trader_b,
            POS_SCALE as i128,
            900_000,
            0,
        );
        let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
        let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
        let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
        let staged = env.market_state().1;
        assert_eq!(staged.insurance, discovery);
        assert_eq!(staged.assets[0].effective_price, ENTRY);
        assert_eq!(staged.assets[0].raw_oracle_target_price, MARK);
        census(&env, portfolios);

        let mut total_fee = 0;
        let mut total_reward = 0;
        let mut paid = 0;
        let mut budgets = [0u128; 2];
        let mut episodes = Vec::new();
        for (phase, slot, report_price, price) in [
            (0, 6, MARK, first_price),
            (1, 7, FINAL_TARGET, second_price),
            (2, 14, FINAL_TARGET, FINAL_TARGET),
        ] {
            let now = 995 + slot as i64;
            set_test_clock(&mut env, slot, now);
            let report = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, now);
            tracked.push(report);
            let retained = observe(&env, target, owners[4].pubkey(), Some(report), Some(keeper));
            let mut policy_updated = phase != 1;
            let mut liquidated = false;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env, portfolios);
                let before_cert = health_cert(&env.portfolio_state(target));
                if phase == 2
                    && before.assets[0].effective_price == price
                    && before_cert.certified_liq_deficit == 0
                    && census(&env, portfolios)[0]
                {
                    break;
                }
                let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                let vault = env.svm.get_account(&env.vault);
                peak_crank = peak_crank.max(submit(
                    &mut env,
                    &owners[4],
                    &[retained.clone()],
                    &tracked,
                    None,
                ));
                let current = census(&env, portfolios);
                let (profile, after) = env.market_state();
                assert_eq!(after.assets[0].effective_price, price);
                assert_eq!(after.assets[0].raw_oracle_target_price, report_price);
                assert_eq!(profile.oracle_target_publish_time, now);
                assert_eq!(profile.last_good_oracle_slot, slot);
                assert_eq!(
                    [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                    peers
                );
                assert_eq!(env.svm.get_account(&env.vault), vault);
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert_eq!(values(&env, portfolios)[4], before_values[4]);
                    assert_eq!(after.insurance, before.insurance);
                    assert_eq!(
                        after.insurance_domain_budget,
                        before.insurance_domain_budget
                    );
                    if !policy_updated {
                        assert!(current[0]);
                        assert!(
                            health_cert(&env.portfolio_state(target)).certified_liq_deficit > 0
                        );
                        assert!(price > MARK && MARK > report_price);
                        let accounts = portfolios.map(|key| env.svm.get_account(&key));
                        let profile_before = state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap();
                        let sequence = env.control_sequences(0).liquidation_fee;
                        peak_policy =
                            peak_policy.max(env.update_liquidation_fee_policy_with_cu(SHARES[1]));
                        assert_eq!(env.control_sequences(0).liquidation_fee, sequence + 1);
                        let (cfg, updated) = env.market_state();
                        assert_eq!(cfg.liquidation_cranker_fee_share_bps, SHARES[1]);
                        assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), accounts);
                        assert_eq!(updated.insurance, after.insurance);
                        assert_eq!(
                            updated.insurance_domain_budget,
                            after.insurance_domain_budget
                        );
                        assert_eq!(updated.assets[0].effective_price, price);
                        assert_eq!(
                            updated.assets[0].oi_eff_long_q,
                            after.assets[0].oi_eff_long_q
                        );
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &env.svm.get_account(&env.market).unwrap().data,
                                0,
                            )
                            .unwrap(),
                            profile_before
                        );
                        assert_eq!(env.token_amount(tokens[4]) as u128, paid);
                        policy_updated = true;
                    }
                    continue;
                }
                assert!(phase < 2 && policy_updated);
                assert!(before_cert.certified_liq_deficit > 0 && current[0]);
                assert_eq!(before.assets[0].effective_price, price);
                assert_eq!(
                    health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                    0
                );
                let penalty = fee(closed, price, 5);
                let reward = penalty * u128::from(SHARES[phase]) / 10_000;
                assert!(closed > 0 && closed < before.assets[0].oi_eff_long_q && reward > 0);
                for wrong_price in [ENTRY, MARK, report_price, ACCEPTED_PRINT, 900_000] {
                    assert_ne!(penalty, fee(closed, wrong_price, 5));
                }
                if phase == 1 {
                    assert_ne!(reward, penalty * u128::from(SHARES[0]) / 10_000);
                }
                let mut expected = before_values;
                expected[0] -= penalty as i128;
                expected[4] += reward as i128;
                assert_eq!(values(&env, portfolios), expected);
                total_fee += penalty;
                total_reward += reward;
                budgets[0] += (penalty - reward) / 2;
                budgets[1] += (penalty - reward).div_ceil(2);
                episodes.push((closed, penalty, reward));
                liquidated = true;
                break;
            }
            assert_eq!(
                liquidated,
                phase < 2,
                "two rewarded episodes, no catchup bonus"
            );
            // Settle the absent peers' ADL/source work before advancing the price again.
            for _ in 0..8 {
                for i in [1, 2, 3, 0] {
                    if !census(&env, portfolios)[i] {
                        let ix =
                            observe(&env, portfolios[i], owners[4].pubkey(), Some(report), None);
                        peak_crank =
                            peak_crank.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
                    }
                }
                if census(&env, portfolios)[..4].iter().all(|current| *current) {
                    break;
                }
            }
            assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
            let group = env.market_state().1;
            assert_eq!(group.insurance, discovery + total_fee - total_reward);
            assert_eq!(&group.insurance_domain_budget[..2], &budgets);
            assert!(group.insurance_domain_budget[2..]
                .iter()
                .all(|value| *value == 0));
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                budgets.iter().sum::<u128>()
            );
            assert_eq!(
                group.insurance - group.insurance_domain_budget_remaining_total,
                discovery
            );
            assert_eq!(
                values(&env, portfolios)[4],
                (FUNDS[4] as u128 + total_reward - paid) as i128
            );
            peak_crank = peak_crank.max(submit(
                &mut env,
                &owners[4],
                &[retained],
                &tracked,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
            ));
            if phase == 0 && pay_first {
                peak_payout = peak_payout.max(withdraw_reward(
                    &mut env,
                    &owners[4],
                    keeper,
                    tokens[4],
                    total_reward,
                ));
                paid = total_reward;
                assert_eq!(values(&env, portfolios)[4], FUNDS[4] as i128);
                census(&env, portfolios);
            }
            assert_eq!(env.svm.get_account(&env.mint), mint);
            assert_eq!(env.token_amount(tokens[4]) as u128, paid);
            assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
            assert_eq!(env.market_state().1.vault + paid, supply);
        }
        let remainder = FUNDS[4] as u128 + total_reward - paid;
        peak_payout = peak_payout.max(withdraw_reward(
            &mut env, &owners[4], keeper, tokens[4], remainder,
        ));
        paid += remainder;
        assert_eq!(paid, FUNDS[4] as u128 + total_reward);
        assert_eq!(values(&env, portfolios)[4], 0);
        census(&env, portfolios);
        let group = env.market_state().1;
        // Fractional positions remain open. Their rounded claims need not exhaust
        // custody; the full endpoint, including that residual, must agree below.
        let claims = u128::try_from(values(&env, portfolios).iter().sum::<i128>()).unwrap();
        let residual = group.vault.checked_sub(claims + group.insurance).unwrap();
        assert_eq!(group.vault + paid, supply);
        assert_eq!(
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q
        );
        assert_eq!(env.svm.get_account(&env.mint), mint);
        println!("policy catchup pay_first={pay_first}: episodes={episodes:?}, payout={paid}, budgets={budgets:?}, residual={residual}");
        let outcome = (
            episodes,
            paid,
            values(&env, portfolios),
            group.insurance,
            group.vault,
            budgets,
            residual,
        );
        if let Some(expected) = &reference {
            assert_eq!(
                &outcome, expected,
                "receipt timing cannot change policy or price attribution"
            );
        } else {
            reference = Some(outcome);
        }
    }
    assert_cu_within("reward policy succession", peak_policy, CRANK_CU_LIMIT);
    println!("row422 policy succession: 2 histories, 4 liquidations, 6 exact healthy rollbacks; crank={peak_crank}, policy={peak_policy}, payout={peak_payout} CU");
}
