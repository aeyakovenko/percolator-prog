//! INV-020/045, reopenings 426/425: a maximum canonical prefix is market progress,
//! not complete account evidence. Mixed-mode catchup and public trade interruptions
//! must preserve the next fractional price atom and its original owners' claims.
//! System/SPL/ATA/wrapper instructions construct all protocol and custody accounts;
//! only authenticated Clock and external Pyth fixtures are supplied by the harness.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const PRICE: u64 = 100;
const CAP_BPS: u64 = 24;
const DEPOSIT: u128 = 1_000_000;

fn funded_owner(env: &mut V16CuEnv, owner: &Keypair) -> (Pubkey, Pubkey) {
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
            DEPOSIT as u64,
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("public collateral mint");
    env.send(
        env.deposit_ix(portfolio.pubkey(), DEPOSIT),
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
    .expect("public collateral deposit");
    (portfolio.pubkey(), tokens)
}

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let market = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap())
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn refresh(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    report: Pubkey,
    reverse: bool,
    include_auth: bool,
) -> Result<u64, String> {
    let mut observations = crank_observations_with_accounts(0, 1);
    if include_auth {
        observations.extend(crank_observations(1));
    }
    if reverse {
        observations.reverse();
    }
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations,
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(report, false),
        ],
        &[],
    )
}

fn assert_prefix(env: &V16CuEnv, elapsed: u64, direction: i64) -> [u64; 2] {
    // The fixed public anchor earns 2,400 numerator units per logical slot.
    // 32 -> 64 -> 65 -> 66 -> 67 crosses a carry that a trade must not erase.
    let numerator = PRICE * CAP_BPS * elapsed;
    let movement = (numerator / 10_000) as i64;
    let expected = [
        (PRICE as i64 + direction * movement) as u64,
        (PRICE as i64 - direction * movement) as u64,
    ];
    let group = env.market_state().1;
    let profiles = profiles(env);
    for asset in 0..2 {
        assert_eq!(group.assets[asset].slot_last, elapsed);
        assert_eq!(group.assets[asset].effective_price, expected[asset]);
        assert_eq!(group.assets[asset].fund_px_last, PRICE);
        assert_eq!(group.assets[asset].f_long_num, 0);
        assert_eq!(group.assets[asset].f_short_num, 0);
        assert_eq!(
            u64::from(profiles[asset].price_move_remainder_bps_num),
            numerator % 10_000,
            "asset {asset}, elapsed {elapsed}: preserve the canonical fractional capacity"
        );
    }
    assert_eq!(profiles[0].mark_ewma_e6, expected[0]);
    assert_eq!(profiles[1].mark_ewma_e6, (100 - direction * 20) as u64);
    expected
}

fn assert_values(env: &V16CuEnv, portfolios: [Pubkey; 2], profit: i128, withdrawn: u128) {
    let expected = [DEPOSIT as i128 + profit, DEPOSIT as i128 - profit];
    let mut capital = 0;
    for (portfolio, value) in portfolios.into_iter().zip(expected) {
        let account = env.portfolio_state(portfolio);
        capital += account.capital.get();
        assert_eq!(
            account.capital.get() as i128 + account.pnl.get(),
            value - withdrawn as i128,
            "settled value belongs to this portfolio, not just the aggregate"
        );
    }
    let group = env.market_state().1;
    assert_eq!(group.c_tot, capital);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 2 * (DEPOSIT - withdrawn));
    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
}

