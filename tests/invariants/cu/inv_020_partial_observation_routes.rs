//! INV-020: three-leg partial observation followed by single/batch owner reduction.
//! Public construction only; decoded copies and Account snapshots are read-only oracles.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};

const PRICES: [u64; 3] = [1_040_000, 1_050_000, 960_000];
const PRINCIPAL: u128 = 10_000_000;
const SUPPLY: u64 = (2 * PRINCIPAL + 1_000) as u64;

fn crank(env: &mut V16CuEnv, target: Pubkey, report: Pubkey, order: &[u16]) -> u64 {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    let observations = order
        .iter()
        .map(|&asset_index| {
            if asset_index == 0 {
                accounts.push(AccountMeta::new_readonly(report, false));
            }
            CrankObservationHint {
                asset_index,
                oracle_accounts: u8::from(asset_index == 0),
            }
        })
        .collect();
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations,
            },
            accounts,
            &[],
        )
        .expect("public observation continuation");
    assert_cu_within("three-leg observation", cu, 500_000);
    cu
}

fn census(env: &V16CuEnv, portfolios: [Pubkey; 3]) {
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    assert_market_stock_census(
        "partial-observation routes",
        &group,
        &market_data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("partial-observation routes", &group, &accounts).unwrap();
    let (_, market) = state::market_view_mut(&mut market_data).unwrap();
    market.validate_shape().unwrap();
    for portfolio in portfolios {
        let mut data = env.svm.get_account(&portfolio).unwrap().data;
        state::portfolio_view_mut_for_market_slots(&mut data, 3)
            .unwrap()
            .validate_with_market(&market.as_view())
            .unwrap();
    }
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, SUPPLY);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, u128::from(SUPPLY));
}

fn reduce_legs(
    env: &mut V16CuEnv,
    owners: &[Keypair; 3],
    portfolios: [Pubkey; 3],
    taker: usize,
    assets: &[u16],
    batch: bool,
) -> u64 {
    let maker = 1 - taker;
    let size_q = if taker == 0 { -1 } else { 1 } * POS_SCALE as i128;
    let ix = if batch {
        env.batch_trade_no_cpi_ix(
            portfolios[taker],
            portfolios[maker],
            assets
                .iter()
                .map(|&asset_index| BatchTradeLeg {
                    asset_index,
                    market_id: env.asset_market_id(asset_index),
                    size_q,
                    exec_price: PRICES[asset_index as usize],
                    fee_bps: 0,
                })
                .collect(),
        )
    } else {
        assert_eq!(assets.len(), 1);
        env.trade_no_cpi_ix(
            portfolios[taker],
            portfolios[maker],
            assets[0],
            size_q,
            PRICES[assets[0] as usize],
            0,
        )
    };
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ix,
            vec![
                AccountMeta::new(owners[taker].pubkey(), true),
                AccountMeta::new(owners[maker].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[taker], false),
                AccountMeta::new(portfolios[maker], false),
            ],
            &[&owners[taker], &owners[maker]],
        )
        .expect("fully observed owner reduction");
    assert_cu_within("three-leg single/batch reduction", cu, 750_000);
    census(env, portfolios);
    cu
}

