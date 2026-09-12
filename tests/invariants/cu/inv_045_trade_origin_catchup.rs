//! INV-045 / row 422: paid Hybrid discovery through lagged/caught-up liquidation.
//! Compare raw prints, unequal stale reports and separate/combined market catchup.
//! Fresh-report handoff and its reward-eligibility transition remain outside this test.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const ENTRY: u64 = 1_000_000;
const ACCEPTED_PRINT: u64 = 990_400;
const MARK: u64 = 992_320;
const FUNDS: [u64; 5] = [5_100_000, 100_000_000, 10_000_000, 10_000_000, 1_000];

fn fund(env: &mut V16CuEnv, owner: &Keypair, amount: u64) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[owner],
    )
    .expect("public portfolio initialization");
    env.portfolios.push(portfolio.pubkey());
    let tokens = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &tokens,
            &env.admin.pubkey(),
            &[],
            amount,
        )
        .unwrap(),
        &[&env.admin],
    )
    .unwrap();
    env.send(
        env.deposit_ix(portfolio.pubkey(), amount as u128),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("public SPL deposit");
    (portfolio.pubkey(), tokens)
}

fn fee(q: u128, price: u64, bps: u128) -> u128 {
    ((q * u128::from(price)).div_ceil(POS_SCALE) * bps).div_ceil(10_000)
}

