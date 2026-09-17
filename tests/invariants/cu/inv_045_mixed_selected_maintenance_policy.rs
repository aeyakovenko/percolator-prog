//! Row422 / INV-020/024/036/041/045/061/062: maintenance receipts cross
//! policy succession before mixed selected-asset liquidation and keeper payout.
//! Public System/SPL/wrapper state; Clock and external reports are fixtures.

use super::*;

const DEPOSITS: [u64; 5] = [10_200_000, 100_000_000, 10_000_000, 10_000_000, 1_000];
const RATE: u128 = 7;
const LIQ_SHARE: u128 = 3_333;

#[derive(Default)]
struct Maintenance {
    fees: [u128; 5],
    receipts: [u128; 5],
    domains: [u128; 2],
}

impl Maintenance {
    fn record(&mut self, actor: usize, charge: u128, share: u128) -> u128 {
        let reward = charge * share / 10_000;
        if share != 0 {
            assert!(reward > 0 && charge * share % 10_000 != 0);
        }
        self.fees[actor] += charge;
        self.receipts[4] += reward;
        self.domains[0] += (charge - reward) / 2;
        self.domains[1] += (charge - reward).div_ceil(2);
        reward
    }

    fn check(
        &self,
        env: &V16CuEnv,
        discovery: u128,
        selected: usize,
        penalty: u128,
        reward: u128,
        eligible: bool,
    ) {
        let group = env.market_state().1;
        let retained = self.fees.iter().sum::<u128>() - self.receipts.iter().sum::<u128>();
        assert_eq!(group.insurance, discovery + retained + penalty - reward);
        let mut domains = vec![0; group.insurance_domain_budget.len()];
        domains[..2].copy_from_slice(&self.domains);
        if eligible {
            domains[2 * selected] += (penalty - reward) / 2;
            domains[2 * selected + 1] += (penalty - reward).div_ceil(2);
        }
        assert_eq!(group.insurance_domain_budget, domains);
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            domains.iter().sum::<u128>()
        );
        assert_eq!(
            group.insurance - group.insurance_domain_budget_remaining_total,
            discovery + if eligible { 0 } else { penalty }
        );
    }
}

fn sync(env: &V16CuEnv, target: Pubkey, recipient: Option<Pubkey>) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    accounts.extend(recipient.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
    }
}

fn submit_sync(env: &mut V16CuEnv, ix: Instruction) -> u64 {
    env.svm.expire_blockhash();
    send_raw_tx(&mut env.svm, &env.payer, ix, &[]).expect("permissionless maintenance sync")
}

