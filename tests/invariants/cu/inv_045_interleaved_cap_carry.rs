//! INV-045, reopening 422/425: mixed trade routes must not spend, erase, or duplicate
//! fractional oracle capacity, or turn previously collected trade fees into keeper rewards.
//! Public fresh-feed reports and reductions share one trajectory in two route orders.
//! A known maintenance debit crosses the conservative lag-adjusted health threshold
//! on the fifth slot; neither that debit nor old trade fees is a liquidation reward.
//! This is sampled fixed-artifact evidence, not closure of the invariant_status row.

use super::*;

const ENTRY: u64 = 100;
const REPORT: u64 = 80;
const CAP_BPS: u64 = 24;
const TRADE_BPS: u128 = 37;
const REWARD_BPS: u128 = 3_333;
const MAINTENANCE: u128 = 5_000;
const TARGET_Q: u128 = 10_000 * POS_SCALE;
const REDUCTION_Q: u128 = 101 * POS_SCALE;
const DEPOSITS: [u128; 5] = [270_100, 1_000_000, 1_000_000, 1_000_000, 100_000];

fn profile(env: &V16CuEnv) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0).unwrap()
}

fn values(env: &V16CuEnv, portfolios: [Pubkey; 5]) -> [i128; 5] {
    portfolios.map(|key| {
        let account = env.portfolio_state(key);
        i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
    })
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn fee(q: u128, price: u64, bps: u128) -> u128 {
    // Independent bounded arithmetic: quote notional rounds up before the fee does.
    ((q * u128::from(price)).div_ceil(POS_SCALE) * bps).div_ceil(10_000)
}

fn assert_cap(env: &V16CuEnv, elapsed: u64) {
    // The authenticated target and the exposed trajectory's 100-atom anchor are constant.
    // Neither trade count, OI reductions, nor same-slot report count buys elapsed slots.
    let numerator = ENTRY * CAP_BPS * elapsed;
    let accepted = ENTRY - numerator / 10_000;
    let asset = env.market_state().1.assets[0];
    let profile = profile(env);
    assert_eq!(asset.effective_price, accepted);
    assert_eq!(profile.mark_ewma_e6, accepted);
    assert_eq!(
        profile.price_move_remainder_bps_num as u64,
        numerator % 10_000
    );
    assert_eq!(asset.raw_oracle_target_price, REPORT);
    assert_eq!(profile.oracle_target_price_e6, REPORT);
    assert_eq!(asset.fund_px_last, ENTRY);
    assert_eq!(asset.slot_last, 1 + elapsed);
}

fn reward_crank(
    env: &mut V16CuEnv,
    target: Pubkey,
    keeper_owner: &Keypair,
    keeper: Pubkey,
    oracle: Pubkey,
) -> Result<u64, String> {
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(keeper_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new_readonly(oracle, false),
            AccountMeta::new(keeper, false),
        ],
        &[keeper_owner],
    )
}

fn reduce(
    env: &mut V16CuEnv,
    route: AccountResidualCounterTradePath,
    owners: [&Keypair; 2],
    portfolios: [Pubkey; 2],
    matcher: (Pubkey, Pubkey, Pubkey),
) -> u64 {
    let [a, b] = portfolios;
    let [owner_a, owner_b] = owners;
    let (program, context, delegate) = matcher;
    let size_q = -(REDUCTION_Q as i128);
    let fee_bps = TRADE_BPS as u64;
    if matches!(
        route,
        AccountResidualCounterTradePath::TradeCpi | AccountResidualCounterTradePath::BatchTradeCpi
    ) {
        // A non-CPI position change disables the old LP delegation. The owner
        // publicly reauthorizes the same context and the exact fee before reuse.
        env.set_matcher_config_with_trade_fee_cap(
            program,
            owner_b,
            b,
            context,
            delegate,
            1,
            TRADE_BPS as u16,
        );
    }
    env.svm.expire_blockhash();
    match route {
        AccountResidualCounterTradePath::TradeNoCpi => {
            env.trade_asset_with_cu(0, owner_a, a, owner_b, b, size_q, REPORT, fee_bps)
        }
        AccountResidualCounterTradePath::TradeCpi => env.trade_cpi_with_cu_on_asset(
            owner_a, a, owner_b, b, program, context, delegate, 0, size_q, fee_bps,
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
                        exec_price: REPORT,
                        fee_bps,
                    }],
                ),
                vec![
                    AccountMeta::new(owner_a.pubkey(), true),
                    AccountMeta::new(owner_b.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                ],
                &owners,
            )
            .expect("public batch reduction with bilateral fee consent"),
        AccountResidualCounterTradePath::BatchTradeCpi => env
            .send(
                env.batch_trade_cpi_ix(
                    a,
                    b,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        fee_bps,
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
            .expect("public batch CPI reduction with delegated fee consent"),
    }
}