#[test]
fn v16_program_trade_origin_liquidation_prices_and_entitlements_survive_catchup_order() {
    let mut peak_crank = 0;
    let mut peak_trade = 0;
    let mut peak_withdraw = 0;
    for elapsed in [1u64, 4] {
        let price = (ENTRY - elapsed * 2_400).max(MARK);
        let mut reference = None;
        for raw_print in [980_000, 900_000] {
            for stale_price in [ENTRY, 1_100_000] {
                for publish_first in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            max_abs_funding_e9_per_slot: 0,
                            ..production_risk_params()
                        },
                    );
                    set_test_clock(&mut env, 1, 100);
                    env.update_liquidation_fee_policy_with_cu(10_000);
                    let feed = [0x47; 32];
                    let old_report = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        0,
                        1,
                        0,
                        [feed, [0; 32], [0; 32]],
                        &[old_report],
                        1,
                        100,
                        1,
                        0,
                        1,
                        0,
                    )
                    .expect("public Hybrid setup");
                    let stale_report =
                        env.set_pyth_price_with_conf(&feed, stale_price as i64, -6, 0, 101);
                    let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
                    let funded =
                        std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
                    let portfolios = funded.map(|pair| pair.0);
                    let tokens = funded.map(|pair| pair.1);
                    let [target, peer, trader_a, trader_b, keeper] = portfolios;
                    let values = |env: &V16CuEnv| {
                        portfolios.map(|key| {
                            let account = env.portfolio_state(key);
                            account.capital.get() as i128 + account.pnl.get()
                        })
                    };
                    let profile = |env: &V16CuEnv| {
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap()
                    };
                    let crank =
                        |env: &mut V16CuEnv, account: Pubkey, reward: bool, duplicate: bool| {
                            let mut accounts = vec![
                                AccountMeta::new(owners[4].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(account, false),
                            ];
                            accounts.push(AccountMeta::new_readonly(stale_report, false));
                            if reward {
                                accounts.push(AccountMeta::new(keeper, false));
                            }
                            let mut observations = crank_observations_with_accounts(0, 1);
                            if duplicate {
                                observations.extend(crank_observations(0));
                            }
                            env.svm.expire_blockhash();
                            env.send(
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: env.svm.get_sysvar::<Clock>().slot,
                                    observations,
                                },
                                accounts,
                                &[&owners[4]],
                            )
                        };
                    peak_trade = peak_trade.max(env.trade_asset_with_cu(
                        0,
                        &owners[0],
                        target,
                        &owners[1],
                        peer,
                        (100 * POS_SCALE) as i128,
                        ENTRY,
                        0,
                    ));
                    let vault = env.token_amount(env.vault);
                    assert_eq!(vault, FUNDS.iter().sum::<u64>());
                    let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));

                    // A clock-only prefix advances engine time without spending discovery age.
                    set_test_clock(&mut env, 5, 1_000);
                    peak_crank = peak_crank.max(crank(&mut env, keeper, false, false).unwrap());
                    assert_eq!(env.market_state().1.assets[0].slot_last, 5);
                    assert_eq!(profile(&env).mark_ewma_last_slot, 1);
                    let untouched = [target, peer, keeper].map(|key| env.svm.get_account(&key));
                    peak_trade = peak_trade.max(env.trade_asset_with_cu(
                        0,
                        &owners[2],
                        trader_a,
                        &owners[3],
                        trader_b,
                        POS_SCALE as i128,
                        raw_print,
                        0,
                    ));
                    assert_eq!(
                        [target, peer, keeper].map(|key| env.svm.get_account(&key)),
                        untouched
                    );
                    let staged = env.market_state().1;
                    assert_eq!(staged.assets[0].effective_price, ENTRY);
                    assert_eq!(staged.assets[0].raw_oracle_target_price, MARK);
                    assert_eq!(profile(&env).mark_ewma_e6, MARK);
                    assert_eq!(profile(&env).mark_ewma_last_slot, 5);
                    // Four elapsed slots accept 96 bps; alpha=4/5 leaves a 7,680-atom move.
                    // Existing 100-unit OI pays a bilateral 77-bps rounded externality.
                    let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
                    let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
                    let per_trader_fee = fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
                    let movement_fee = 2 * per_trader_fee;
                    assert!(movement_fee > 0);
                    assert_ne!(per_trader_fee, fee(POS_SCALE, raw_print, trade_bps));
                    assert_eq!(staged.insurance, movement_fee);
                    let mut expected = FUNDS.map(i128::from);
                    expected[2] -= per_trader_fee as i128;
                    expected[3] -= per_trader_fee as i128;
                    assert_eq!(values(&env), expected);
                    let budget_before = staged.insurance_domain_budget_remaining_total;
                    let domains_before = staged.insurance_domain_budget.clone();
                    let mut keys = vec![
                        env.market,
                        env.mint,
                        env.vault,
                        old_report,
                        stale_report,
                        env.admin.pubkey(),
                    ];
                    keys.extend(portfolios);
                    keys.extend(tokens);
                    keys.extend(owners.each_ref().map(Signer::pubkey));
                    let frame = |env: &V16CuEnv| -> Vec<_> {
                        keys.iter().map(|key| env.svm.get_account(key)).collect()
                    };
                    set_test_clock(&mut env, 5 + elapsed, 1_001);
                    let before_reject = frame(&env);
                    let error = crank(&mut env, target, true, true)
                        .expect_err("duplicate after valid catchup");
                    assert!(
                        error.contains(&format!(
                            "Custom({})",
                            PercolatorError::InvalidInstruction as u32
                        )),
                        "{error}"
                    );
                    assert_eq!(frame(&env), before_reject);
                    if publish_first {
                        peak_crank = peak_crank.max(crank(&mut env, keeper, false, false).unwrap());
                        assert_eq!(
                            values(&env),
                            expected,
                            "market catchup cannot redistribute owner claims"
                        );
                        assert_eq!(env.market_state().1.insurance, movement_fee);
                    }
                    let mut liquidation = None;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        let before_values = values(&env);
                        let foreign =
                            [peer, trader_a, trader_b, keeper].map(|key| env.svm.get_account(&key));
                        peak_crank = peak_crank.max(crank(&mut env, target, true, false).unwrap());
                        let after = env.market_state().1;
                        assert_eq!(after.assets[0].effective_price, price);
                        assert_eq!(after.assets[0].raw_oracle_target_price, MARK);
                        assert_eq!(profile(&env).mark_ewma_e6, MARK);
                        assert_eq!(profile(&env).last_good_oracle_slot, 1);
                        assert_eq!(profile(&env).oracle_target_publish_time, 100);
                        assert_eq!(
                            [peer, trader_a, trader_b, keeper].map(|key| env.svm.get_account(&key)),
                            foreign
                        );
                        assert_eq!(after.insurance_domain_budget_remaining_total, budget_before);
                        assert_eq!(after.insurance_domain_budget, domains_before);
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        assert_eq!(after.vault, u128::from(vault));
                        let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                        if closed == 0 {
                            assert_eq!(after.insurance, before.insurance);
                            continue;
                        }
                        let penalty = fee(closed, price, 5);
                        assert!(penalty > 0 && closed < 100 * POS_SCALE);
                        for wrong in [ENTRY, raw_print, ACCEPTED_PRINT, stale_price] {
                            assert_ne!(
                                penalty,
                                fee(closed, wrong, 5),
                                "price witness must distinguish {wrong}"
                            );
                        }
                        if elapsed == 1 {
                            assert_ne!(price, MARK);
                            assert_ne!(penalty, fee(closed, MARK, 5));
                        } else {
                            assert_eq!(price, MARK);
                        }
                        assert_eq!(before_values[0] - values(&env)[0], penalty as i128);
                        assert_eq!(after.insurance - before.insurance, penalty);
                        assert_eq!(
                            after.assets[0].oi_eff_long_q,
                            after.assets[0].oi_eff_short_q
                        );
                        assert_eq!(
                            health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                            0
                        );
                        liquidation = Some((closed, penalty));
                        break;
                    }
                    let (closed, penalty) = liquidation.expect("bounded nonzero liquidation");
                    for account in [peer, trader_a, trader_b] {
                        peak_crank =
                            peak_crank.max(crank(&mut env, account, false, false).unwrap());
                    }
                    let pnl = i128::from(ENTRY - price);
                    expected[0] -= 100 * pnl + penalty as i128;
                    expected[1] += 100 * pnl;
                    expected[2] -= pnl;
                    expected[3] += pnl;
                    assert_eq!(
                        values(&env),
                        expected,
                        "each owner retains its own PnL and fee debit"
                    );
                    assert_eq!(env.market_state().1.insurance, movement_fee + penalty);
                    env.svm.expire_blockhash();
                    peak_withdraw = peak_withdraw.max(
                        env.send(
                            env.withdraw_ix(keeper, FUNDS[4] as u128),
                            vec![
                                AccountMeta::new(owners[4].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(keeper, false),
                                AccountMeta::new(tokens[4], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[4]],
                        )
                        .expect("keeper withdraws exactly its original principal"),
                    );
                    expected[4] = 0;
                    assert_eq!(values(&env), expected);
                    assert_eq!(env.token_amount(tokens[4]), FUNDS[4]);
                    assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
                    assert_eq!(env.svm.get_account(&env.mint), custody[0]);
                    assert_eq!(env.token_amount(env.vault), vault - FUNDS[4]);
                    let group = env.market_state().1;
                    assert_eq!(group.vault, u128::from(vault - FUNDS[4]));
                    assert_eq!(
                        expected.iter().sum::<i128>() + group.insurance as i128,
                        group.vault as i128
                    );
                    assert_eq!(group.insurance_domain_budget_remaining_total, budget_before);
                    let outcome = (
                        closed,
                        penalty,
                        expected,
                        group.insurance,
                        group.vault,
                        domains_before,
                    );
                    if let Some(reference) = &reference {
                        assert_eq!(
                            &outcome, reference,
                            "raw print, stale report price and catchup order commute"
                        );
                    } else {
                        reference = Some(outcome);
                    }
                }
            }
        }
        let (closed, penalty, ..) = reference.expect("eight equivalent histories per price");
        println!("elapsed={elapsed} price={price} closed={closed} penalty={penalty}");
    }
    assert_cu_within(
        "trade-origin catchup/liquidation",
        peak_crank,
        CRANK_CU_LIMIT,
    );
    assert_cu_within("paid trade-origin mark", peak_trade, TRADE_CU_LIMIT);
    assert_cu_within(
        "keeper principal withdrawal",
        peak_withdraw,
        CUSTODY_CU_LIMIT,
    );
    println!(
        "trade-origin worlds=16 crank={peak_crank} trade={peak_trade} withdrawal={peak_withdraw}"
    );
}
