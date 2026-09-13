//! INV-045/024/036/061/066, row422: earned Hybrid rewards through full catchup
//! and resolved redemption of the entire five-portfolio cohort. Finite public
//! conformance only; retained versus early receipts cross both redemption orders.
//! System/SPL/ATA/wrapper construction; only Clock, external Pyth reports, signer
//! SOL and program loading are harness inputs. No protocol-account mutation.

use super::*;

const SHARE: u128 = 3_333;
const FINAL: u64 = 980_000;
const ACTION_CU: u64 = 500_000;

fn checked_submit(
    env: &mut V16CuEnv,
    signer: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
) -> u64 {
    let mut keys = tracked.to_vec();
    keys.push(env.payer.pubkey());
    keys.extend(
        instructions
            .iter()
            .flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey)),
    );
    keys.sort_unstable();
    keys.dedup();
    let lamports: Vec<_> = keys
        .iter()
        .map(|key| env.svm.get_account(key).map(|account| account.lamports))
        .collect();
    let succeeds = rejection.is_none();
    let cu = submit_with_cu_limit(env, signer, instructions, tracked, rejection, ACTION_CU);
    if succeeds {
        for (key, before) in keys.iter().zip(lamports) {
            let after = env.svm.get_account(key).map(|account| account.lamports);
            let expected = if *key == env.payer.pubkey() {
                before.map(|amount| amount - 2 * FeeStructure::default().lamports_per_signature)
            } else {
                before
            };
            assert_eq!(
                after, expected,
                "SPL progress preserves rent and signer SOL: {key}"
            );
        }
    }
    cu
}

fn treasury(env: &V16CuEnv, discovery: u128, fees: u128, rewards: u128, budgets: [u128; 2]) {
    let group = env.market_state().1;
    assert_eq!(group.insurance, discovery + fees - rewards);
    assert_eq!(&group.insurance_domain_budget[..2], &budgets);
    assert!(group.insurance_domain_budget[2..]
        .iter()
        .all(|value| *value == 0));
    assert_eq!(
        group.insurance_domain_budget_remaining_total,
        fees - rewards
    );
    assert_eq!(
        group.insurance - group.insurance_domain_budget_remaining_total,
        discovery
    );
}

fn custody(env: &V16CuEnv, tokens: [Pubkey; 5], supply: u128) -> [u128; 5] {
    let paid = tokens.map(|key| u128::from(env.token_amount(key)));
    let group = env.market_state().1;
    assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
    assert_eq!(group.vault + paid.iter().sum::<u128>(), supply);
    paid
}