#[test]
fn v16_program_interleaved_trade_routes_preserve_oracle_cap_carry_and_reward_provenance() {
    let routes = [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
    ];
    let trade_fee = fee(REDUCTION_Q, ENTRY, TRADE_BPS);
    assert_eq!(trade_fee, 38);
    assert_ne!(trade_fee, fee(REDUCTION_Q, REPORT, TRADE_BPS));
    let mut outcomes = Vec::new();

    for reverse_routes in [false, true] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: ENTRY,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: MAINTENANCE,
            ..production_risk_params()
        });
        let config = env.market_state().1.config;
        assert_eq!(config.max_price_move_bps_per_slot, CAP_BPS);
        assert_eq!(config.liquidation_fee_bps, 5);
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
            100,
            0,
        )
        .expect("public fresh-oracle configuration");
        let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        let [target, peer, trader_a, trader_b, keeper] = portfolios;
        let sources: Vec<_> = (0..5)
            .map(|i| env.deposit(&owners[i], portfolios[i], DEPOSITS[i]))
            .collect();
        for (a, b, q) in [(0, 1, TARGET_Q), (2, 3, 8 * REDUCTION_Q)] {
            env.svm.expire_blockhash();
            env.try_trade_asset_with_cu(
                0,
                &owners[a],
                portfolios[a],
                &owners[b],
                portfolios[b],
                q as i128,
                ENTRY,
                0,
            )
            .unwrap_or_else(|error| panic!("public opening pair {a}/{b}: {error}"));
        }
        assert_eq!(values(&env, portfolios), DEPOSITS.map(|v| v as i128));
        env.update_trade_fee_policy_with_cu(TRADE_BPS as u64);
        let matcher = auth_matcher_for_lp_via_system_create(&mut env, &owners[3], trader_b);
        let mut keys = vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
        keys.extend([matcher.0, matcher.1, matcher.2]);
        keys.extend(portfolios);
        keys.extend(owners.each_ref().map(Signer::pubkey));
        keys.extend(sources.iter().copied());
        let vault = env.token_amount(env.vault);
        assert_eq!(u128::from(vault), DEPOSITS.iter().sum::<u128>());

        // Stage a non-extreme adverse target without any elapsed capacity. A keeper
        // cannot liquidate the long at 80 while the committed engine/Hybrid mark is 100.
        set_test_clock(&mut env, 1, 101);
        let staged = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, 101);
        keys.push(staged);
        let mut max_crank_cu = reward_crank(&mut env, target, &owners[4], keeper, staged)
            .expect("public same-slot target publication");
        assert_cap(&env, 0);
        assert_eq!(values(&env, portfolios), DEPOSITS.map(|v| v as i128));
        assert_eq!(env.market_state().1.insurance, 0);
        let initial_req =
            fee(TARGET_Q, ENTRY, 500) + TARGET_Q * u128::from(ENTRY - REPORT) / POS_SCALE;
        assert_eq!(initial_req, 250_000);
        assert_eq!(
            health_cert(&env.portfolio_state(target)).certified_maintenance_req,
            initial_req
        );
        assert!(DEPOSITS[0] - 4 * MAINTENANCE > initial_req);

        let mut total_trade_fees = 0;
        let mut total_maintenance = 0;
        let mut expected_values = DEPOSITS.map(|v| v as i128);
        let mut trade_cus = [0; 4];
        for elapsed in 1..=4 {
            let slot = 1 + elapsed;
            let now = 100 + 10 * elapsed as i64;
            set_test_clock(&mut env, slot, now);
            let fresh = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, now);
            keys.push(fresh);
            let cu = reward_crank(&mut env, target, &owners[4], keeper, fresh)
                .expect("clock-only public accrual and healthy refresh");
            max_crank_cu = max_crank_cu.max(cu);
            total_maintenance += MAINTENANCE;
            expected_values[0] -= MAINTENANCE as i128;
            assert_eq!(values(&env, portfolios), expected_values);
            assert_eq!(
                env.market_state().1.insurance,
                total_trade_fees + total_maintenance
            );
            assert_cap(&env, elapsed);

            // Every route appears on both sides of a same-slot fresh publication over
            // the two schedules. All eight reductions share and retain the same carry.
            for half in 0..2 {
                let index = (2 * (elapsed as usize - 1) + half) % routes.len();
                let route_index = if reverse_routes { 3 - index } else { index };
                let route = routes[route_index];
                let before_profile = profile(&env);
                let cu = reduce(
                    &mut env,
                    route,
                    [&owners[2], &owners[3]],
                    [trader_a, trader_b],
                    matcher,
                );
                trade_cus[route_index] = trade_cus[route_index].max(cu);
                total_trade_fees += 2 * trade_fee;
                if half == 0 {
                    total_maintenance += 2 * MAINTENANCE;
                    expected_values[2] -= MAINTENANCE as i128;
                    expected_values[3] -= MAINTENANCE as i128;
                }
                expected_values[2] -= trade_fee as i128;
                expected_values[3] -= trade_fee as i128;
                assert_eq!(
                    values(&env, portfolios),
                    expected_values,
                    "{route:?}: actor-local fee"
                );
                assert_eq!(
                    profile(&env),
                    before_profile,
                    "{route:?}: preserve oracle state"
                );
                assert_eq!(
                    env.market_state().1.insurance,
                    total_trade_fees + total_maintenance
                );
                assert_cap(&env, elapsed);

                set_test_clock(&mut env, slot, now + 1 + half as i64);
                let repeated = env.set_pyth_price_with_conf(
                    &feed,
                    REPORT as i64,
                    -6,
                    0,
                    now + 1 + half as i64,
                );
                keys.push(repeated);
                let cu = reward_crank(&mut env, target, &owners[4], keeper, repeated)
                    .expect("same target with a newer authenticated publication");
                max_crank_cu = max_crank_cu.max(cu);
                assert_eq!(
                    values(&env, portfolios),
                    expected_values,
                    "same-slot publication cannot recollect maintenance or pay a reward"
                );
                assert_eq!(
                    env.market_state().1.insurance,
                    total_trade_fees + total_maintenance
                );
                assert_cap(&env, elapsed);
                let target_state = env.portfolio_state(target);
                assert_eq!(
                    active_leg_for_asset(&target_state, 0)
                        .basis_pos_q
                        .unsigned_abs(),
                    TARGET_Q
                );
                assert_eq!(health_cert(&target_state).certified_liq_deficit, 0);

                let before_retry = frame(&env, &keys);
                let error = reward_crank(&mut env, target, &owners[4], keeper, repeated)
                    .expect_err("healthy duplicate cannot extract accumulated trade fees");
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(
                    frame(&env, &keys),
                    before_retry,
                    "exact healthy-retry rollback"
                );
            }
        }
        assert_eq!(total_trade_fees, 608);
        assert_eq!(total_maintenance, 60_000);
        assert_eq!(profile(&env).price_move_remainder_bps_num, 9_600);
        for trader in [trader_a, trader_b] {
            assert!(!has_active_leg_for_asset(&env.portfolio_state(trader), 0));
        }
        assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, TARGET_Q);

        // Even with real trade/maintenance fees in insurance and 9,600 units of cap
        // carry, the keeper cannot exceed its capital net of its own senior fee.
        let before_withdraw = frame(&env, &keys);
        env.svm.expire_blockhash();
        let error = env
            .send(
                env.withdraw_ix(keeper, DEPOSITS[4] - 4 * MAINTENANCE + 1),
                vec![
                    AccountMeta::new(owners[4].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new(sources[4], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[4]],
            )
            .expect_err("unearned reward cannot exit custody before the first committed atom");
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::EngineLockActive as u32
            )),
            "{error}"
        );
        assert_eq!(frame(&env, &keys), before_withdraw);

        set_test_clock(&mut env, 6, 150);
        let fresh = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, 150);
        keys.push(fresh);
        let mut liquidation = None;
        for attempt in 0..6 {
            let before = env.market_state().1;
            let before_target = env.portfolio_state(target);
            let before_values = values(&env, portfolios);
            let peer_before = env.svm.get_account(&peer);
            let cu = reward_crank(&mut env, target, &owners[4], keeper, fresh)
                .expect("bounded first committed-price liquidation");
            max_crank_cu = max_crank_cu.max(cu);
            assert_cap(&env, 5);
            let after = env.market_state().1;
            assert_eq!(env.svm.get_account(&peer), peer_before);
            assert_eq!(env.token_amount(env.vault), vault);
            if attempt == 0 {
                total_maintenance += MAINTENANCE;
                expected_values[0] -= MAINTENANCE as i128 + 10_000;
            }
            let closed_q = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
            if closed_q == 0 {
                assert_eq!(values(&env, portfolios)[4], before_values[4]);
                assert_eq!(values(&env, portfolios), expected_values);
                assert_eq!(after.insurance, total_trade_fees + total_maintenance);
                let cert = health_cert(&env.portfolio_state(target));
                let committed_req =
                    fee(TARGET_Q, 99, 500) + TARGET_Q * u128::from(99 - REPORT) / POS_SCALE;
                assert_eq!(committed_req, 239_500);
                assert_eq!(cert.certified_maintenance_req, committed_req);
                assert_eq!(cert.certified_equity, expected_values[0]);
                assert_eq!(cert.certified_liq_deficit, 4_400);
                continue;
            }
            assert!(closed_q > 0 && closed_q < TARGET_Q);
            assert_eq!(
                before.assets[0].effective_price, 99,
                "mark must already be committed"
            );
            assert!(health_cert(&before_target).certified_liq_deficit > 0);
            let liquidation_fee = fee(closed_q, 99, 5);
            let reward = liquidation_fee * REWARD_BPS / 10_000;
            assert!(reward > 0 && reward < liquidation_fee);
            assert_ne!(liquidation_fee, fee(closed_q, REPORT, 5));
            let mut expected = before_values;
            expected[0] -= liquidation_fee as i128;
            expected[4] += reward as i128;
            assert_eq!(values(&env, portfolios), expected);
            expected_values[0] -= liquidation_fee as i128;
            expected_values[4] += reward as i128;
            assert_eq!(expected, expected_values);
            assert_eq!(after.insurance - before.insurance, liquidation_fee - reward);
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
            liquidation = Some((closed_q, liquidation_fee, reward));
            break;
        }
        let (closed_q, liquidation_fee, reward) = liquidation.expect("nonvacuous paid liquidation");
        let cu = reward_crank(&mut env, peer, &owners[4], keeper, fresh)
            .expect("counterparty refresh attributes the committed-price gain");
        max_crank_cu = max_crank_cu.max(cu);
        total_maintenance += 5 * MAINTENANCE;
        assert_eq!(total_maintenance, 90_000);
        assert_cap(&env, 5);
        let mut expected = DEPOSITS.map(|v| v as i128);
        expected[0] -= 10_000 + liquidation_fee as i128 + (5 * MAINTENANCE) as i128;
        expected[1] += 10_000 - (5 * MAINTENANCE) as i128;
        expected[2] -= (total_trade_fees / 2 + 4 * MAINTENANCE) as i128;
        expected[3] -= (total_trade_fees / 2 + 4 * MAINTENANCE) as i128;
        expected[4] += reward as i128;
        assert_eq!(
            values(&env, portfolios),
            expected,
            "complete actor provenance"
        );
        let mut insurance = total_trade_fees + total_maintenance + liquidation_fee - reward;
        assert_eq!(env.market_state().1.insurance, insurance);
        assert_eq!(
            expected.iter().sum::<i128>() + insurance as i128,
            i128::from(vault)
        );

        let before_payout_profile = profile(&env);
        // Flat accounts still owe maintenance at withdrawal. Request the exact net
        // amount, so withdraw-all's fee adjustment cannot make a zero payout pass.
        let payout = DEPOSITS[4] + reward - 5 * MAINTENANCE;
        let (destination, withdraw_cu) = env.withdraw_with_cu(&owners[4], keeper, payout);
        total_maintenance += 5 * MAINTENANCE;
        insurance += 5 * MAINTENANCE;
        assert_eq!(total_maintenance, 115_000);
        expected[4] = 0;
        assert_eq!(values(&env, portfolios), expected);
        assert_eq!(env.token_amount(destination), payout as u64);
        assert_eq!(env.token_amount(env.vault), vault - payout as u64);
        assert_eq!(env.market_state().1.vault, u128::from(vault) - payout);
        assert_eq!(env.market_state().1.insurance, insurance);
        assert_eq!(profile(&env), before_payout_profile);
        assert_eq!(
            expected.iter().sum::<i128>() + insurance as i128 + payout as i128,
            i128::from(vault)
        );
        // These histories include maintenance and source-ledger cleanup, beyond the
        // basic-route CU benchmarks. Enforce the same SVM ceiling as cu_ix().
        for (route, cu) in routes.into_iter().zip(trade_cus) {
            assert_cu_within(&format!("interleaved cap {route:?}"), cu, 1_400_000);
        }
        assert_cu_within("interleaved cap crank", max_crank_cu, 1_400_000);
        assert_cu_within("interleaved cap payout", withdraw_cu, CUSTODY_CU_LIMIT);
        eprintln!("INV-045 reverse={reverse_routes}: trade_fees={total_trade_fees}, maintenance={total_maintenance}, liquidation_fee={liquidation_fee}, reward={reward}, payout={payout}, CU trades={trade_cus:?} crank={max_crank_cu} withdraw={withdraw_cu}");
        outcomes.push((
            closed_q,
            liquidation_fee,
            reward,
            expected,
            insurance,
            payout,
        ));
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "route order cannot change carry or entitlement"
    );
}
