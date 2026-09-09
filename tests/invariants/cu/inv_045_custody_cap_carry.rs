//! INV-045 / row 425: non-position economic routes frame pending canonical carry.
//! Two unequal exposed assets cross their first price atoms while custody and fee
//! routes permute before/after public accrual. This is partial public-route evidence
//! for all-economic-routes-preserve-canonical-fractional-accrual-carry, not closure
//! of the trade-before-crank dimension owned by production-fix PR #425.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const ANCHORS: [u64; 2] = [100, 125];
const UNITS: [i128; 2] = [7, 11];
const CAPITAL: u128 = 1_000_000;
const WALLET: u64 = 1_000;
const CAP_BPS: u64 = 24;
const MAINTENANCE: u128 = 3;

#[derive(Clone, Copy, Debug)]
enum Route {
    Deposit,
    Withdraw,
    Maintenance,
    Insurance,
}

const ROUTES: [Route; 4] = [
    Route::Deposit,
    Route::Withdraw,
    Route::Maintenance,
    Route::Insurance,
];

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let market = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap())
}

fn mint_wallet(env: &mut V16CuEnv, owner: Pubkey, amount: u64) -> Pubkey {
    let tokens = create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint);
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
    .expect("public SPL mint");
    tokens
}

fn transfer(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    tokens: Pubkey,
    amount: u128,
    withdraw: bool,
) -> u64 {
    let mut accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new(tokens, false),
        AccountMeta::new(env.vault, false),
    ];
    if withdraw {
        accounts.push(AccountMeta::new_readonly(env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    let ix = if withdraw {
        env.withdraw_ix(portfolio, amount)
    } else {
        env.deposit_ix(portfolio, amount)
    };
    env.svm.expire_blockhash();
    env.send(ix, accounts, &[owner])
        .expect("public custody route")
}

fn assert_carry(env: &V16CuEnv, elapsed: u64, direction: i128) -> i128 {
    let group = env.market_state().1;
    let profiles = profiles(env);
    let mut long_pnl = 0;
    for asset in 0..2 {
        // Input-derived quotient/remainder, independent of engine/wrapper helpers.
        let numerator = ANCHORS[asset] * CAP_BPS * elapsed;
        let sign = if asset == 0 { direction } else { -direction };
        let movement = sign * i128::from(numerator / 10_000);
        let state = group.assets[asset];
        assert_eq!(state.slot_last, elapsed);
        assert_eq!(
            state.effective_price as i128,
            ANCHORS[asset] as i128 + movement
        );
        assert_eq!(state.fund_px_last, ANCHORS[asset]);
        assert_eq!(
            state.raw_oracle_target_price as i128,
            ANCHORS[asset] as i128 + sign * 20
        );
        assert_eq!(
            profiles[asset].price_move_remainder_bps_num as u64,
            numerator % 10_000
        );
        assert_eq!(state.oi_eff_long_q, UNITS[asset] as u128 * POS_SCALE);
        assert_eq!(state.oi_eff_short_q, UNITS[asset] as u128 * POS_SCALE);
        assert_eq!(state.f_long_num, 0);
        assert_eq!(state.f_short_num, 0);
        long_pnl += UNITS[asset] * movement;
    }
    long_pnl
}

#[test]
fn v16_program_custody_route_words_preserve_pending_fractional_carry() {
    let mut route_cu = [0; 4];
    let mut crank_cu = 0;
    let mut checked_routes = 0;
    let mut outcomes = Vec::new();
    for direction in [-1, 1] {
        let mut direction_outcomes = Vec::new();
        for rotation in 0..4 {
            for reverse in [false, true] {
                for crank_first in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            initial_price: ANCHORS[0],
                            max_price_move_bps_per_slot: CAP_BPS,
                            max_abs_funding_e9_per_slot: 0,
                            maintenance_fee_per_slot: MAINTENANCE,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(0);
                    for asset in 0..2 {
                        env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, ANCHORS[asset]);
                    }
                    let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
                    let mut portfolios = [Pubkey::default(); 3];
                    let mut tokens = [Pubkey::default(); 3];
                    for actor in 0..3 {
                        env.svm
                            .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                            .unwrap();
                        let portfolio = Keypair::new();
                        system_create_account_for_test(
                            &mut env.svm,
                            &env.payer,
                            &portfolio,
                            env.portfolio_account_len,
                            env.program_id,
                        );
                        portfolios[actor] = portfolio.pubkey();
                        env.send(
                            ProgInstruction::InitPortfolio,
                            vec![
                                AccountMeta::new(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolio.pubkey(), false),
                            ],
                            &[&owners[actor]],
                        )
                        .expect("public portfolio initialization");
                        env.portfolios.push(portfolio.pubkey());
                        tokens[actor] =
                            mint_wallet(&mut env, owners[actor].pubkey(), CAPITAL as u64 + WALLET);
                        transfer(
                            &mut env,
                            &owners[actor],
                            portfolio.pubkey(),
                            tokens[actor],
                            CAPITAL,
                            false,
                        );
                    }
                    let admin = env.admin.pubkey();
                    let admin_tokens = mint_wallet(&mut env, admin, WALLET);
                    for asset in 0..2 {
                        env.trade_asset_with_cu(
                            asset as u16,
                            &owners[0],
                            portfolios[0],
                            &owners[1],
                            portfolios[1],
                            UNITS[asset] * POS_SCALE as i128,
                            ANCHORS[asset],
                            0,
                        );
                        let sign = if asset == 0 { direction } else { -direction };
                        env.push_auth_mark_for_asset_as_admin(
                            asset as u16,
                            0,
                            (ANCHORS[asset] as i128 + sign * 20) as u64,
                        );
                    }
                    assert_eq!(assert_carry(&env, 0, direction), 0);
                    let mut flat_capital = CAPITAL;
                    let mut flat_tokens = WALLET;
                    let mut topups = 0;
                    let mut flat_fees = 0;
                    let mut accrued = 0;
                    for elapsed in 1..=5 {
                        env.svm.warp_to_slot(elapsed);
                        // The same word is run on both sides of the crank boundary.
                        // Custody never supplies a new price or consumes elapsed capacity.
                        for phase in 0..2 {
                            if (phase == 0) == crank_first {
                                for actor in 0..2 {
                                    env.svm.expire_blockhash();
                                    let cu = env.crank(
                                        portfolios[actor],
                                        ProgInstruction::PermissionlessCrank {
                                            now_slot: elapsed,
                                            observations: crank_observations_for_assets(&[0, 1]),
                                        },
                                    );
                                    crank_cu = crank_cu.max(cu);
                                    assert_carry(&env, elapsed, direction);
                                }
                                accrued = elapsed;
                            } else {
                                let mut fee_due = true;
                                for step in 0..4 {
                                    let index =
                                        (rotation + if reverse { 3 - step } else { step }) % 4;
                                    let route = ROUTES[index];
                                    let before = profiles(&env);
                                    let untouched =
                                        [0, 1].map(|actor| env.svm.get_account(&portfolios[actor]));
                                    env.svm.expire_blockhash();
                                    let cu = match route {
                                        Route::Deposit => {
                                            flat_capital += 17;
                                            flat_tokens -= 17;
                                            transfer(
                                                &mut env,
                                                &owners[2],
                                                portfolios[2],
                                                tokens[2],
                                                17,
                                                false,
                                            )
                                        }
                                        Route::Withdraw => {
                                            flat_capital -= 13;
                                            flat_tokens += 13;
                                            transfer(
                                                &mut env,
                                                &owners[2],
                                                portfolios[2],
                                                tokens[2],
                                                13,
                                                true,
                                            )
                                        }
                                        Route::Maintenance => env.sync_maintenance_fee_with_cu(
                                            portfolios[2],
                                            None,
                                            elapsed,
                                        ),
                                        Route::Insurance => {
                                            topups += 19;
                                            env.top_up_insurance_from_admin_token_with_cu(
                                                admin_tokens,
                                                19,
                                            )
                                        }
                                    };
                                    if fee_due
                                        && matches!(route, Route::Withdraw | Route::Maintenance)
                                    {
                                        flat_capital -= MAINTENANCE;
                                        flat_fees += MAINTENANCE;
                                        fee_due = false;
                                    }
                                    route_cu[index] = route_cu[index].max(cu);
                                    checked_routes += 1;
                                    assert_eq!(
                                        profiles(&env),
                                        before,
                                        "{route:?}: complete oracle profile frame"
                                    );
                                    assert_carry(&env, accrued, direction);
                                    assert_eq!(
                                        [0, 1].map(|actor| env.svm.get_account(&portfolios[actor])),
                                        untouched,
                                        "{route:?}: both exposed owners remain byte-identical"
                                    );
                                    assert_eq!(
                                        env.portfolio_state(portfolios[2]).capital.get(),
                                        flat_capital
                                    );
                                    assert_eq!(env.portfolio_state(portfolios[2]).pnl.get(), 0);
                                    assert_eq!(env.token_amount(tokens[2]), flat_tokens);
                                    assert_eq!(
                                        env.token_amount(admin_tokens),
                                        WALLET - topups as u64
                                    );
                                    let group = env.market_state().1;
                                    assert_eq!(
                                        group.insurance,
                                        topups + flat_fees + 2 * MAINTENANCE * accrued as u128
                                    );
                                    let capital: u128 = portfolios
                                        .map(|p| env.portfolio_state(p).capital.get())
                                        .iter()
                                        .sum();
                                    assert_eq!(group.c_tot, capital);
                                    let pnl: i128 = portfolios
                                        .map(|p| env.portfolio_state(p).pnl.get())
                                        .iter()
                                        .sum();
                                    assert_eq!(
                                        group.vault as i128,
                                        capital as i128 + pnl + group.insurance as i128
                                    );
                                    assert_eq!(
                                        group.vault as i128,
                                        3 * CAPITAL as i128 + i128::from(WALLET)
                                            - i128::from(flat_tokens)
                                            + topups as i128
                                    );
                                    assert_eq!(
                                        u128::from(env.token_amount(env.vault)),
                                        group.vault
                                    );
                                    let total_tokens = env.token_amount(env.vault)
                                        + env.token_amount(admin_tokens)
                                        + tokens.map(|t| env.token_amount(t)).iter().sum::<u64>();
                                    assert_eq!(total_tokens, 3 * CAPITAL as u64 + 4 * WALLET);
                                }
                                assert!(!fee_due, "the word includes a real fee debit");
                            }
                        }
                        let long_pnl = assert_carry(&env, elapsed, direction);
                        for actor in 0..2 {
                            let account = env.portfolio_state(portfolios[actor]);
                            let pnl = if actor == 0 { long_pnl } else { -long_pnl };
                            assert_eq!(
                                account.capital.get() as i128 + account.pnl.get(),
                                CAPITAL as i128 - (elapsed as u128 * MAINTENANCE) as i128 + pnl,
                                "input-derived settled entitlement for actor {actor}"
                            );
                            assert_eq!(env.token_amount(tokens[actor]), WALLET);
                        }
                    }
                    assert_eq!(assert_carry(&env, 5, direction), -4 * direction);
                    assert_eq!(
                        profiles(&env).map(|p| p.price_move_remainder_bps_num),
                        [2_000, 5_000]
                    );
                    direction_outcomes.push((
                        portfolios.map(|p| {
                            let a = env.portfolio_state(p);
                            a.capital.get() as i128 + a.pnl.get()
                        }),
                        env.market_state().1.insurance,
                        env.token_amount(env.vault),
                        tokens.map(|t| env.token_amount(t)),
                        env.token_amount(admin_tokens),
                    ));
                }
            }
        }
        assert!(
            direction_outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "route word and crank placement preserve per-owner entitlement and custody"
        );
        outcomes.extend(direction_outcomes);
    }
    assert_eq!(outcomes.len(), 32);
    assert_eq!(checked_routes, 640);
    for (route, cu) in ROUTES.into_iter().zip(route_cu) {
        assert!(cu > 0);
        assert_cu_within(&format!("custody carry {route:?}"), cu, CUSTODY_CU_LIMIT);
    }
    assert_cu_within("two-asset carry crank", crank_cu, 1_400_000);
    eprintln!("INV-045 custody carry: worlds={}, checked routes={checked_routes}, route CU={route_cu:?}, crank CU={crank_cu}", outcomes.len());
}
