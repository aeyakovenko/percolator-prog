//! INV-045 / row 422: old liquidation penalties survive a later fresh-report handoff.
//! A committed stale-mark liquidation precedes fresh liquidation while price still
//! lags. Discovery fees and the old penalty remain outside every new reward/budget.

use super::*;

#[path = "inv_045_paid_origin_routes.rs"]
mod paid_origin_routes;

#[path = "inv_045_paid_origin_hybrid_recipient.rs"]
mod paid_origin_hybrid_recipient;

fn observe_with_optional_recipient_report(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    target_report: Option<Pubkey>,
    recipient_report: Option<Pubkey>,
    reward: Option<Pubkey>,
) -> Instruction {
    let Some(recipient_report) = recipient_report else {
        return observe(env, target, signer, target_report, reward);
    };
    let mut accounts = vec![
        AccountMeta::new(signer, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    if let Some(target_report) = target_report {
        accounts.push(AccountMeta::new_readonly(target_report, false));
        accounts.push(AccountMeta::new_readonly(recipient_report, false));
    }
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                },
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 1,
                },
            ],
        }
        .encode(),
    }
}

fn refresh_with_optional_recipient_report(
    env: &V16CuEnv,
    portfolio: Pubkey,
    signer: Pubkey,
    report: Pubkey,
    recipient_report: Option<Pubkey>,
) -> Instruction {
    if recipient_report.is_none() {
        return paid_origin_routes::refresh(env, portfolio, signer, report);
    }
    observe_with_optional_recipient_report(
        env,
        portfolio,
        signer,
        Some(report),
        recipient_report,
        None,
    )
}

fn run_retained_handoff(
    routes: Option<(bool, bool, bool)>,
) -> ([i128; 5], [u128; 2], [u128; 4], u64) {
    run_retained_handoff_impl(routes, false)
}

pub(super) fn run_retained_handoff_with_hybrid_recipient(
    routes: (bool, bool, bool),
) -> ([i128; 5], [u128; 2], [u128; 4], u64) {
    run_retained_handoff_impl(Some(routes), true)
}

