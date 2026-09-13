//! INV-020/024/036/041/045/061/062: fresh corroboration after paid-mark catchup.
//! Previously retained discovery fees cannot enter the new liquidation-fee split.
//! Cross publication order and shared ownership, including a rolled-back handoff.
//! Fresh-report arrival while the paid mark still lags is outside this sample.

use super::*;

const SHARE: u128 = 3_333;

fn observation(env: &V16CuEnv, owner: &Keypair, account: Pubkey, report: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account, false),
            AccountMeta::new_readonly(report, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: crank_observations_with_accounts(0, 1),
        }
        .encode(),
    }
}

#[test]
fn v16_program_corroborated_paid_mark_only_distributes_new_liquidation_fees() {
    let mut peak_crank = 0;
    let mut peak_trade = 0;
    let mut peak_bundle = 0;
    let mut peak_withdraw = 0;
    let mut reference = None;
    for common_owner in [false, true] {
        for publish_first in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let feed = [0x42; 32];
            let old_report = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[old_report],
                1,
                100,
                0,
                0,
                1,
                0,
            )
            .expect("public Hybrid configuration");
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let actor_owners: [&Keypair; 5] =
                std::array::from_fn(|i| &owners[if common_owner && i >= 2 { 2 } else { i }]);
            let funded =
                std::array::from_fn::<_, 5, _>(|i| fund(&mut env, actor_owners[i], FUNDS[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            assert_eq!(tokens[2] == tokens[4], common_owner);
            let values = |env: &V16CuEnv| {
                portfolios.map(|key| {
                    let account = env.portfolio_state(key);
                    account.capital.get() as i128 + account.pnl.get()
                })
            };
            let profile = |env: &V16CuEnv| {
                state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                    .unwrap()
            };
            let crank = |env: &mut V16CuEnv, account: Pubkey, report: Pubkey, reward: bool| {
                let mut ix = observation(env, actor_owners[4], account, report);
                if reward {
                    ix.accounts.push(AccountMeta::new(keeper, false));
                }
                env.svm.expire_blockhash();
                send_raw_tx(&mut env.svm, &env.payer, ix, &[actor_owners[4]])
            };
            peak_trade = peak_trade.max(env.trade_asset_with_cu(
                0,
                actor_owners[0],
                target,
                actor_owners[1],
                peer,
                (100 * POS_SCALE) as i128,
                ENTRY,
                0,
            ));
            let vault = FUNDS.iter().sum::<u64>();
            assert_eq!(env.token_amount(env.vault), vault);
            let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
            set_test_clock(&mut env, 5, 1_000);
            peak_crank = peak_crank.max(crank(&mut env, keeper, old_report, false).unwrap());
            assert_eq!(profile(&env).mark_ewma_last_slot, 1);
            peak_trade = peak_trade.max(env.trade_asset_with_cu(
                0,
                actor_owners[2],
                trader_a,
                actor_owners[3],
                trader_b,
                POS_SCALE as i128,
                980_000,
                0,
            ));
            // Elapsed discovery accepts 96 bps; alpha=4/5 makes a 7,680-atom mark move.
            // The preexisting 100-unit OI pays the rounded 77-bps externality bilaterally.
            let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
            let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
            let per_trader_fee = fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
            let movement_fee = 2 * per_trader_fee;
            let mut expected = FUNDS.map(i128::from);
            expected[2] -= per_trader_fee as i128;
            expected[3] -= per_trader_fee as i128;
            assert_eq!(values(&env), expected);
            let staged = env.market_state().1;
            assert_eq!(staged.assets[0].effective_price, ENTRY);
            assert_eq!(staged.assets[0].raw_oracle_target_price, MARK);
            assert_eq!(profile(&env).mark_ewma_e6, MARK);
            assert_eq!(staged.insurance, movement_fee);
            assert_eq!(staged.insurance_domain_budget_remaining_total, 0);

            set_test_clock(&mut env, 9, 1_001);
            peak_crank = peak_crank.max(crank(&mut env, keeper, old_report, false).unwrap());
            let caught = env.market_state().1;
            assert_eq!(caught.assets[0].effective_price, MARK);
            assert_eq!(caught.assets[0].raw_oracle_target_price, MARK);
            assert_eq!(caught.assets[0].slot_last, 9);
            assert_eq!(profile(&env).last_good_oracle_slot, 1);
            assert_eq!(profile(&env).oracle_target_publish_time, 100);
            assert_eq!(
                values(&env),
                expected,
                "market-only catchup moves no owner value"
            );
            assert_eq!(caught.insurance, movement_fee);
            assert!(caught
                .insurance_domain_budget
                .iter()
                .all(|amount| *amount == 0));

            let fresh = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, 1_001);
            let publication = observation(&env, actor_owners[4], keeper, fresh);
            let mut invalid = publication.clone();
            let mut duplicate = crank_observations_with_accounts(0, 1);
            duplicate.extend(crank_observations(0));
            invalid.data = ProgInstruction::PermissionlessCrank {
                now_slot: 9,
                observations: duplicate,
            }
            .encode();
            env.svm.expire_blockhash();
            let bundle = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), publication, invalid],
                Some(&env.payer.pubkey()),
                &[&env.payer, actor_owners[4]],
                env.svm.latest_blockhash(),
            );
            bundle.verify().unwrap();
            assert!(
                bincode::serialized_size(&bundle).unwrap()
                    <= solana_sdk::packet::PACKET_DATA_SIZE as u64
            );
            let mut keys = vec![
                env.market,
                env.mint,
                env.vault,
                old_report,
                fresh,
                env.admin.pubkey(),
            ];
            keys.extend(portfolios);
            keys.extend(tokens);
            keys.extend(actor_owners.map(Signer::pubkey));
            keys.extend(bundle.message.account_keys.iter().copied());
            keys.retain(|key| *key != env.payer.pubkey());
            keys.sort_unstable();
            keys.dedup();
            let frame = |env: &V16CuEnv| -> Vec<_> {
                keys.iter().map(|key| env.svm.get_account(key)).collect()
            };
            let before_bundle = frame(&env);
            let failed = env
                .svm
                .send_transaction(bundle)
                .expect_err("reject duplicate suffix");
            assert_eq!(
                failed.err,
                solana_sdk::transaction::TransactionError::InstructionError(
                    3,
                    solana_sdk::instruction::InstructionError::Custom(
                        PercolatorError::InvalidInstruction as u32
                    )
                )
            );
            assert_eq!(
                failed
                    .meta
                    .logs
                    .iter()
                    .filter(|line| { **line == format!("Program {} success", env.program_id) })
                    .count(),
                1,
                "fresh publication completed before the rejecting instruction"
            );
            peak_bundle = peak_bundle.max(failed.meta.compute_units_consumed);
            assert_eq!(
                frame(&env),
                before_bundle,
                "fresh handoff rolls back completely"
            );
            assert_eq!(profile(&env).last_good_oracle_slot, 1);
            assert_eq!(profile(&env).oracle_target_publish_time, 100);

            if publish_first {
                peak_crank = peak_crank.max(crank(&mut env, keeper, fresh, false).unwrap());
                assert_eq!(values(&env), expected);
                assert_eq!(env.market_state().1.insurance, movement_fee);
                assert_eq!(
                    env.market_state().1.insurance_domain_budget,
                    caught.insurance_domain_budget
                );
            }
            let mut liquidation = None;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env);
                let foreign = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                peak_crank = peak_crank.max(crank(&mut env, target, fresh, true).unwrap());
                let after = env.market_state().1;
                assert_eq!(after.assets[0].effective_price, MARK);
                assert_eq!(after.assets[0].raw_oracle_target_price, MARK);
                assert_eq!(profile(&env).oracle_target_price_e6, MARK);
                assert_eq!(profile(&env).mark_ewma_e6, MARK);
                assert_eq!(profile(&env).last_good_oracle_slot, 9);
                assert_eq!(profile(&env).oracle_target_publish_time, 1_001);
                assert_eq!(
                    [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                    foreign
                );
                assert_eq!(
                    [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                    custody
                );
                assert_eq!(after.vault, u128::from(vault));
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert_eq!(values(&env)[4], before_values[4]);
                    assert_eq!(after.insurance, before.insurance);
                    assert_eq!(
                        after.insurance_domain_budget,
                        before.insurance_domain_budget
                    );
                    continue;
                }
                let penalty = fee(closed, MARK, 5);
                let reward = penalty * SHARE / 10_000;
                let retained = penalty - reward;
                assert!(closed > 0 && closed < 100 * POS_SCALE);
                assert!(reward > 0 && reward < penalty && penalty < movement_fee);
                for wrong_price in [ENTRY, ACCEPTED_PRINT, 980_000] {
                    assert_ne!(penalty, fee(closed, wrong_price, 5));
                }
                assert_eq!(before_values[0] - values(&env)[0], penalty as i128);
                assert_eq!(values(&env)[4] - before_values[4], reward as i128);
                assert_eq!(after.insurance, movement_fee + retained);
                assert_eq!(after.insurance_domain_budget_remaining_total, retained);
                assert_eq!(
                    &after.insurance_domain_budget[..2],
                    &[retained / 2, retained.div_ceil(2)]
                );
                assert!(after.insurance_domain_budget[2..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert_eq!(
                    after.assets[0].oi_eff_long_q,
                    after.assets[0].oi_eff_short_q
                );
                assert_eq!(
                    health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                    0
                );
                liquidation = Some((closed, penalty, reward));
                break;
            }
            let (closed, penalty, reward) = liquidation.expect("bounded partial liquidation");
            for account in [peer, trader_a, trader_b] {
                peak_crank = peak_crank.max(crank(&mut env, account, fresh, false).unwrap());
            }
            let pnl = i128::from(ENTRY - MARK);
            expected[0] -= 100 * pnl + penalty as i128;
            expected[1] += 100 * pnl;
            expected[2] -= pnl;
            expected[3] += pnl;
            expected[4] += reward as i128;
            assert_eq!(values(&env), expected);
            let group = env.market_state().1;
            assert_eq!(
                expected.iter().sum::<i128>() + group.insurance as i128,
                vault as i128
            );
            assert_eq!(
                expected[2..].iter().sum::<i128>(),
                FUNDS[2..]
                    .iter()
                    .map(|amount| i128::from(*amount))
                    .sum::<i128>()
                    - movement_fee as i128
                    + reward as i128,
                "common ownership does not rebate discovery costs or transfer peer PnL"
            );
            let mut fixed_point = false;
            for _ in 0..4 {
                let before = frame(&env);
                match crank(&mut env, target, fresh, true) {
                    Ok(cu) => peak_crank = peak_crank.max(cu),
                    Err(error) => {
                        assert!(is_engine_non_progress_error(&error), "{error}");
                        assert_eq!(frame(&env), before);
                        fixed_point = true;
                    }
                }
                assert_eq!(values(&env), expected, "retries cannot pay another reward");
                let retried = env.market_state().1;
                assert_eq!(retried.insurance, group.insurance);
                assert_eq!(
                    retried.insurance_domain_budget,
                    group.insurance_domain_budget
                );
                assert_eq!(
                    retried.assets[0].oi_eff_long_q,
                    group.assets[0].oi_eff_long_q
                );
                assert_eq!(
                    retried.assets[0].oi_eff_short_q,
                    group.assets[0].oi_eff_short_q
                );
                if fixed_point {
                    break;
                }
            }
            assert!(fixed_point, "bounded healthy retry fixed point");
            let payout = u128::from(FUNDS[4]) + reward;
            env.svm.expire_blockhash();
            peak_withdraw = peak_withdraw.max(
                env.send(
                    env.withdraw_ix(keeper, payout),
                    vec![
                        AccountMeta::new(actor_owners[4].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[4], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[actor_owners[4]],
                )
                .expect("keeper withdraws only its principal and new fee share"),
            );
            expected[4] = 0;
            assert_eq!(values(&env), expected);
            for key in tokens {
                assert_eq!(
                    env.token_amount(key),
                    if key == tokens[4] { payout as u64 } else { 0 }
                );
            }
            assert_eq!(env.svm.get_account(&env.mint), custody[0]);
            assert_eq!(env.token_amount(env.vault), vault - payout as u64);
            let final_group = env.market_state().1;
            assert_eq!(final_group.vault, u128::from(vault) - payout);
            assert_eq!(final_group.insurance, group.insurance);
            assert_eq!(
                final_group.insurance_domain_budget,
                group.insurance_domain_budget
            );
            assert_eq!(
                expected.iter().sum::<i128>() + final_group.insurance as i128,
                final_group.vault as i128
            );
            let outcome = (
                closed,
                penalty,
                reward,
                expected,
                final_group.insurance,
                final_group.insurance_domain_budget,
                final_group.vault,
                payout,
            );
            if let Some(reference) = &reference {
                assert_eq!(
                    &outcome, reference,
                    "publication order and owner identity preserve attribution"
                );
            } else {
                reference = Some(outcome);
            }
            println!("common_owner={common_owner} publish_first={publish_first} movement_fee={movement_fee} closed={closed} penalty={penalty} reward={reward}");
        }
    }
    assert_cu_within("corroborated paid-mark crank", peak_crank, CRANK_CU_LIMIT);
    assert_cu_within("paid-mark entry", peak_trade, TRADE_CU_LIMIT);
    assert_cu_within(
        "publication rollback bundle",
        peak_bundle,
        2 * CRANK_CU_LIMIT,
    );
    assert_cu_within(
        "corroborated keeper SPL payout",
        peak_withdraw,
        CUSTODY_CU_LIMIT,
    );
    println!("corroboration worlds=4 crank={peak_crank} trade={peak_trade} bundle={peak_bundle} withdrawal={peak_withdraw}");
}
