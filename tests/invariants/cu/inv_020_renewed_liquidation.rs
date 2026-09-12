//! INV-020 / row426: renewed observations must reauthorize each liquidation episode.
//! Public construction only; full-refresh oracles operate on detached snapshots.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_snapshot_full_refresh, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn crank_ix(
    env: &V16CuEnv,
    portfolios: [Pubkey; 3],
    keeper: &Keypair,
    report: Pubkey,
    assets: &[u16],
    caller_slot: u64,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(keeper.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolios[1], false),
    ];
    let observations = assets
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
    accounts.push(AccountMeta::new(portfolios[2], false));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: caller_slot,
            observations,
        }
        .encode(),
    }
}

#[track_caller]
fn submit(
    env: &mut V16CuEnv,
    keeper: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[&[heap_ix(), cu_ix()][..], instructions].concat(),
        Some(&env.payer.pubkey()),
        &[&env.payer, keeper],
        env.svm.latest_blockhash(),
    );
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some((index, error)) = rejection {
        let failure = result.expect_err("incomplete renewed evidence must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            usize::from(index - 2),
            "the expected public prefix must actually succeed"
        );
        for (key, account) in keys.iter().zip(before) {
            assert_eq!(env.svm.get_account(key), account, "exact rollback {key}");
        }
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("public renewal continuation")
            .compute_units_consumed
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert_cu_within("renewed liquidation transaction", cu, 2 * CRANK_CU_LIMIT);
    cu
}

