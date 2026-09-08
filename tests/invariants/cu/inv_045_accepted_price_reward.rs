//! Wrapper composition for INV-020/024/036/041/045/061/062, reopening 422.
//! Unequal fresh reports share an accepted-price prefix, then converge through a
//! fresh catchup report. The fee price and keeper split for the actual closed
//! quantity follow that prefix, not the unaccepted report. Distinct authenticated
//! targets can change conservative sizing; engine mark/selection proofs stay elsewhere.

use super::*;

const ENTRY: u64 = 1_000_000;
const ACCEPTED: u64 = 997_600;
const FEE_BPS: u128 = 5;
const REWARD_BPS: u128 = 3_333;
const DEPOSITS: [u128; 3] = [51_000, 1_000_000, 1_000];

#[derive(Debug, PartialEq, Eq)]
struct RewardPhase {
    price: u64,
    remaining_q: u128,
    fee: u128,
    reward: u128,
    actor_value: [i128; 3],
    insurance: u128,
}

#[derive(Debug, PartialEq, Eq)]
struct RewardOutcome {
    phases: Vec<RewardPhase>,
    payout: u128,
    final_actor_value: [i128; 3],
    final_insurance: u128,
    final_vault: u128,
}

fn actor_values(env: &V16CuEnv, portfolios: [Pubkey; 3]) -> [i128; 3] {
    portfolios.map(|key| {
        let account = env.portfolio_state(key);
        i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
    })
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn fee_at_price(closed_q: u128, price: u64) -> u128 {
    // Independent two-stage quote rounding; these bounded operands fit u128.
    let notional = (closed_q * u128::from(price)).div_ceil(POS_SCALE);
    (notional * FEE_BPS).div_ceil(10_000)
}

fn reward_crank(
    env: &mut V16CuEnv,
    target: Pubkey,
    keeper: (&Keypair, Pubkey),
    observations: Vec<CrankObservationHint>,
    oracles: &[Pubkey],
) -> Result<u64, String> {
    let mut accounts = vec![
        AccountMeta::new(keeper.0.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    accounts.extend(
        oracles
            .iter()
            .map(|key| AccountMeta::new_readonly(*key, false)),
    );
    accounts.push(AccountMeta::new(keeper.1, false));
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations,
        },
        accounts,
        &[keeper.0],
    )
}

fn trade(
    env: &mut V16CuEnv,
    route: AccountResidualCounterTradePath,
    owners: [&Keypair; 2],
    portfolios: [Pubkey; 2],
    matcher: (Pubkey, Pubkey, Pubkey),
    size_q: i128,
    price: u64,
) -> u64 {
    let [a, b] = portfolios;
    let [owner_a, owner_b] = owners;
    let (program, context, delegate) = matcher;
    env.svm.expire_blockhash();
    match route {
        AccountResidualCounterTradePath::TradeNoCpi => {
            env.trade_asset_with_cu(0, owner_a, a, owner_b, b, size_q, price, 0)
        }
        AccountResidualCounterTradePath::TradeCpi => env.trade_cpi_with_cu_on_asset(
            owner_a, a, owner_b, b, program, context, delegate, 0, size_q, 0,
        ),
        AccountResidualCounterTradePath::BatchTradeNoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    a,
                    b,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        exec_price: price,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(owner_a.pubkey(), true),
                    AccountMeta::new(owner_b.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                ],
                &[owner_a, owner_b],
            )
            .expect("public batch open/reduction"),
        AccountResidualCounterTradePath::BatchTradeCpi => env
            .send(
                env.batch_trade_cpi_ix(
                    a,
                    b,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(owner_a.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                    AccountMeta::new_readonly(program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[owner_a],
            )
            .expect("public batch CPI open/reduction"),
    }
}

fn run_world(
    route: AccountResidualCounterTradePath,
    raw_report: u64,
    common_owner: bool,
) -> RewardOutcome {
    let label = format!("{route:?}/report={raw_report}/common={common_owner}");
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    assert_eq!(
        env.market_state().1.config.liquidation_fee_bps,
        FEE_BPS as u64
    );
    env.update_liquidation_fee_policy_with_cu(REWARD_BPS as u16);
    set_test_clock(&mut env, 1, 100);
    let feed = [0x45; 32];
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
        1_000,
        0,
    )
    .expect("public fresh-feed configuration");

    let owner = Keypair::new();
    let peer_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let owners = [
        &owner,
        if common_owner { &owner } else { &peer_owner },
        if common_owner { &owner } else { &keeper_owner },
    ];
    let portfolios = owners.map(|owner| env.create_portfolio(owner));
    let [target, peer, keeper] = portfolios;
    let sources: Vec<_> = (0..3)
        .map(|i| env.deposit(owners[i], portfolios[i], DEPOSITS[i]))
        .collect();
    let matcher = auth_matcher_for_lp_via_system_create(&mut env, owners[1], peer);
    let open_cu = trade(
        &mut env,
        route,
        [owners[0], owners[1]],
        [target, peer],
        matcher,
        POS_SCALE as i128,
        ENTRY,
    );
    assert_cu_within(&label, open_cu, TRADE_CU_LIMIT);
    assert_eq!(
        actor_values(&env, portfolios),
        DEPOSITS.map(|value| value as i128)
    );
    let vault = env.token_amount(env.vault);
    assert_eq!(u128::from(vault), DEPOSITS.iter().sum::<u128>());

    // Include all economic accounts, signer lamports, matcher state and external
    // inputs. Only the unrelated transaction fee payer is excluded from rollback.
    let mut keys = vec![
        env.market,
        env.mint,
        env.vault,
        target,
        peer,
        keeper,
        env.admin.pubkey(),
        matcher.0,
        matcher.1,
        matcher.2,
        initial,
    ];
    keys.extend(owners.map(Signer::pubkey));
    keys.extend(sources);
    let immutable_keys = [env.mint, matcher.0, matcher.2, initial];
    let immutable = frame(&env, &immutable_keys);
    let matcher_after_open = env.svm.get_account(&matcher.1);
    let mut phases = Vec::new();
    let mut total_reward = 0;
    let mut total_fee = 0;
    let mut max_crank_cu = 0;

    // The first accepted price is deliberately distinct from BOTH raw reports.
    // A later common fresh report removes target/effective lag for the payout.
    for (slot, report) in [(2, raw_report), (3, ACCEPTED)] {
        let accepted_price = ACCEPTED;
        let now = 99 + slot as i64;
        set_test_clock(&mut env, slot, now);
        let fresh = env.set_pyth_price_with_conf(&feed, report as i64, -6, 0, now);
        let stale = env.set_pyth_price_with_conf(&feed, report as i64, -6, 0, now - 61);
        let foreign = env.set_pyth_price_with_conf(&[0x46; 32], report as i64, -6, 0, now);
        keys.extend([fresh, stale, foreign]);

        // Bad evidence cannot refresh either certificate or pay the attached keeper.
        for (case, hints, oracles, expected_error) in [
            (
                "stale",
                crank_observations_with_accounts(0, 1),
                vec![stale],
                "Custom(27)",
            ),
            (
                "foreign feed",
                crank_observations_with_accounts(0, 1),
                vec![foreign],
                "Custom(29)",
            ),
            (
                "duplicate observation",
                vec![
                    CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 1
                    };
                    2
                ],
                vec![fresh, fresh],
                "Custom(9)",
            ),
        ] {
            let before = frame(&env, &keys);
            let error = reward_crank(&mut env, target, (owners[2], keeper), hints, &oracles)
                .expect_err("invalid observation must reject");
            assert!(
                error.contains(expected_error),
                "{label}/{slot}/{case}: {error}"
            );
            assert_eq!(
                frame(&env, &keys),
                before,
                "{label}/{slot}/{case}: exact rollback"
            );
        }

        let before_publication = actor_values(&env, portfolios);
        let cu = env.crank_with_oracle_tail(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            &[fresh],
        );
        max_crank_cu = max_crank_cu.max(cu);
        let published = env.market_state().1;
        assert_eq!(
            published.assets[0].raw_oracle_target_price, report,
            "{label}"
        );
        assert_eq!(
            published.assets[0].effective_price, accepted_price,
            "{label}"
        );
        assert_eq!(
            actor_values(&env, portfolios),
            before_publication,
            "publication is not a reward"
        );
        assert_eq!(published.insurance, total_fee - total_reward);

        let before_q = published.assets[0].oi_eff_long_q;
        let mut liquidation = None;
        for _ in 0..6 {
            let account_before = env.portfolio_state(target);
            let group_before = env.market_state().1;
            let values_before = actor_values(&env, portfolios);
            let peer_before = env.svm.get_account(&peer);
            let before = frame(&env, &keys);
            let result = reward_crank(
                &mut env,
                target,
                (owners[2], keeper),
                crank_observations_with_accounts(0, 1),
                &[fresh],
            );
            if slot == 3 {
                if let Err(error) = &result {
                    assert!(is_engine_non_progress_error(error), "{label}: {error}");
                    assert_eq!(
                        frame(&env, &keys),
                        before,
                        "healthy retry must roll back exactly"
                    );
                    assert_eq!(health_cert(&account_before).certified_liq_deficit, 0);
                    liquidation = Some((0, 0));
                    break;
                }
            }
            let cu = result
                .unwrap_or_else(|error| panic!("{label}/{slot}: bounded liquidation: {error}"));
            max_crank_cu = max_crank_cu.max(cu);
            let after = env.market_state().1;
            let values_after = actor_values(&env, portfolios);
            assert_eq!(after.assets[0].effective_price, accepted_price);
            assert_eq!(env.token_amount(env.vault), vault);
            assert_eq!(after.vault, u128::from(vault));
            assert_eq!(env.svm.get_account(&peer), peer_before);
            let closed_q = before_q - after.assets[0].oi_eff_long_q;
            if slot == 3 {
                assert_eq!(closed_q, 0, "same accepted price cannot create a new close");
                assert_eq!(
                    values_after, values_before,
                    "catchup cannot redistribute quote"
                );
            }
            if closed_q == 0 {
                assert_eq!(
                    values_after[2], values_before[2],
                    "refresh cannot pay a reward"
                );
                assert_eq!(after.insurance, group_before.insurance);
                continue;
            }
            assert!(health_cert(&account_before).certified_liq_deficit > 0);
            assert!(closed_q < before_q, "fixture must retain a real owner exit");
            let fee = fee_at_price(closed_q, accepted_price);
            let reward = fee * REWARD_BPS / 10_000;
            assert!(
                reward > 0 && reward < fee,
                "nonvacuous, negative-sum self reward"
            );
            if slot == 2 {
                assert_ne!(
                    fee,
                    fee_at_price(closed_q, report),
                    "raw-price oracle must distinguish this case"
                );
            }
            assert_eq!(
                values_before[0] - values_after[0],
                fee as i128,
                "target pays its fee"
            );
            assert_eq!(
                values_after[1], values_before[1],
                "peer cannot collect the fee"
            );
            assert_eq!(
                values_after[2] - values_before[2],
                reward as i128,
                "keeper receives only its share"
            );
            assert_eq!(after.insurance - group_before.insurance, fee - reward);
            assert_eq!(
                after.assets[0].oi_eff_long_q,
                after.assets[0].oi_eff_short_q
            );
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
            total_reward += reward;
            total_fee += fee;
            liquidation = Some((fee, reward));
            break;
        }
        let (fee, reward) = liquidation.expect(
            "six cranks must reach partial liquidation or the post-catchup healthy fixed point",
        );

        // Refresh the counterparty's ADL-effective leg without another target fee.
        let before_reward = env.portfolio_state(keeper).capital.get();
        let cu = env.crank_with_oracle_tail(
            peer,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            &[fresh],
        );
        max_crank_cu = max_crank_cu.max(cu);
        let after = env.market_state().1;
        assert_eq!(env.portfolio_state(keeper).capital.get(), before_reward);
        assert_eq!(after.insurance, total_fee - total_reward);
        let marked_loss = i128::from(ENTRY - ACCEPTED);
        assert_eq!(
            actor_values(&env, portfolios),
            [
                DEPOSITS[0] as i128 - marked_loss - total_fee as i128,
                DEPOSITS[1] as i128 + marked_loss,
                DEPOSITS[2] as i128 + total_reward as i128,
            ],
            "accepted-price history, not aggregate stock alone, defines every actor's claim"
        );
        assert_eq!(
            actor_values(&env, portfolios).iter().sum::<i128>() + after.insurance as i128,
            i128::from(vault)
        );
        phases.push(RewardPhase {
            price: accepted_price,
            remaining_q: after.assets[0].oi_eff_long_q,
            fee,
            reward,
            actor_value: actor_values(&env, portfolios),
            insurance: after.insurance,
        });
    }

    assert_eq!(
        env.svm.get_account(&matcher.1),
        matcher_after_open,
        "oracle/crank routes cannot alter the matcher context"
    );
    let remaining = env.market_state().1.assets[0].oi_eff_long_q;
    let exit_cu = trade(
        &mut env,
        route,
        [owners[0], owners[1]],
        [target, peer],
        matcher,
        -(remaining as i128),
        ACCEPTED,
    );
    assert_cu_within(&label, exit_cu, TRADE_CU_LIMIT);
    for portfolio in [target, peer] {
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(portfolio),
            0
        ));
    }
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.portfolio_state(keeper).capital.get(),
        DEPOSITS[2] + total_reward
    );

    let values_before_payout = actor_values(&env, portfolios);
    let matcher_after_exit = env.svm.get_account(&matcher.1);
    let payout = DEPOSITS[2] + total_reward;
    let (destination, withdraw_cu) = env.withdraw_with_cu(owners[2], keeper, payout);
    assert_cu_within(&label, withdraw_cu, CUSTODY_CU_LIMIT);
    assert_eq!(
        env.token_amount(destination),
        u64::try_from(payout).unwrap()
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault - u64::try_from(payout).unwrap()
    );
    let final_values = actor_values(&env, portfolios);
    assert_eq!(
        final_values,
        [values_before_payout[0], values_before_payout[1], 0]
    );
    let final_group = env.market_state().1;
    assert_eq!(final_group.insurance, total_fee - total_reward);
    assert_eq!(final_group.vault, u128::from(env.token_amount(env.vault)));
    assert_eq!(
        final_values.iter().sum::<i128>() + final_group.insurance as i128 + payout as i128,
        i128::from(vault)
    );
    assert_eq!(frame(&env, &immutable_keys), immutable);
    assert_eq!(env.svm.get_account(&matcher.1), matcher_after_exit);
    assert_cu_within(&label, max_crank_cu, CRANK_CU_LIMIT);
    println!("{label}: fees={total_fee}, reward={total_reward}, payout={payout}, crank_max_cu={max_crank_cu}");
    RewardOutcome {
        phases,
        payout,
        final_actor_value: final_values,
        final_insurance: final_group.insurance,
        final_vault: final_group.vault,
    }
}

#[test]
fn v16_program_fresh_report_liquidation_rewards_follow_accepted_price_through_spl_exit() {
    let mut worlds = 0;
    for report in [500_000, 750_000] {
        let mut baseline = None;
        for route in [
            AccountResidualCounterTradePath::TradeNoCpi,
            AccountResidualCounterTradePath::TradeCpi,
            AccountResidualCounterTradePath::BatchTradeNoCpi,
            AccountResidualCounterTradePath::BatchTradeCpi,
        ] {
            for common_owner in [false, true] {
                let result = run_world(route, report, common_owner);
                if let Some(expected) = &baseline {
                    assert_eq!(
                        &result, expected,
                        "transport or common ownership changed the same report history's entitlement"
                    );
                } else {
                    baseline = Some(result);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
}