fn run_retained_handoff_impl(
    routes: Option<(bool, bool, bool)>,
    hybrid_recipient: bool,
) -> ([i128; 5], [u128; 2], [u128; 4], u64) {
    const SHARE: u128 = 3_333;
    const FRESH_TARGET: u64 = 980_000;
    let crank_limit = if routes.is_some() {
        500_000
    } else {
        CRANK_CU_LIMIT
    };
    let submit = |env: &mut V16CuEnv,
                  signer: &Keypair,
                  instructions: &[Instruction],
                  tracked: &[Pubkey],
                  rejection: Option<(u8, InstructionError)>| {
        submit_with_cu_limit(env, signer, instructions, tracked, rejection, crank_limit)
    };
    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            max_portfolio_assets: if routes.is_some() { 2 } else { 1 },
            max_abs_funding_e9_per_slot: 0,
            ..production_risk_params()
        },
    );
    set_test_clock(&mut env, 1, 100);
    env.update_liquidation_fee_policy_with_cu(SHARE as u16);
    let feed = [0x6d; 32];
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
    let recipient_feed = [0x6e; 32];
    let recipient_initial = if routes.is_some() && hybrid_recipient {
        let report = env.set_pyth_price_with_conf(&recipient_feed, 100, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            1,
            1,
            0,
            [recipient_feed, [0; 32], [0; 32]],
            &[report],
            1,
            100,
            0,
            0,
            1,
            0,
        )
        .expect("public recipient Hybrid configuration");
        Some(report)
    } else {
        None
    };
    let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
    let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
    let portfolios = funded.map(|pair| pair.0);
    let tokens = funded.map(|pair| pair.1);
    let [target, peer, trader_a, trader_b, keeper] = portfolios;
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
    .expect("fixed public SPL endowment");
    let mut peak_trade = env.trade_asset_with_cu(
        0,
        &owners[0],
        target,
        &owners[1],
        peer,
        (100 * POS_SCALE) as i128,
        ENTRY,
        0,
    );
    if routes.is_some() {
        if !hybrid_recipient {
            env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
        }
        peak_trade = peak_trade.max(env.trade_asset_with_cu(
            1,
            &owners[4],
            keeper,
            &owners[1],
            peer,
            3 * POS_SCALE as i128,
            100,
            0,
        ));
        if !hybrid_recipient {
            env.push_auth_mark_for_asset_as_admin(1, 1, 120);
        }
    }
    let mut tracked = vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
    tracked.extend(recipient_initial);
    tracked.extend(portfolios);
    tracked.extend(tokens);
    tracked.extend(owners.each_ref().map(Signer::pubkey));
    let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
    let supply = FUNDS.iter().map(|value| *value as u128).sum::<u128>();
    assert_eq!(env.token_amount(env.vault) as u128, supply);

    set_test_clock(&mut env, 5, 1_000);
    let clock_only = observe(
        &env,
        if routes.is_some() { trader_a } else { keeper },
        owners[4].pubkey(),
        Some(initial),
        None,
    );
    let mut peak_crank = submit(&mut env, &owners[4], &[clock_only], &tracked, None);
    assert_eq!(env.market_state().1.assets[0].slot_last, 5);
    assert_eq!(env.market_state().0.mark_ewma_last_slot, 1);
    peak_trade = peak_trade.max(if let Some((batch, cpi, _)) = routes {
        paid_origin_routes::discover(&mut env, &owners, portfolios, &mut tracked, batch, cpi)
    } else {
        env.trade_asset_with_cu(
            0,
            &owners[2],
            trader_a,
            &owners[3],
            trader_b,
            POS_SCALE as i128,
            900_000,
            0,
        )
    });
    let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
    let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
    let paid = fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
    let discovery = 2 * paid;
    let mut initial_values = FUNDS.map(i128::from);
    initial_values[2] -= paid as i128;
    initial_values[3] -= paid as i128;
    assert_eq!(values(&env, portfolios), initial_values);
    assert_eq!(env.market_state().1.insurance, discovery);
    assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
    assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
    census(&env, portfolios);

    let mut old_penalty = 0;
    let mut new_penalties = 0;
    let mut rewards = 0;
    let mut budgets = [0u128; 2];
    let mut peak_reject = 0;
    let mut late_rollbacks = 0;
    let mut reward_rollbacks = 0;
    let mut liquidation_count = 0;
    let mut missing_rollbacks = 0;
    let stale_price = ENTRY - ENTRY * 24 / 10_000;
    // Replacing the pending target restarts the cap anchor at the accepted price.
    let handoff_price = stale_price - stale_price * 24 / 10_000;
    for (phase, slot, price) in [
        (0, 6, stale_price),
        (1, 7, handoff_price),
        (2, 14, FRESH_TARGET),
    ] {
        let now = 995 + slot as i64;
        set_test_clock(&mut env, slot, now);
        let report = if phase == 0 {
            initial
        } else {
            env.set_pyth_price_with_conf(&feed, FRESH_TARGET as i64, -6, 0, now)
        };
        let recipient_report = if routes.is_some() && hybrid_recipient {
            Some(env.set_pyth_price_with_conf(&recipient_feed, 120, -6, 0, now))
        } else {
            None
        };
        let equivocal = env.set_pyth_price_with_conf(&feed, FRESH_TARGET as i64 + 1, -6, 0, now);
        tracked.extend([report, equivocal]);
        tracked.extend(recipient_report);
        let valid = observe_with_optional_recipient_report(
            &env,
            target,
            owners[4].pubkey(),
            Some(report),
            recipient_report,
            Some(keeper),
        );
        let conflict = observe_with_optional_recipient_report(
            &env,
            target,
            owners[4].pubkey(),
            Some(equivocal),
            recipient_report,
            Some(keeper),
        );
        let missing = observe_with_optional_recipient_report(
            &env,
            target,
            owners[4].pubkey(),
            None,
            recipient_report,
            None,
        );
        if let Some((_, _, publish_first)) = routes {
            peak_reject = peak_reject.max(submit(
                &mut env,
                &owners[4],
                &[missing.clone()],
                &tracked,
                Some((2, InstructionError::NotEnoughAccountKeys)),
            ));
            missing_rollbacks += 1;
            if publish_first {
                let publication = refresh_with_optional_recipient_report(
                    &env,
                    keeper,
                    owners[4].pubkey(),
                    report,
                    recipient_report,
                );
                peak_reject = peak_reject.max(submit(
                    &mut env,
                    &owners[4],
                    &[publication.clone(), missing.clone()],
                    &tracked,
                    Some((3, InstructionError::NotEnoughAccountKeys)),
                ));
                missing_rollbacks += 1;
                let mut before_value = values(&env, portfolios);
                peak_crank =
                    peak_crank.max(submit(&mut env, &owners[4], &[publication], &tracked, None));
                before_value[4] = FUNDS[4] as i128
                    + rewards as i128
                    + 3 * ((slot - 1) * 100 * 24 / 10_000) as i128;
                assert_eq!(values(&env, portfolios), before_value);
                census(&env, portfolios);
            }
        }
        let mut liquidated = false;
        for _ in 0..6 {
            let before = env.market_state().1;
            let before_values = values(&env, portfolios);
            let keeper_before = env.portfolio_state(keeper);
            let before_cert = health_cert(&env.portfolio_state(target));
            if phase == 2
                && before.assets[0].effective_price == price
                && before_cert.certified_liq_deficit == 0
                && census(&env, portfolios)[0]
            {
                break;
            }
            let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
            if routes.is_some() {
                peak_reject = peak_reject.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone(), missing.clone()],
                    &tracked,
                    Some((3, InstructionError::NotEnoughAccountKeys)),
                ));
                missing_rollbacks += 1;
            }
            if phase != 0 {
                peak_reject = peak_reject.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone(), conflict.clone()],
                    &tracked,
                    Some((
                        3,
                        InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                    )),
                ));
                late_rollbacks += 1;
            }
            peak_crank = peak_crank.max(submit(
                &mut env,
                &owners[4],
                &[valid.clone()],
                &tracked,
                None,
            ));
            let current = census(&env, portfolios);
            let keeper_after = env.portfolio_state(keeper);
            assert_eq!(keeper_after.legs, keeper_before.legs);
            assert_eq!(keeper_after.pnl, keeper_before.pnl);
            let (profile, after) = env.market_state();
            assert_eq!(after.assets[0].effective_price, price, "phase={phase}");
            assert_eq!(
                after.assets[0].raw_oracle_target_price,
                if phase == 0 { MARK } else { FRESH_TARGET }
            );
            assert_eq!(
                profile.last_good_oracle_slot,
                if phase == 0 { 1 } else { slot }
            );
            assert_eq!(
                profile.oracle_target_publish_time,
                if phase == 0 { 100 } else { now }
            );
            assert_eq!(
                [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                peers
            );
            assert_eq!(
                [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                custody
            );
            assert_eq!(after.vault, supply);
            let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
            if closed == 0 {
                assert_eq!(values(&env, portfolios)[4], before_values[4]);
                assert_eq!(after.insurance, before.insurance);
                assert_eq!(
                    after.insurance_domain_budget,
                    before.insurance_domain_budget
                );
                continue;
            }
            assert_eq!(before.assets[0].effective_price, price);
            assert!(before_cert.certified_liq_deficit > 0 && current[0]);
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
            assert!(closed < before.assets[0].oi_eff_long_q);
            let penalty = fee(closed, price, 5);
            let penalty_reclaimable = phase != 0 && routes.is_none();
            let reward = if penalty_reclaimable {
                penalty * SHARE / 10_000
            } else {
                0
            };
            assert!(penalty > 0);
            if phase == 0 {
                old_penalty += penalty;
                assert_eq!(budgets, [0, 0]);
            } else {
                assert!(old_penalty > 0);
                if phase == 1 {
                    assert!(
                        price > MARK && MARK > FRESH_TARGET,
                        "fresh publication precedes paid-target catchup"
                    );
                    for wrong in [ENTRY, MARK, FRESH_TARGET, ACCEPTED_PRINT, 900_000] {
                        assert_ne!(penalty, fee(closed, wrong, 5));
                    }
                }
                new_penalties += penalty;
                rewards += reward;
                if penalty_reclaimable {
                    assert!(reward > 0);
                    let retained = penalty - reward;
                    budgets[0] += retained / 2;
                    budgets[1] += retained.div_ceil(2);
                    reward_rollbacks += 1;
                } else {
                    assert_eq!(
                        reward, 0,
                        "trade-origin liquidation penalties must not become cranker rewards"
                    );
                }
            }
            let mut expected = before_values;
            expected[0] -= penalty as i128;
            expected[4] += reward as i128;
            assert_eq!(values(&env, portfolios), expected);
            assert_eq!(
                after.insurance,
                discovery + old_penalty + new_penalties - rewards
            );
            assert_eq!(&after.insurance_domain_budget[..2], &budgets);
            assert!(after.insurance_domain_budget[2..]
                .iter()
                .all(|amount| *amount == 0));
            assert_eq!(
                after.insurance_domain_budget_remaining_total,
                budgets.iter().sum::<u128>()
            );
            assert_eq!(
                after.insurance - after.insurance_domain_budget_remaining_total,
                discovery + old_penalty + new_penalties - rewards - budgets.iter().sum::<u128>(),
                "trade-origin discovery and liquidation stock cannot become new domain entitlement"
            );
            assert_eq!(
                after.assets[0].oi_eff_long_q,
                after.assets[0].oi_eff_short_q
            );
            println!("retained penalty phase={phase} price={price} closed={closed} penalty={penalty} reward={reward} budgets={budgets:?}");
            liquidation_count += 1;
            liquidated = true;
            break;
        }
        assert_eq!(
            liquidated,
            phase != 2,
            "two liquidation episodes; final catchup cannot charge or reward again"
        );
        if routes.is_some() && !census(&env, portfolios)[4] {
            let ix = refresh_with_optional_recipient_report(
                &env,
                keeper,
                owners[4].pubkey(),
                report,
                recipient_report,
            );
            peak_crank = peak_crank.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
        }
        // Finish account-local ADL/source work before the next price boundary.
        for _ in 0..8 {
            for i in [1, 2, 3, 0] {
                if census(&env, portfolios)[i] {
                    continue;
                }
                let ix = if routes.is_some() {
                    refresh_with_optional_recipient_report(
                        &env,
                        portfolios[i],
                        owners[4].pubkey(),
                        report,
                        recipient_report,
                    )
                } else {
                    observe(&env, portfolios[i], owners[4].pubkey(), Some(report), None)
                };
                peak_crank = peak_crank.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
            }
            if census(&env, portfolios)[..4].iter().all(|value| *value) {
                break;
            }
        }
        assert!(census(&env, portfolios)[..4].iter().all(|value| *value));
        let group = env.market_state().1;
        assert_eq!(
            group.insurance,
            discovery + old_penalty + new_penalties - rewards
        );
        assert_eq!(&group.insurance_domain_budget[..2], &budgets);
        assert_eq!(
            values(&env, portfolios)[4],
            FUNDS[4] as i128
                + rewards as i128
                + if routes.is_some() {
                    3 * ((slot - 1) * 100 * 24 / 10_000) as i128
                } else {
                    0
                }
        );
        if routes.is_some() {
            let asset = group.assets[1];
            let movement = (slot - 1) * 100 * 24 / 10_000;
            assert_eq!(
                (asset.effective_price, asset.k_long),
                (100 + movement, movement as i128 * ADL_ONE as i128)
            );
            assert_eq!(
                (asset.oi_eff_long_q, asset.oi_eff_short_q),
                (3 * POS_SCALE, 3 * POS_SCALE)
            );
        }
        peak_reject = peak_reject.max(submit(
            &mut env,
            &owners[4],
            &[valid],
            &tracked,
            Some((
                2,
                InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
            )),
        ));
    }
    assert_eq!(liquidation_count, 2);
    assert_eq!(reward_rollbacks, usize::from(routes.is_none()));
    assert!(
        late_rollbacks >= 2,
        "fresh publication and liquidation both roll back"
    );
    if routes.is_some() {
        let before_values = values(&env, portfolios);
        env.svm.expire_blockhash();
        peak_trade = peak_trade.max(env.trade_asset_with_cu(
            1,
            &owners[4],
            keeper,
            &owners[1],
            peer,
            -3 * POS_SCALE as i128,
            103,
            0,
        ));
        assert_eq!(values(&env, portfolios), before_values);
        assert_eq!(env.market_state().1.assets[1].oi_eff_long_q, 0);
    }
    let payout = FUNDS[4] as u128 + rewards;
    let before_payout = values(&env, portfolios);
    let withdrawal = Instruction {
        program_id: env.program_id,
        data: env.withdraw_ix(keeper, payout).encode(),
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
    if routes.is_some() {
        let missing = observe(&env, target, owners[4].pubkey(), None, None);
        peak_reject = peak_reject.max(submit(
            &mut env,
            &owners[4],
            &[withdrawal.clone(), missing],
            &tracked,
            Some((3, InstructionError::NotEnoughAccountKeys)),
        ));
        missing_rollbacks += 1;
    }
    let payout_cu = submit(&mut env, &owners[4], &[withdrawal], &tracked, None);
    let mut expected = before_payout;
    expected[4] -= payout as i128;
    assert_eq!(expected[4], if routes.is_some() { 9 } else { 0 });
    assert_eq!(values(&env, portfolios), expected);
    assert_eq!(env.token_amount(tokens[4]) as u128, payout);
    assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
    assert_eq!(env.svm.get_account(&env.mint), custody[0]);
    assert_eq!(env.token_amount(env.vault) as u128, supply - payout);
    census(&env, portfolios);
    let group = env.market_state().1;
    assert_eq!(group.vault, supply - payout);
    assert_eq!(
        group.insurance,
        discovery + old_penalty + new_penalties - rewards
    );
    assert_eq!(&group.insurance_domain_budget[..2], &budgets);
    assert!(group.insurance_domain_budget[2..]
        .iter()
        .all(|amount| *amount == 0));
    assert_eq!(
        group.insurance_domain_budget_remaining_total,
        budgets.iter().sum::<u128>()
    );
    assert_eq!(
        group.insurance - group.insurance_domain_budget_remaining_total,
        discovery + old_penalty + new_penalties - rewards - budgets.iter().sum::<u128>()
    );
    assert_cu_within("retained penalty trade", peak_trade, TRADE_CU_LIMIT);
    assert_cu_within("retained penalty crank", peak_crank, crank_limit);
    assert_cu_within("retained penalty rejection", peak_reject, 2 * crank_limit);
    assert_cu_within("retained penalty payout", payout_cu, CUSTODY_CU_LIMIT);
    println!("retained penalty: liquidations={liquidation_count} late_rollbacks={late_rollbacks} fixed_point_rollbacks=3 rewarded_rollbacks={reward_rollbacks} discovery={discovery} old_penalty={old_penalty} new_penalties={new_penalties} rewards={rewards} payout={payout} CU trade={peak_trade} crank={peak_crank} rejection={peak_reject} payout={payout_cu}");
    if routes.is_some() {
        assert!(missing_rollbacks >= 7);
        eprintln!("Scope I paid origin: routes={routes:?}, missing_rollbacks={missing_rollbacks}, payout_rollbacks=1");
    }
    (
        expected,
        budgets,
        [discovery, old_penalty, new_penalties, payout],
        peak_trade.max(peak_crank).max(peak_reject).max(payout_cu),
    )
}

#[test]
fn v16_program_retained_stale_penalty_survives_fresh_liquidation_and_catchup() {
    run_retained_handoff(None);
}