fn current_values(env: &V16CuEnv, portfolios: [Pubkey; 3], remaining: &[u16]) -> [(u128, i128); 2] {
    let group = env.market_state().1;
    let notional: u128 = remaining
        .iter()
        .map(|&i| u128::from(PRICES[i as usize]))
        .sum();
    let gain = i128::from(PRICES[0] - PRICE) + i128::from(PRICES[1] - PRICE);
    let loss = i128::from(PRICE - PRICES[2]);
    let expected_values = [
        PRINCIPAL as i128 + gain - loss,
        PRINCIPAL as i128 - gain + loss,
    ];
    let values =
        [0, 1].map(|i| {
            let account = env.portfolio_state(portfolios[i]);
            assert!(
            assert_current_certificate_matches_independent(
                "three-leg reduction certificate",
                &group,
                &account,
            )
            .unwrap(),
            "owner {i}, remaining {remaining:?}: cert={:?}, epochs={:?}, stale={:?}, value={:?}",
            health_cert(&account),
            (group.oracle_epoch, group.funding_epoch, group.risk_epoch, group.asset_set_epoch),
            (account.stale_state, account.b_stale_state),
            (account.capital.get(), account.pnl.get()),
        );
            let cert = health_cert(&account);
            assert_eq!(cert.certified_initial_req, notional / 10);
            assert_eq!(cert.certified_maintenance_req, notional / 10);
            assert_eq!(cert.certified_worst_case_loss, notional);
            assert_eq!(cert.certified_liq_deficit, 0);
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get(),
                expected_values[i]
            );
            for asset in 0..3 {
                assert_eq!(
                    has_active_leg_for_asset(&account, asset),
                    remaining.contains(&(asset as u16))
                );
                if remaining.contains(&(asset as u16)) {
                    assert_eq!(
                        active_leg_for_asset(&account, asset).basis_pos_q,
                        if i == 0 {
                            POS_SCALE as i128
                        } else {
                            -(POS_SCALE as i128)
                        }
                    );
                }
            }
            (account.capital.get(), account.pnl.get())
        });
    for (i, asset) in group.assets[..3].iter().enumerate() {
        let oi = if remaining.contains(&(i as u16)) {
            POS_SCALE
        } else {
            0
        };
        assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (oi, oi));
        assert_eq!(
            (
                asset.effective_price,
                asset.raw_oracle_target_price,
                asset.slot_last
            ),
            (PRICES[i], PRICES[i], 65)
        );
        assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
    }
    values
}

