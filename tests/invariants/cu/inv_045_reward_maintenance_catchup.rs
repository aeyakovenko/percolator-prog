//! INV-045/024/036/041, row422: earned Hybrid rewards subsequently fund the
//! recipient's own maintenance. Asset-0 maintenance budgets and asset-1
//! liquidation budgets must remain distinct across fee collection order.
//! Public construction and instructions only; finite conformance, row stays OPEN.

use super::*;

#[path = "inv_045_clipped_reward_maintenance.rs"]
mod clipped_reward_maintenance;

const RATE: u128 = 160;
const SHARE: u128 = 3_333;

fn observe(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    report: Pubkey,
    keeper: Option<Pubkey>,
) -> Instruction {
    let mut ix = super::observe(env, target, signer, Some(report), keeper);
    ix.data = ProgInstruction::PermissionlessCrank {
        now_slot: u64::MAX,
        observations: crank_observations_with_accounts(1, 1),
    }
    .encode();
    ix
}

fn treasury(
    env: &V16CuEnv,
    portfolios: [Pubkey; 5],
    discovery: u128,
    fees: u128,
    rewards: u128,
    liquidation_budgets: [u128; 2],
) {
    let group = env.market_state().1;
    let maintenance = portfolios
        .iter()
        .map(|key| {
            let account = env.portfolio_state(*key);
            assert_eq!(account.fee_credits.get(), 0);
            RATE * u128::from(account.last_fee_slot.get() - 1)
        })
        .sum::<u128>();
    assert_eq!(group.insurance, discovery + maintenance + fees - rewards);
    assert_eq!(
        &group.insurance_domain_budget[..2],
        &[maintenance / 2, maintenance / 2]
    );
    assert_eq!(&group.insurance_domain_budget[2..4], &liquidation_budgets);
    assert!(group.insurance_domain_budget[4..]
        .iter()
        .all(|amount| *amount == 0));
    assert_eq!(
        group.insurance_domain_budget_remaining_total,
        maintenance + fees - rewards
    );
    assert_eq!(
        group.insurance - group.insurance_domain_budget_remaining_total,
        discovery
    );
    census(env, portfolios);
}

fn collect_keeper(env: &mut V16CuEnv, portfolios: [Pubkey; 5], slot: u64) -> u64 {
    let keeper = portfolios[4];
    let before = env.market_state().1;
    let account = env.portfolio_state(keeper);
    let charge = RATE * u128::from(slot - account.last_fee_slot.get());
    assert!(charge > 0 && charge < account.capital.get());
    let peers = portfolios[..4]
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    env.svm.expire_blockhash();
    let cu = env.sync_maintenance_fee_with_cu(keeper, None, u64::MAX);
    let after = env.market_state().1;
    let collected = env.portfolio_state(keeper);
    assert_eq!(collected.last_fee_slot.get(), slot);
    assert_eq!(collected.capital.get(), account.capital.get() - charge);
    assert_eq!(collected.pnl.get(), 0);
    assert_eq!(after.insurance, before.insurance + charge);
    for domain in 0..4 {
        assert_eq!(
            after.insurance_domain_budget[domain],
            before.insurance_domain_budget[domain] + if domain < 2 { charge / 2 } else { 0 }
        );
    }
    assert_eq!(after.assets[1], before.assets[1]);
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap(),
        profile
    );
    assert_eq!(
        portfolios[..4]
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        peers
    );
    assert_eq!(
        [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
        custody
    );
    assert_cu_within("keeper fee collection during catchup", cu, CUSTODY_CU_LIMIT);
    cu
}