#[test]
fn v16_program_chunked_mixed_observations_gate_full_refresh_and_preserve_claims() {
    let routes = [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ];
    let mut worlds = 0;
    let mut max_crank_cu = 0;
    let mut max_trade_cu = 0;
    for direction in [-1i64, 1] {
        for reverse in [false, true] {
            for rotation in 0..routes.len() {
                let label =
                    format!("direction={direction}, reverse={reverse}, rotation={rotation}");
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        initial_price: PRICE,
                        max_price_move_bps_per_slot: CAP_BPS,
                        max_accrual_dt_slots: 64,
                        min_funding_lifetime_slots: 64,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 0, 100);
                let feed = [0x95; 32];
                let initial = env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100);
                env.try_configure_hybrid_asset_with_conf_filter_cu(
                    0,
                    1,
                    0,
                    [feed, [0; 32], [0; 32]],
                    &[initial],
                    0,
                    100,
                    0,
                    0,
                    100,
                    0,
                )
                .expect("public fresh Hybrid configuration");
                env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);
                let owners = [Keypair::new(), Keypair::new()];
                let funded = owners.each_ref().map(|owner| funded_owner(&mut env, owner));
                let portfolios = funded.map(|(portfolio, _)| portfolio);
                let sources = funded.map(|(_, tokens)| tokens);
                let custody_keys = [env.mint, env.vault, sources[0], sources[1]];
                let custody = frame(&env, &custody_keys);
                for (asset, units) in [(0, 400i128), (1, -700)] {
                    env.trade_asset_with_cu(
                        asset,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        units * POS_SCALE as i128,
                        PRICE,
                        0,
                    );
                }
                let targets = [(100 + direction * 20) as u64, (100 - direction * 20) as u64];
                set_test_clock(&mut env, 0, 101);
                env.push_auth_mark_for_asset_as_admin(1, u64::MAX, targets[1]);
                let report = env.set_pyth_price_with_conf(&feed, targets[0] as i64, -6, 0, 101);
                refresh(&mut env, portfolios[0], report, reverse, true)
                    .expect("same-slot public target staging");
                assert_prefix(&env, 0, direction);
                assert_values(&env, portfolios, 0, 0);
                let original_accounts = portfolios.map(|key| env.svm.get_account(&key));

                set_test_clock(&mut env, 64, 102);
                let cu = refresh(&mut env, portfolios[0], report, reverse, true)
                    .unwrap_or_else(|error| panic!("{label}: maximum prefix: {error}"));
                max_crank_cu = max_crank_cu.max(cu);
                assert_cu_within(&label, cu, 1_375_000);
                assert_eq!(percolator::V16_MAX_ACCRUAL_PATH_STEPS, 32);
                assert_prefix(&env, 32, direction);
                // This successful instruction pays market debt, not either owner's stale leg.
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    original_accounts
                );
                for portfolio in portfolios {
                    let cert = health_cert(&env.portfolio_state(portfolio));
                    assert!(cert.valid);
                    assert!(cert.cert_oracle_epoch < env.market_state().1.oracle_epoch);
                }
                let tracked = [
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    env.mint,
                    env.vault,
                    sources[0],
                    sources[1],
                    initial,
                    report,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    env.admin.pubkey(),
                ];
                // Exclude only the network fee payer and runtime/program accounts.
                let immutable_keys = [
                    initial,
                    report,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    env.admin.pubkey(),
                ];
                let immutable = frame(&env, &immutable_keys);
                let before_omission = frame(&env, &tracked);
                let error = refresh(&mut env, portfolios[0], report, reverse, false)
                    .expect_err("finishing Hybrid alone cannot certify the pending AuthMark leg");
                assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                assert_eq!(
                    frame(&env, &tracked),
                    before_omission,
                    "the omitted observation must undo the real second Hybrid prefix"
                );

                let mut profit = i128::from(direction) * 15 * (400 + 700);
                let mut remaining_units = 400i128;
                let mut previous = [100 + direction * 15, 100 - direction * 15];
                for elapsed in 64..=67 {
                    if elapsed > 64 {
                        set_test_clock(&mut env, elapsed, 102 + (elapsed - 64) as i64);
                    }
                    for portfolio in portfolios {
                        let before = frame(&env, &tracked);
                        match refresh(&mut env, portfolio, report, reverse, true) {
                            Ok(cu) => {
                                max_crank_cu = max_crank_cu.max(cu);
                                assert_cu_within(&label, cu, 1_375_000);
                                assert_ne!(
                                    frame(&env, &tracked),
                                    before,
                                    "successful refresh must progress"
                                );
                            }
                            Err(error) => {
                                assert_eq!(portfolio, portfolios[1]);
                                assert!([65, 66].contains(&elapsed), "{label}: {error}");
                                assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                                assert_eq!(
                                    frame(&env, &tracked),
                                    before,
                                    "already-current peer retry must roll back exactly"
                                );
                            }
                        }
                    }
                    let accepted = assert_prefix(&env, elapsed, direction);
                    if elapsed > 64 {
                        profit += remaining_units * (accepted[0] as i128 - previous[0] as i128)
                            - 700 * (accepted[1] as i128 - previous[1] as i128);
                    }
                    previous = accepted.map(|price| price as i64);
                    assert_values(&env, portfolios, profit, 0);
                    for portfolio in portfolios {
                        let account = env.portfolio_state(portfolio);
                        let cert = health_cert(&account);
                        let group = env.market_state().1;
                        assert!(cert.valid);
                        assert_eq!(cert.cert_oracle_epoch, group.oracle_epoch);
                        assert_eq!(cert.cert_funding_epoch, group.funding_epoch);
                        // Peer settlement can age a conservative risk cache without
                        // changing the complete authenticated price/funding evidence.
                        assert!(cert.cert_risk_epoch <= group.risk_epoch);
                        assert_eq!(cert.cert_asset_set_epoch, group.asset_set_epoch);
                        assert_eq!(cert.active_bitmap_at_cert, active_bitmap(&account));
                        assert_eq!(cert.certified_liq_deficit, 0);
                    }
                    let before_trade = profiles(&env);
                    let route = routes[(rotation + (elapsed - 64) as usize) % routes.len()];
                    let cu = execute_account_residual_counter_trade_path(
                        &mut env,
                        route,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        -(100 * POS_SCALE as i128),
                        accepted[0],
                    );
                    max_trade_cu = max_trade_cu.max(cu);
                    assert_cu_within(&label, cu, TRADE_CU_LIMIT);
                    remaining_units -= 100;
                    assert_eq!(
                        profiles(&env),
                        before_trade,
                        "{label}: {route:?} changed oracle carry"
                    );
                    assert_prefix(&env, elapsed, direction);
                    assert_values(&env, portfolios, profit, 0);
                    assert_eq!(
                        env.market_state().1.assets[0].oi_eff_long_q,
                        remaining_units as u128 * POS_SCALE
                    );
                    assert_eq!(
                        env.market_state().1.assets[0].oi_eff_short_q,
                        remaining_units as u128 * POS_SCALE
                    );
                    assert_eq!(frame(&env, &custody_keys), custody);
                    assert_eq!(frame(&env, &immutable_keys), immutable);
                }
                assert_eq!(profit, i128::from(direction) * 17_300);
                assert_eq!(profiles(&env)[0].price_move_remainder_bps_num, 800);
                assert_ne!(
                    profit,
                    i128::from(direction) * 16_500,
                    "the next atom must actually change the surviving owners' claims"
                );

                // Catch the remaining AuthMark exposure all the way up before closing it.
                set_test_clock(&mut env, 84, 122);
                for portfolio in portfolios {
                    let cu = refresh(&mut env, portfolio, report, reverse, true)
                        .expect("bounded final target catchup");
                    max_crank_cu = max_crank_cu.max(cu);
                    assert_cu_within(&label, cu, 1_375_000);
                }
                for asset in 0..2 {
                    assert_eq!(
                        env.market_state().1.assets[asset].effective_price,
                        targets[asset]
                    );
                    assert_eq!(profiles(&env)[asset].price_move_remainder_bps_num, 0);
                }
                profit += i128::from(direction) * 4 * 700;
                assert_eq!(profit, i128::from(direction) * 20_100);
                assert_values(&env, portfolios, profit, 0);
                let cu = env.trade_asset_with_cu(
                    1,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    700 * POS_SCALE as i128,
                    targets[1],
                    0,
                );
                max_trade_cu = max_trade_cu.max(cu);
                assert_cu_within(&label, cu, TRADE_CU_LIMIT);
                for portfolio in portfolios {
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(
                        &env.portfolio_state(portfolio)
                    )));
                }
                for asset in &env.market_state().1.assets[..2] {
                    assert_eq!(asset.oi_eff_long_q, 0);
                    assert_eq!(asset.oi_eff_short_q, 0);
                }
                for i in 0..2 {
                    let cu = env
                        .send(
                            env.withdraw_ix(portfolios[i], 1_000),
                            vec![
                                AccountMeta::new(owners[i].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                                AccountMeta::new(sources[i], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[i]],
                        )
                        .expect("flat owner custody debit after complete observed exit");
                    assert_cu_within(&label, cu, CUSTODY_CU_LIMIT);
                    assert_eq!(env.token_amount(sources[i]), 1_000);
                }
                assert_values(&env, portfolios, profit, 1_000);
                assert_eq!(frame(&env, &immutable_keys), immutable);
                let mint = env.svm.get_account(&env.mint).unwrap();
                assert_eq!(
                    spl_token::state::Mint::unpack(&mint.data).unwrap().supply,
                    2 * DEPOSIT as u64
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("chunked mixed-oracle carry: {worlds} worlds, max crank {max_crank_cu} CU, max trade {max_trade_cu} CU");
}