#[test]
fn v16_program_partial_observation_three_leg_reductions_match_single_and_batch() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut reference = None;
    for taker in 0..2 {
        for reverse in [false, true] {
            for explicit in [true, false] {
                for batch in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            max_portfolio_assets: 3,
                            initial_price: PRICE,
                            min_nonzero_mm_req: 599,
                            min_nonzero_im_req: 600,
                            maintenance_margin_bps: 1_000,
                            initial_margin_bps: 1_000,
                            max_price_move_bps_per_slot: 10,
                            max_accrual_dt_slots: 65,
                            min_funding_lifetime_slots: 65,
                            ..V16CuMarketParams::default()
                        },
                    );
                    set_test_clock(&mut env, 0, 100);
                    let feed = [0xab; 32];
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
                    .expect("public Hybrid configuration");
                    for asset in 1..3 {
                        env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
                    }
                    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                    let funded = [PRINCIPAL, PRINCIPAL, 1_000]
                        .into_iter()
                        .enumerate()
                        .map(|(i, amount)| funded_owner(&mut env, &owners[i], amount))
                        .collect::<Vec<_>>();
                    let portfolios = [0, 1, 2].map(|i| funded[i].0);
                    let tokens = [0, 1, 2].map(|i| funded[i].1);
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
                    for asset in 0..3 {
                        env.trade_asset_with_cu(
                            asset,
                            &owners[0],
                            portfolios[0],
                            &owners[1],
                            portfolios[1],
                            POS_SCALE as i128,
                            PRICE,
                            0,
                        );
                    }
                    census(&env, portfolios);
                    set_test_clock(&mut env, 0, 101);
                    for asset in 1..3 {
                        env.push_auth_mark_for_asset_as_admin(
                            asset,
                            u64::MAX,
                            PRICES[asset as usize],
                        );
                    }
                    let report = env.set_pyth_price_with_conf(&feed, PRICES[0] as i64, -6, 0, 101);
                    let order = if reverse { [2, 1, 0] } else { [0, 1, 2] };
                    peak = peak.max(crank(&mut env, portfolios[2], report, &order));
                    let accounts = frame(&env, &portfolios);
                    set_test_clock(&mut env, 64, 102);
                    peak = peak.max(crank(&mut env, portfolios[2], report, &order));
                    assert_eq!(frame(&env, &portfolios), accounts);
                    assert!(env.market_state().1.assets[..3]
                        .iter()
                        .all(|a| a.slot_last == 32));
                    census(&env, portfolios);

                    // The omitted AuthMark siblings retain pending accrual. Hybrid discovery
                    // can progress, but it must leave both owners and their old certificates alone.
                    set_test_clock(&mut env, 65, 103);
                    peak = peak.max(crank(&mut env, portfolios[2], report, &[0]));
                    let partial = env.market_state().1;
                    assert_eq!([0, 1, 2].map(|i| partial.assets[i].slot_last), [64, 32, 32]);
                    assert_eq!(frame(&env, &portfolios), accounts);
                    for portfolio in &portfolios[..2] {
                        assert!(
                            health_cert(&env.portfolio_state(*portfolio)).cert_oracle_epoch
                                < partial.oracle_epoch
                        );
                    }
                    census(&env, portfolios);

                    for expected_debt in [2, 0] {
                        let before: u64 = env.market_state().1.assets[..3]
                            .iter()
                            .map(|a| 65 - a.slot_last)
                            .sum();
                        peak = peak.max(crank(&mut env, portfolios[2], report, &order));
                        let after: u64 = env.market_state().1.assets[..3]
                            .iter()
                            .map(|a| 65 - a.slot_last)
                            .sum();
                        assert_eq!(after, expected_debt);
                        assert!(after < before);
                        assert_eq!(frame(&env, &portfolios[..2]), accounts[..2]);
                        census(&env, portfolios);
                    }
                    let custody_keys = [
                        env.vault,
                        env.mint,
                        tokens[0],
                        tokens[1],
                        tokens[2],
                        initial,
                        report,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        owners[2].pubkey(),
                        env.admin.pubkey(),
                        solana_sdk::sysvar::clock::ID,
                    ];
                    let custody = frame(&env, &custody_keys);
                    if explicit {
                        // The long's loss settlement advances source-credit risk epochs,
                        // so the short needs recertification after that peer settlement.
                        for i in [1, 0, 1] {
                            peak = peak.max(crank(&mut env, portfolios[i], report, &order));
                            census(&env, portfolios);
                        }
                        current_values(&env, portfolios, &[0, 1, 2]);
                    }
                    if batch {
                        peak = peak.max(reduce_legs(
                            &mut env,
                            &owners,
                            portfolios,
                            taker,
                            &[0, 1],
                            true,
                        ));
                    } else {
                        peak = peak.max(reduce_legs(
                            &mut env,
                            &owners,
                            portfolios,
                            taker,
                            &[0],
                            false,
                        ));
                        current_values(&env, portfolios, &[1, 2]);
                        peak = peak.max(reduce_legs(
                            &mut env,
                            &owners,
                            portfolios,
                            taker,
                            &[1],
                            false,
                        ));
                    }
                    let reduced = current_values(&env, portfolios, &[2]);
                    // Close the untraded loss leg through the other transport to check its
                    // attributed value survives the earlier two-leg reduction.
                    peak = peak.max(reduce_legs(
                        &mut env,
                        &owners,
                        portfolios,
                        1 - taker,
                        &[2],
                        !batch,
                    ));
                    let closed = current_values(&env, portfolios, &[]);
                    assert_eq!(frame(&env, &custody_keys), custody);
                    assert_eq!(env.portfolio_state(portfolios[2]).capital.get(), 1_000);
                    let result = (reduced, closed);
                    if let Some(expected) = reference {
                        assert_eq!(
                            result, expected,
                            "taker={taker}, reverse={reverse}, explicit={explicit}, batch={batch}"
                        );
                    } else {
                        reference = Some(result);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("partial-observation reduction routes: {worlds} public worlds, peak {peak} CU; no injected economic state");
}