#[test]
fn v16_program_hybrid_catchup_rewards_survive_resolved_cohort_redemption_order() {
    let first = ENTRY - ENTRY * 24 / 10_000;
    let second = first - first * 24 / 10_000;
    let supply = FUNDS.iter().map(|amount| u128::from(*amount)).sum::<u128>();
    let mut reference = None;
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut waiting_rollbacks = 0;
    let mut liquidations = 0;
    let mut redemptions = 0;
    for pay_early in [false, true] {
        for reverse in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            peak = peak.max(env.update_liquidation_fee_policy_with_cu(SHARE as u16));
            let feed = [0x7c; 32];
            let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
            peak = peak.max(
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
                .unwrap(),
            );
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            peak = peak.max(env.trade_asset_with_cu(
                0,
                &owners[0],
                target,
                &owners[1],
                peer,
                (100 * POS_SCALE) as i128,
                ENTRY,
                0,
            ));
            let mut tracked = vec![env.market, env.mint, env.vault, initial, env.admin.pubkey()];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let mint = env.svm.get_account(&env.mint);
            let mut providers = vec![(initial, env.svm.get_account(&initial))];
            set_test_clock(&mut env, 5, 1_000);
            let advance = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
            peak = peak.max(checked_submit(
                &mut env,
                &owners[4],
                &[advance],
                &tracked,
                None,
            ));
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
            let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
            let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
            let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
            assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
            assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
            let mut fees = 0;
            let mut rewards = 0;
            let mut budgets = [0; 2];
            let mut early_paid = 0;
            let mut episodes = Vec::new();
            treasury(&env, discovery, fees, rewards, budgets);
            census(&env, portfolios);
            for (phase, slot, raw, price) in [
                (0, 6, MARK, first),
                (1, 7, FINAL, second),
                (2, 14, FINAL, FINAL),
            ] {
                let now = 995 + slot as i64;
                set_test_clock(&mut env, slot, now);
                let report = env.set_pyth_price_with_conf(&feed, raw as i64, -6, 0, now);
                tracked.push(report);
                providers.push((report, env.svm.get_account(&report)));
                let valid = observe(&env, target, owners[4].pubkey(), Some(report), Some(keeper));
                let mut rewarded = false;
                for _ in 0..6 {
                    let before = env.market_state().1;
                    let before_values = values(&env, portfolios);
                    let certificate = health_cert(&env.portfolio_state(target));
                    if phase == 2
                        && before.assets[0].effective_price == price
                        && census(&env, portfolios)[0]
                        && certificate.certified_liq_deficit == 0
                    {
                        break;
                    }
                    let recipient = env.portfolio_state(keeper);
                    let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                    let vault = env.svm.get_account(&env.vault);
                    peak = peak.max(checked_submit(
                        &mut env,
                        &owners[4],
                        &[valid.clone()],
                        &tracked,
                        None,
                    ));
                    let after = env.market_state().1;
                    assert_eq!(after.assets[0].effective_price, price);
                    assert_eq!(after.assets[0].raw_oracle_target_price, raw);
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    assert_eq!(profile.oracle_target_publish_time, now);
                    assert_eq!(profile.last_good_oracle_slot, slot);
                    assert_eq!(
                        [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                        peers
                    );
                    assert_eq!(env.svm.get_account(&env.vault), vault);
                    let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                    let penalty = fee(closed, price, 5);
                    let reward = penalty * SHARE / 10_000;
                    let mut expected_recipient = recipient;
                    expected_recipient.capital =
                        percolator::V16PodU128::new(recipient.capital.get() + reward);
                    if reward != 0 {
                        expected_recipient.health_cert.valid = 0;
                    }
                    assert_eq!(env.portfolio_state(keeper), expected_recipient);
                    if closed != 0 {
                        assert!(phase < 2 && reward > 0 && closed < 100 * POS_SCALE);
                        assert!(certificate.certified_liq_deficit > 0);
                        assert_eq!(before.assets[0].effective_price, price);
                        for wrong in [ENTRY, MARK, raw, ACCEPTED_PRINT, 900_000] {
                            assert_ne!(penalty, fee(closed, wrong, 5));
                        }
                        let mut expected_values = before_values;
                        expected_values[0] -= penalty as i128;
                        expected_values[4] += reward as i128;
                        assert_eq!(values(&env, portfolios), expected_values);
                        fees += penalty;
                        rewards += reward;
                        budgets[0] += (penalty - reward) / 2;
                        budgets[1] += (penalty - reward).div_ceil(2);
                        episodes.push((closed, penalty, reward));
                        liquidations += 1;
                        rewarded = true;
                    }
                    treasury(&env, discovery, fees, rewards, budgets);
                    census(&env, portfolios);
                    if rewarded {
                        break;
                    }
                }
                assert_eq!(rewarded, phase < 2, "full catchup cannot create a bonus");
                for _ in 0..8 {
                    for i in [1, 2, 3, 0] {
                        if !census(&env, portfolios)[i] {
                            let refresh = observe(
                                &env,
                                portfolios[i],
                                owners[4].pubkey(),
                                Some(report),
                                None,
                            );
                            peak = peak.max(checked_submit(
                                &mut env,
                                &owners[4],
                                &[refresh],
                                &tracked,
                                None,
                            ));
                            treasury(&env, discovery, fees, rewards, budgets);
                        }
                    }
                    if census(&env, portfolios)[..4].iter().all(|current| *current) {
                        break;
                    }
                }
                assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
                peak = peak.max(checked_submit(
                    &mut env,
                    &owners[4],
                    &[valid],
                    &tracked,
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                    )),
                ));
                rollbacks += 1;
                if phase == 0 && pay_early {
                    let withdraw = Instruction {
                        program_id: env.program_id,
                        data: env.withdraw_ix(keeper, rewards).encode(),
                        accounts: vec![
                            AccountMeta::new(owners[4].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(keeper, false),
                            AccountMeta::new(tokens[4], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    };
                    peak = peak.max(checked_submit(
                        &mut env,
                        &owners[4],
                        &[withdraw],
                        &tracked,
                        None,
                    ));
                    early_paid = rewards;
                }
                assert_eq!(
                    values(&env, portfolios)[4],
                    (u128::from(FUNDS[4]) + rewards - early_paid) as i128
                );
                assert_eq!(custody(&env, tokens, supply), [0, 0, 0, 0, early_paid]);
                treasury(&env, discovery, fees, rewards, budgets);
            }

            let live_values = values(&env, portfolios);
            println!("terminal catchup early={pay_early} reverse={reverse}: episodes={episodes:?}, values={live_values:?}");
            let live = env.market_state().1;
            let live_residual = live
                .vault
                .checked_sub(
                    u128::try_from(live_values.iter().sum::<i128>()).unwrap() + live.insurance,
                )
                .unwrap();
            let accounts = portfolios.map(|key| env.svm.get_account(&key));
            let resolve = Instruction {
                program_id: env.program_id,
                data: ProgInstruction::ResolveMarket {
                    asset_generation_frontier: live.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
                accounts: vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
            };
            let admin = env.admin.insecure_clone();
            peak = peak.max(checked_submit(&mut env, &admin, &[resolve], &tracked, None));
            assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), accounts);
            let frozen = env.market_state().1;
            let frozen_profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(frozen.mode, MarketModeV16::Resolved);
            assert_eq!(frozen.resolved_slot, 14);
            assert_eq!(frozen.assets[0].effective_price, FINAL);
            assert_eq!(frozen.vault, live.vault);
            treasury(&env, discovery, fees, rewards, budgets);
            // A later external report and Clock cannot reprice a resolved receipt.
            set_test_clock(&mut env, 30, 1_025);
            let later = env.set_pyth_price_with_conf(&feed, 1_200_000, -6, 0, 1_025);
            tracked.push(later);
            providers.push((later, env.svm.get_account(&later)));
            let invalid_suffix = Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![255],
            };
            let mut terminal_rollbacks = [false; 5];
            for round in 0..16 {
                let mut progressed = false;
                for actor in if reverse {
                    [4, 3, 2, 1, 0]
                } else {
                    [0, 1, 2, 3, 4]
                } {
                    if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                        continue;
                    }
                    let close = Instruction {
                        program_id: env.program_id,
                        data: ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                        .encode(),
                        accounts: vec![
                            AccountMeta::new_readonly(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    };
                    let before = env.portfolio_state(portfolios[actor]);
                    // A flat winner waits for other stored position residues to detach.
                    if percolator::active_bitmap_is_empty(active_bitmap(&before))
                        && before.pnl.get() > 0
                        && env.market_state().1.resolved_payout_blocker_count > 0
                    {
                        assert_eq!(before.last_fee_slot.get(), frozen.resolved_slot);
                        assert!(before
                            .source_domains
                            .iter()
                            .all(|source| source.source_claim_liened_num.get() == 0));
                        peak = peak.max(checked_submit(
                            &mut env,
                            &owners[actor],
                            &[close],
                            &tracked,
                            Some((
                                2,
                                InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                            )),
                        ));
                        waiting_rollbacks += 1;
                        continue;
                    }
                    if !terminal_rollbacks[actor] {
                        peak = peak.max(checked_submit(
                            &mut env,
                            &owners[actor],
                            &[close.clone(), invalid_suffix.clone()],
                            &tracked,
                            Some((3, InstructionError::InvalidInstructionData)),
                        ));
                        terminal_rollbacks[actor] = true;
                        rollbacks += 1;
                    }
                    let before_paid = custody(&env, tokens, supply);
                    let peers: Vec<_> = portfolios
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != actor)
                        .map(|(_, key)| (*key, env.svm.get_account(key)))
                        .collect();
                    peak = peak.max(checked_submit(
                        &mut env,
                        &owners[actor],
                        &[close],
                        &tracked,
                        None,
                    ));
                    progressed = true;
                    redemptions += 1;
                    let after_paid = custody(&env, tokens, supply);
                    let after = env.portfolio_state(portfolios[actor]);
                    assert!(after_paid[actor] >= before_paid[actor]);
                    assert!(
                        after != before || after_paid != before_paid,
                        "accepted redemption progresses"
                    );
                    for (key, account) in peers {
                        assert_eq!(env.svm.get_account(&key), account);
                    }
                    for i in 0..5 {
                        if i != actor {
                            assert_eq!(after_paid[i], before_paid[i]);
                        }
                    }
                    if actor == 4 {
                        assert_eq!(after_paid[4], u128::from(FUNDS[4]) + rewards);
                        assert_eq!(after.capital.get(), 0);
                        assert_eq!(after.pnl.get(), 0);
                        assert!(resolved_portfolio_is_terminal(&env, keeper));
                    }
                    let group = env.market_state().1;
                    assert_eq!(group.assets[0].effective_price, FINAL);
                    assert_eq!(group.resolved_slot, 14);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap(),
                        frozen_profile
                    );
                    treasury(&env, discovery, fees, rewards, budgets);
                    census(&env, portfolios);
                }
                if portfolios
                    .iter()
                    .all(|key| resolved_portfolio_is_terminal(&env, *key))
                {
                    break;
                }
                assert!(progressed, "nonterminal cohort stalled in round {round}");
            }
            assert!(terminal_rollbacks.iter().all(|checked| *checked));
            assert!(portfolios
                .iter()
                .all(|key| resolved_portfolio_is_terminal(&env, *key)));
            let paid = custody(&env, tokens, supply);
            let final_group = env.market_state().1;
            assert_eq!((final_group.c_tot, final_group.pnl_pos_tot), (0, 0));
            assert_eq!(
                (
                    final_group.assets[0].oi_eff_long_q,
                    final_group.assets[0].oi_eff_short_q
                ),
                (0, 0)
            );
            assert_eq!(final_group.source_claim_bound_total_num, 0);
            assert_eq!(paid[4], u128::from(FUNDS[4]) + rewards);
            assert_eq!(
                paid.map(|amount| amount as i128),
                std::array::from_fn(
                    |i| live_values[i] + if i == 4 { early_paid as i128 } else { 0 }
                )
            );
            assert_eq!(final_group.vault, final_group.insurance + live_residual);
            assert_eq!(env.svm.get_account(&env.mint), mint);
            for (key, account) in providers {
                assert_eq!(env.svm.get_account(&key), account);
            }
            println!("terminal rewards early={pay_early} reverse={reverse}: episodes={episodes:?}, paid={paid:?}, budgets={budgets:?}, residual={live_residual}");
            let outcome = (
                episodes,
                paid,
                final_group.vault,
                final_group.insurance,
                budgets,
                live_residual,
            );
            if let Some(expected) = &reference {
                assert_eq!(&outcome, expected);
            } else {
                reference = Some(outcome);
            }
        }
    }
    assert_eq!(liquidations, 8);
    assert_eq!(rollbacks, 32);
    assert!(waiting_rollbacks > 0);
    assert_cu_within("Hybrid reward terminal redemption", peak, ACTION_CU);
    println!("row422 terminal redemption: 4 histories, {liquidations} liquidations, {redemptions} progressing closes, {rollbacks} healthy/suffix rollbacks, {waiting_rollbacks} waiting rollbacks; peak={peak} CU");
}