fn audit(env: &V16CuEnv, portfolios: [Pubkey; 3]) -> [bool; 3] {
    let market = env.svm.get_account(&env.market).unwrap();
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "renewed liquidation",
        &group,
        &market.data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("renewed liquidation", &group, &accounts).unwrap();
    portfolios.map(|key| {
        assert_current_certificate_matches_snapshot_full_refresh(
            "renewed liquidation",
            &market.data,
            &env.svm.get_account(&key).unwrap().data,
        )
        .expect("every current certificate must agree with detached full refresh")
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    assets: Vec<percolator::AssetStateV16>,
    certificates: [percolator::HealthCertV16; 3],
    value: [(u128, i128, u64); 3],
    stock: [u128; 4],
}

fn checkpoint(env: &V16CuEnv, portfolios: [Pubkey; 3], tokens: [Pubkey; 3]) -> Checkpoint {
    let group = env.market_state().1;
    Checkpoint {
        assets: group.assets,
        certificates: portfolios.map(|key| health_cert(&env.portfolio_state(key))),
        value: std::array::from_fn(|i| {
            let account = env.portfolio_state(portfolios[i]);
            (
                account.capital.get(),
                account.pnl.get(),
                env.token_amount(tokens[i]),
            )
        }),
        stock: [group.c_tot, group.insurance, group.vault, group.pnl_pos_tot],
    }
}

#[test]
fn v16_program_renewed_observations_preserve_repeated_liquidation_certificates() {
    let mut reference = None;
    let mut worlds = 0;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    let mut certificate_checks = 0;
    let mut peak_cu = 0;
    for reverse in [false, true] {
        for caller_slot in [0, u64::MAX] {
            for interrupted in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        initial_price: PRICE,
                        min_nonzero_mm_req: 599,
                        min_nonzero_im_req: 600,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 10,
                        max_accrual_dt_slots: 64,
                        min_funding_lifetime_slots: 64,
                        liquidation_fee_bps: 100,
                        liquidation_fee_cap: 10_000,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 0, 100);
                env.update_liquidation_fee_policy_with_cu(5_000);
                let feed = [0xc6; 32];
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
                .unwrap();
                env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);
                let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                let funded = [0, 1, 2].map(|i| funded_owner(&mut env, &owners[i], DEPOSITS[i]));
                let portfolios = funded.map(|(key, _)| key);
                let tokens = funded.map(|(_, key)| key);
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
                for asset in 0..2 {
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
                    certificate_checks +=
                        audit(&env, portfolios).into_iter().filter(|x| *x).count();
                }
                let mut tracked = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    initial,
                    env.admin.pubkey(),
                    solana_sdk::sysvar::clock::ID,
                ];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(|owner| owner.pubkey()));
                let custody = frame(
                    &env,
                    &[env.mint, env.vault, tokens[0], tokens[1], tokens[2]],
                );
                let order = if reverse { [1, 0] } else { [0, 1] };
                let mut history = Vec::new();
                let mut previous_report = initial;
                let mut total_penalty = 0;
                let mut total_reward = 0;
                for episode in 1..=2u64 {
                    let start = 64 * (episode - 1);
                    let prices = [PRICE + episode * 40_000, PRICE + episode * 50_000];
                    let timestamp = 100 + episode as i64;
                    set_test_clock(&mut env, start, timestamp);
                    env.push_auth_mark_for_asset_as_admin(1, caller_slot, prices[1]);
                    let report =
                        env.set_pyth_price_with_conf(&feed, prices[0] as i64, -6, 0, timestamp);
                    tracked.push(report);
                    let full = crank_ix(&env, portfolios, &owners[2], report, &order, caller_slot);
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        &owners[2],
                        &[full.clone()],
                        &tracked,
                        None,
                    ));
                    certificate_checks +=
                        audit(&env, portfolios).into_iter().filter(|x| *x).count();
                    history.push(checkpoint(&env, portfolios, tokens));
                    let prefix_accounts = frame(&env, &portfolios);
                    set_test_clock(&mut env, start + 64, timestamp);
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        &owners[2],
                        &[full.clone()],
                        &tracked,
                        None,
                    ));
                    assert_eq!(
                        frame(&env, &portfolios),
                        prefix_accounts,
                        "bounded market work cannot liquidate"
                    );
                    for asset in &env.market_state().1.assets[..2] {
                        assert_eq!(asset.slot_last, start + 32);
                        assert_eq!(asset.f_long_num, 0);
                        assert_eq!(asset.f_short_num, 0);
                    }
                    assert!(
                        !audit(&env, portfolios)[1],
                        "a partial path cannot recertify the target"
                    );
                    history.push(checkpoint(&env, portfolios, tokens));
                    if interrupted {
                        let duplicate = [order[0], order[1], order[0]];
                        for (assets, evidence, error) in [
                            (&order[..], previous_report, PercolatorError::OracleStale),
                            (&duplicate[..], report, PercolatorError::InvalidInstruction),
                            (&[0][..], report, PercolatorError::EngineNonProgress),
                            (&[][..], report, PercolatorError::EngineNonProgress),
                        ] {
                            let ix = crank_ix(
                                &env,
                                portfolios,
                                &owners[2],
                                evidence,
                                assets,
                                caller_slot,
                            );
                            peak_cu = peak_cu.max(submit(
                                &mut env,
                                &owners[2],
                                &[ix],
                                &tracked,
                                Some((2, error)),
                            ));
                            rollbacks += 1;
                        }
                    }
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        &owners[2],
                        &[full.clone()],
                        &tracked,
                        None,
                    ));
                    let current = audit(&env, portfolios);
                    assert!(
                        current[1],
                        "renewed complete evidence must produce a current target certificate"
                    );
                    certificate_checks += current.into_iter().filter(|x| *x).count();
                    for (asset, price) in env.market_state().1.assets[..2].iter().zip(prices) {
                        assert_eq!(
                            (asset.slot_last, asset.effective_price),
                            (start + 64, price)
                        );
                    }
                    let mut deficit =
                        health_cert(&env.portfolio_state(portfolios[1])).certified_liq_deficit;
                    assert!(
                        deficit > 0,
                        "episode {episode} must independently require liquidation"
                    );
                    history.push(checkpoint(&env, portfolios, tokens));
                    let mut actions = 0;
                    while deficit > 0 {
                        assert!(
                            actions < 4,
                            "two-leg liquidation must make bounded progress"
                        );
                        let before = env.market_state().1;
                        let equity = env.portfolio_state(portfolios[1]).capital.get();
                        let position_epoch = env.portfolio_position_epoch(portfolios[1]);
                        if interrupted {
                            let stale = crank_ix(
                                &env,
                                portfolios,
                                &owners[2],
                                previous_report,
                                &order,
                                caller_slot,
                            );
                            peak_cu = peak_cu.max(submit(
                                &mut env,
                                &owners[2],
                                &[full.clone(), stale],
                                &tracked,
                                Some((3, PercolatorError::OracleStale)),
                            ));
                            rollbacks += 1;
                        }
                        // Empty observations are authorized only after complete renewal in this slot.
                        let action = crank_ix(
                            &env,
                            portfolios,
                            &owners[2],
                            report,
                            if interrupted { &[] } else { &order },
                            caller_slot,
                        );
                        peak_cu =
                            peak_cu.max(submit(&mut env, &owners[2], &[action], &tracked, None));
                        assert_eq!(
                            env.portfolio_position_epoch(portfolios[1]),
                            position_epoch + 1
                        );
                        let after = env.market_state().1;
                        let penalty = equity - env.portfolio_state(portfolios[1]).capital.get();
                        assert!(penalty > 0 && penalty <= 10_000);
                        total_penalty += penalty;
                        total_reward += penalty / 2;
                        assert_eq!(after.insurance, total_penalty - total_reward);
                        assert_eq!(
                            env.portfolio_state(portfolios[2]).capital.get(),
                            DEPOSITS[2] + total_reward
                        );
                        assert_eq!(
                            after.c_tot + after.insurance,
                            before.c_tot + before.insurance
                        );
                        let current = audit(&env, portfolios);
                        assert!(current[1]);
                        certificate_checks += current.into_iter().filter(|x| *x).count();
                        let remaining =
                            health_cert(&env.portfolio_state(portfolios[1])).certified_liq_deficit;
                        assert!(
                            remaining < deficit,
                            "each bounded action reduces the independently checked deficit"
                        );
                        deficit = remaining;
                        assert_eq!(
                            frame(
                                &env,
                                &[env.mint, env.vault, tokens[0], tokens[1], tokens[2]]
                            ),
                            custody
                        );
                        history.push(checkpoint(&env, portfolios, tokens));
                        actions += 1;
                        liquidations += 1;
                    }
                    // Publicly settle the opposing cohort after unilateral reductions.
                    let mut peer_refresh = full.clone();
                    peer_refresh.accounts[2].pubkey = portfolios[0];
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        &owners[2],
                        &[peer_refresh],
                        &tracked,
                        None,
                    ));
                    let current = audit(&env, portfolios);
                    assert!(current[0]);
                    certificate_checks += current.into_iter().filter(|x| *x).count();
                    history.push(checkpoint(&env, portfolios, tokens));
                    previous_report = report;
                }
                let payout = DEPOSITS[2] + total_reward;
                let withdrawal = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[2].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[2], false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(portfolios[2], payout).encode(),
                };
                peak_cu = peak_cu.max(submit(&mut env, &owners[2], &[withdrawal], &tracked, None));
                assert_eq!(u128::from(env.token_amount(tokens[2])), payout);
                assert_eq!(env.portfolio_state(portfolios[2]).capital.get(), 0);
                assert_eq!(env.token_amount(tokens[0]), 0);
                assert_eq!(env.token_amount(tokens[1]), 0);
                assert_eq!(env.market_state().1.insurance, total_penalty - total_reward);
                assert_eq!(
                    u128::from(env.token_amount(env.vault)) + payout,
                    DEPOSITS.iter().sum()
                );
                certificate_checks += audit(&env, portfolios).into_iter().filter(|x| *x).count();
                history.push(checkpoint(&env, portfolios, tokens));
                if let Some(expected) = &reference {
                    assert_eq!(&history, expected,
                        "every renewal/prefix/refresh/liquidation agrees: reverse={reverse}, caller={caller_slot}, interrupted={interrupted}");
                } else {
                    reference = Some(history);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert!(
        liquidations > 16,
        "later episodes must exercise multiple liquidation steps"
    );
    assert_eq!(rollbacks, 32 + liquidations / 2);
    assert!(certificate_checks >= 64);
    println!("row426 renewed liquidation: {worlds} worlds, 16 episodes, {liquidations} liquidation steps, {rollbacks} exact rollbacks, {certificate_checks} full-refresh comparisons; peak={peak_cu} CU");
}