#[test]
fn v16_program_mixed_selected_liquidation_preserves_maintenance_policy_receipts_and_payout() {
    let mut peak = [0; 4]; // progress/rollback, maintenance, policy, withdrawal/rollback
    let mut worlds = 0;
    let mut rollbacks = 0;
    for elapsed in [1u64, 4] {
        let slot = 5 + elapsed;
        let price = (ENTRY - elapsed * 2_400).max(MARK);
        for selected in [0usize, 1] {
            let eligible = selected == 1 || elapsed == 4;
            let mut reference = None;
            for collect_first in [false, true] {
                let label =
                    format!("elapsed={elapsed} selected={selected} collect_first={collect_first}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        max_abs_funding_e9_per_slot: 0,
                        maintenance_fee_per_slot: RATE,
                        ..production_risk_params()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.configure_auth_mark_for_asset_as_admin(1, 1, ENTRY);
                env.update_liquidation_fee_policy_with_cu(LIQ_SHARE as u16);
                env.update_maintenance_fee_policy_with_cu(3_333);
                let feed = [0x79; 32];
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
                for asset in [selected, selected ^ 1] {
                    peak[0] = peak[0].max(env.trade_asset_with_cu(
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
                let mut tracked =
                    vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                let mut maintenance = Maintenance::default();
                set_test_clock(&mut env, 5, 1_000);
                let advance = observation(&env, keeper, owners[4].pubkey(), initial, None, false);
                peak[0] = peak[0].max(submit(&mut env, &owners[4], &[advance], &tracked, None));
                for actor in 0..2 {
                    let ix = sync(&env, portfolios[actor], None);
                    peak[1] = peak[1].max(submit_sync(&mut env, ix));
                    maintenance.record(actor, 4 * RATE, 0);
                    assert_eq!(
                        env.portfolio_state(portfolios[actor]).last_fee_slot.get(),
                        5
                    );
                }
                // Both syncs are retained under the old policy. The second executes
                // after succession, while both receipts belong to the same keeper.
                let retained = [
                    sync(&env, trader_a, Some(keeper)),
                    sync(&env, trader_b, Some(keeper)),
                ];
                for (phase, actor) in [2usize, 3].into_iter().enumerate() {
                    if phase == 1 {
                        let accounts = portfolios.map(|key| env.svm.get_account(&key));
                        let before = env.market_state().1;
                        let profile = state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap();
                        let seq = env.control_sequences(0).maintenance_fee;
                        peak[2] = peak[2].max(env.update_maintenance_fee_policy_with_cu(6_667));
                        assert_eq!(env.control_sequences(0).maintenance_fee, seq + 1);
                        assert_eq!(
                            env.market_state().0.maintenance_cranker_fee_share_bps,
                            6_667
                        );
                        assert_eq!(env.market_state().1, before);
                        assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), accounts);
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &env.svm.get_account(&env.market).unwrap().data,
                                0
                            )
                            .unwrap(),
                            profile
                        );
                    }
                    let before = values(&env, portfolios);
                    let charge = 4 * RATE;
                    let reward = maintenance.record(actor, charge, [3_333, 6_667][phase]);
                    peak[1] = peak[1].max(submit_sync(&mut env, retained[phase].clone()));
                    let mut expected = before;
                    expected[actor] -= charge as i128;
                    expected[4] += reward as i128;
                    assert_eq!(values(&env, portfolios), expected);
                    assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), 1);
                    assert_eq!(
                        env.portfolio_state(portfolios[actor]).last_fee_slot.get(),
                        5
                    );
                    maintenance.check(&env, 0, selected, 0, 0, eligible);
                }
                assert_eq!(maintenance.receipts[4], 27);
                peak[0] = peak[0].max(env.trade_asset_with_cu(
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
                maintenance.check(&env, discovery, selected, 0, 0, eligible);
                peak[0] = peak[0].max(env.push_auth_mark_for_asset_as_admin(1, 5, MARK));
                // Publish through the trader, leaving the keeper's later fee debt intact.
                let report = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, 1_000);
                tracked.push(report);
                let publish = observation(&env, trader_a, owners[4].pubkey(), report, None, false);
                peak[0] = peak[0].max(submit(&mut env, &owners[4], &[publish], &tracked, None));
                set_test_clock(&mut env, slot, 1_000 + elapsed as i64);
                let fresh =
                    env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, 1_000 + elapsed as i64);
                let conflict = env.set_pyth_price_with_conf(
                    &feed,
                    MARK as i64 + 1,
                    -6,
                    0,
                    1_000 + elapsed as i64,
                );
                tracked.extend([fresh, conflict]);
                if collect_first {
                    let ix = sync(&env, keeper, None);
                    peak[1] = peak[1].max(submit_sync(&mut env, ix));
                    maintenance.record(4, RATE * u128::from(slot - 1), 0);
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
                    peak[0] = peak[0].max(submit(
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
                    peak[0] = peak[0].max(submit(
                        &mut env,
                        &owners[4],
                        &[valid.clone()],
                        &tracked,
                        None,
                    ));
                    let after = env.market_state().1;
                    if maintenance.fees[0] == 4 * RATE {
                        assert_eq!(env.portfolio_state(target).last_fee_slot.get(), slot);
                        maintenance.record(0, RATE * u128::from(elapsed), 0);
                    }
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
                    assert_eq!(profile.last_good_oracle_slot, slot);
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
                    let penalty = fee(closed, price, 5);
                    let reward = if eligible {
                        penalty * LIQ_SHARE / 10_000
                    } else {
                        0
                    };
                    let mut expected_recipient = recipient;
                    expected_recipient.capital =
                        percolator::V16PodU128::new(recipient.capital.get() + reward);
                    if reward != 0 {
                        expected_recipient.health_cert.valid = 0;
                    }
                    assert_eq!(
                        env.portfolio_state(keeper),
                        expected_recipient,
                        "liquidation cannot reprice maintenance receipts or forgive keeper fees"
                    );
                    maintenance.check(&env, discovery, selected, penalty, reward, eligible);
                    if closed != 0 {
                        assert!(closed < 100 * POS_SCALE && penalty * LIQ_SHARE / 10_000 > 0);
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
                    liquidation.expect("bounded mixed selected liquidation with maintenance");
                for actor in 1..4 {
                    let ix = observation(
                        &env,
                        portfolios[actor],
                        owners[4].pubkey(),
                        fresh,
                        None,
                        false,
                    );
                    peak[0] = peak[0].max(submit(&mut env, &owners[4], &[ix], &tracked, None));
                    assert_eq!(
                        env.portfolio_state(portfolios[actor]).last_fee_slot.get(),
                        slot
                    );
                    maintenance.record(actor, RATE * u128::from(elapsed), 0);
                }
                let loss = i128::from(ENTRY - price);
                let mut expected = DEPOSITS.map(i128::from);
                expected[0] -= 200 * loss + penalty as i128;
                expected[1] += 200 * loss;
                expected[2] -= loss + (discovery / 2) as i128;
                expected[3] += loss - (discovery / 2) as i128;
                expected[4] += reward as i128;
                for actor in 0..5 {
                    expected[actor] +=
                        maintenance.receipts[actor] as i128 - maintenance.fees[actor] as i128;
                }
                assert_eq!(values(&env, portfolios), expected, "{label}");
                let keeper_fee = RATE * u128::from(slot - 1);
                let payout =
                    u128::from(DEPOSITS[4]) + maintenance.receipts[4] + reward - keeper_fee;
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
                    data: env.withdraw_ix(keeper, payout).encode(),
                };
                let invalid_suffix = Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![255],
                };
                peak[3] = peak[3].max(submit(
                    &mut env,
                    &owners[4],
                    &[withdraw.clone(), invalid_suffix],
                    &tracked,
                    Some((3, InstructionError::InvalidInstructionData)),
                ));
                rollbacks += 1;
                let peers = portfolios[..4]
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>();
                peak[3] = peak[3].max(submit(&mut env, &owners[4], &[withdraw], &tracked, None));
                if !collect_first {
                    maintenance.record(4, keeper_fee, 0);
                }
                expected[4] = 0;
                assert_eq!(values(&env, portfolios), expected);
                assert_eq!(
                    portfolios[..4]
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    peers
                );
                assert_eq!(env.token_amount(tokens[4]) as u128, payout);
                assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
                assert_eq!(maintenance.fees, [keeper_fee; 5]);
                assert!(portfolios
                    .iter()
                    .all(|key| env.portfolio_state(*key).last_fee_slot.get() == slot));
                maintenance.check(&env, discovery, selected, penalty, reward, eligible);
                let group = env.market_state().1;
                assert_eq!(
                    group.vault + payout,
                    DEPOSITS.iter().map(|v| u128::from(*v)).sum::<u128>()
                );
                assert_eq!(group.vault, env.token_amount(env.vault) as u128);
                assert_eq!(
                    expected.iter().sum::<i128>() + group.insurance as i128,
                    group.vault as i128
                );
                assert_eq!(env.svm.get_account(&env.mint), custody[0]);
                census(&env, portfolios);
                let accounts = portfolios.map(|key| {
                    let p = env.portfolio_state(key);
                    (p.capital.get(), p.pnl.get(), p.last_fee_slot.get(), p.legs)
                });
                let outcome = (
                    closed,
                    penalty,
                    reward,
                    payout,
                    accounts,
                    group.insurance_domain_budget,
                    group.insurance,
                    group.vault,
                );
                if let Some(reference) = &reference {
                    assert_eq!(&outcome, reference, "{label}");
                } else {
                    reference = Some(outcome);
                }
                eprintln!("mixed maintenance {label}: penalty={penalty} liquidation_reward={reward} maintenance_reward={} keeper_fee={keeper_fee} payout={payout}", maintenance.receipts[4]);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks), (8, 24));
    for cu in peak {
        assert_cu_within("mixed selected maintenance policy", cu, 650_000);
    }
    eprintln!("mixed selected maintenance policy: worlds={worlds} exact_rollbacks={rollbacks} peak_cu={peak:?}");
}