#[test]
fn v16_program_keeper_maintenance_preserves_distinct_reward_budgets_until_catchup() {
    const FINAL: u64 = 980_000;
    let first = ENTRY - ENTRY * 24 / 10_000;
    let second = first - first * 24 / 10_000;
    let mut reference = None;
    let mut peak = 0;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    for collect_first in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                maintenance_fee_per_slot: RATE,
                max_abs_funding_e9_per_slot: 0,
                ..production_risk_params()
            },
        );
        set_test_clock(&mut env, 1, 100);
        env.configure_auth_mark_for_asset_as_admin(0, 1, ENTRY);
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feed = [0x76; 32];
        let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            1,
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
        assert_eq!(env.market_state().0.fee_redirect_to_market_0_bps, 0);
        let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
        let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
        let portfolios = funded.map(|pair| pair.0);
        let tokens = funded.map(|pair| pair.1);
        let [target, peer, trader_a, trader_b, keeper] = portfolios;
        env.trade_asset_with_cu(
            1,
            &owners[0],
            target,
            &owners[1],
            peer,
            (100 * POS_SCALE) as i128,
            ENTRY,
            0,
        );
        let mut tracked = vec![env.market, env.mint, env.vault, initial, env.admin.pubkey()];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
        let mint = env.svm.get_account(&env.mint);
        let supply = FUNDS.iter().map(|amount| *amount as u128).sum::<u128>();
        set_test_clock(&mut env, 5, 1_000);
        let advance = observe(&env, trader_a, owners[4].pubkey(), initial, None);
        submit_with_cu_limit(&mut env, &owners[4], &[advance], &tracked, None, 500_000);
        env.trade_asset_with_cu(
            1,
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
        assert_eq!(env.market_state().1.assets[1].effective_price, ENTRY);
        assert_eq!(env.market_state().1.assets[1].raw_oracle_target_price, MARK);
        assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), 1);
        treasury(&env, portfolios, discovery, 0, 0, [0; 2]);
        let mut fees = 0;
        let mut rewards = 0;
        let mut budgets = [0; 2];
        let mut episodes = Vec::new();
        for (phase, slot, raw, price) in [
            (0, 6, MARK, first),
            (1, 7, FINAL, second),
            (2, 14, FINAL, FINAL),
        ] {
            let now = 995 + slot as i64;
            set_test_clock(&mut env, slot, now);
            let report = env.set_pyth_price_with_conf(&feed, raw as i64, -6, 0, now);
            tracked.push(report);
            if collect_first {
                peak = peak.max(collect_keeper(&mut env, portfolios, slot));
                treasury(&env, portfolios, discovery, fees, rewards, budgets);
            }
            let valid = observe(&env, target, owners[4].pubkey(), report, Some(keeper));
            let mut rewarded = false;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_target = env.portfolio_state(target);
                if phase == 2
                    && before.assets[1].effective_price == price
                    && census(&env, portfolios)[0]
                    && health_cert(&before_target).certified_liq_deficit == 0
                {
                    break;
                }
                let recipient = env.portfolio_state(keeper);
                let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
                peak = peak.max(submit_with_cu_limit(
                    &mut env,
                    &owners[4],
                    &[valid.clone()],
                    &tracked,
                    None,
                    500_000,
                ));
                let after = env.market_state().1;
                assert_eq!(after.assets[1].effective_price, price);
                assert_eq!(after.assets[1].raw_oracle_target_price, raw);
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    1,
                )
                .unwrap();
                assert_eq!(profile.oracle_target_publish_time, now);
                assert_eq!(profile.last_good_oracle_slot, slot);
                assert_eq!(
                    [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                    peers
                );
                assert_eq!(
                    [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                    custody
                );
                let closed = before.assets[1].oi_eff_long_q - after.assets[1].oi_eff_long_q;
                let penalty = fee(closed, price, 5);
                let reward = penalty * SHARE / 10_000;
                let received = env.portfolio_state(keeper);
                let mut expected_recipient = recipient;
                expected_recipient.capital =
                    percolator::V16PodU128::new(recipient.capital.get() + reward);
                if reward != 0 {
                    expected_recipient.health_cert.valid = 0;
                }
                assert_eq!(
                    received, expected_recipient,
                    "reward preserves the recipient's own fee cursor and liabilities"
                );
                if closed != 0 {
                    assert!(phase < 2 && reward > 0 && closed < 100 * POS_SCALE);
                    assert!(health_cert(&before_target).certified_liq_deficit > 0);
                    assert_eq!(
                        health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                        0
                    );
                    for wrong in [ENTRY, MARK, raw, ACCEPTED_PRINT, 900_000] {
                        assert_ne!(penalty, fee(closed, wrong, 5));
                    }
                    assert_eq!(
                        env.portfolio_state(target).capital.get() as i128
                            + env.portfolio_state(target).pnl.get(),
                        before_target.capital.get() as i128 + before_target.pnl.get()
                            - penalty as i128
                    );
                    fees += penalty;
                    rewards += reward;
                    budgets[0] += (penalty - reward) / 2;
                    budgets[1] += (penalty - reward).div_ceil(2);
                    episodes.push((closed, penalty, reward));
                    rewarded = true;
                    liquidations += 1;
                }
                treasury(&env, portfolios, discovery, fees, rewards, budgets);
                if rewarded {
                    break;
                }
            }
            assert_eq!(
                rewarded,
                phase < 2,
                "full catchup creates no additional reward"
            );
            if !collect_first && phase < 2 {
                peak = peak.max(collect_keeper(&mut env, portfolios, slot));
            }
            for _ in 0..8 {
                for i in [1, 2, 3, 0] {
                    if !census(&env, portfolios)[i] {
                        let refresh =
                            observe(&env, portfolios[i], owners[4].pubkey(), report, None);
                        peak = peak.max(submit_with_cu_limit(
                            &mut env,
                            &owners[4],
                            &[refresh],
                            &tracked,
                            None,
                            500_000,
                        ));
                        treasury(&env, portfolios, discovery, fees, rewards, budgets);
                    }
                }
                if census(&env, portfolios)[..4].iter().all(|current| *current) {
                    break;
                }
            }
            assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
            for (i, key) in portfolios.iter().enumerate() {
                let fee_slot = if i == 4 && phase == 2 && !collect_first {
                    7
                } else {
                    slot
                };
                assert_eq!(env.portfolio_state(*key).last_fee_slot.get(), fee_slot);
            }
            let keeper_fee_slot = env.portfolio_state(keeper).last_fee_slot.get();
            assert_eq!(
                env.portfolio_state(keeper).capital.get(),
                FUNDS[4] as u128 + rewards - RATE * u128::from(keeper_fee_slot - 1)
            );
            treasury(&env, portfolios, discovery, fees, rewards, budgets);
            peak = peak.max(submit_with_cu_limit(
                &mut env,
                &owners[4],
                &[valid],
                &tracked,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
                500_000,
            ));
            rollbacks += 1;
        }
        let keeper_fees = 13 * RATE;
        assert!(keeper_fees > FUNDS[4] as u128 && rewards > keeper_fees);
        let payout = FUNDS[4] as u128 + rewards - keeper_fees;
        let pending_fee = if collect_first { 0 } else { 7 * RATE };
        assert_eq!(
            env.portfolio_state(keeper).capital.get(),
            payout + pending_fee
        );
        let before_payout = env.market_state().1;
        let peer_accounts = portfolios[..4]
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
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
        peak = peak.max(submit_with_cu_limit(
            &mut env,
            &owners[4],
            &[withdraw.clone(), invalid_suffix],
            &tracked,
            Some((3, InstructionError::InvalidInstructionData)),
            500_000,
        ));
        rollbacks += 1;
        peak = peak.max(submit_with_cu_limit(
            &mut env,
            &owners[4],
            &[withdraw],
            &tracked,
            None,
            500_000,
        ));
        assert_eq!(env.token_amount(tokens[4]) as u128, payout);
        assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
        assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
        assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), 14);
        assert_eq!(
            portfolios[..4]
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            peer_accounts
        );
        assert_eq!(env.svm.get_account(&env.mint), mint);
        treasury(&env, portfolios, discovery, fees, rewards, budgets);
        let group = env.market_state().1;
        assert_eq!(group.insurance, before_payout.insurance + pending_fee);
        assert_eq!(group.vault, env.token_amount(env.vault) as u128);
        assert_eq!(group.vault + payout, supply);
        let claims = u128::try_from(values(&env, portfolios).iter().sum::<i128>()).unwrap();
        let residual = group.vault.checked_sub(claims + group.insurance).unwrap();
        let accounts = portfolios.map(|key| {
            let account = env.portfolio_state(key);
            (
                account.capital.get(),
                account.pnl.get(),
                account.last_fee_slot.get(),
                account.legs,
            )
        });
        let outcome = (
            episodes,
            accounts,
            payout,
            group.insurance,
            group.vault,
            group.insurance_domain_budget,
            residual,
        );
        println!("row422 collect_first={collect_first}: rewards={rewards}, keeper_fees={keeper_fees}, pending_payout_fee={pending_fee}, payout={payout}, liquidation_budgets={budgets:?}, residual={residual}");
        if let Some(expected) = &reference {
            assert_eq!(&outcome, expected, "maintenance order preserves every owner's entitlement and distinct fee destinations");
        } else {
            reference = Some(outcome);
        }
    }
    assert_eq!((liquidations, rollbacks), (4, 8));
    println!("row422 maintenance: two histories, four liquidations, eight exact rollbacks, peak CU={peak}");
}
