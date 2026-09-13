//! INV-020 / row426: an observation-authorized liquidation reward and its SPL payout
//! must roll back together when a later authenticated observation rejects. The same
//! withdrawal intent must remain usable after public recertification and pay once.
//! Unlike renewed_liquidation and active/CPI keeper histories, the aborted prefix
//! includes the actual reward withdrawal, custody transfer, and sequence consumption.
//! Four independently constructed public worlds compare separate and bundled routes.
//! Clock/Pyth are external fixtures; no program-owned Account is installed or replayed.
//! Limits: one two-leg liquidation, flat recipient, zero funding/maintenance/trade
//! fees, unchanged provider/authority, and finite shapes. Row426 remains OPEN.

use super::*;

fn payout_ix(
    env: &V16CuEnv,
    owner: &Keypair,
    keeper: Pubkey,
    token: Pubkey,
    amount: u128,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(keeper, amount).encode(),
    }
}

#[track_caller]
fn abort_after_payout(
    env: &mut V16CuEnv,
    owner: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
) -> u64 {
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[&[heap_ix(), cu_ix()][..], instructions].concat(),
        Some(&env.payer.pubkey()),
        &[&env.payer, owner],
        env.svm.latest_blockhash(),
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("late stale observation");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            (instructions.len() + 1) as u8,
            InstructionError::Custom(PercolatorError::OracleStale as u32),
        ),
        "{failure:?}"
    );
    for (program, successes) in [(env.program_id, instructions.len() - 1), (spl_token::ID, 1)] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            successes,
            "refresh/liquidation/withdrawal and SPL transfer must succeed before rejection"
        );
    }
    for (key, account) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            assert_eq!(env.svm.get_account(key), Some(payer.clone()));
        } else {
            assert_eq!(
                env.svm.get_account(key),
                account,
                "complete Account rollback {key}"
            );
        }
    }
    let cu = failure.meta.compute_units_consumed;
    assert_cu_within("observation rollback after reward SPL payout", cu, 900_000);
    cu
}

#[test]
fn v16_program_observation_abort_restores_liquidation_reward_payout_and_intent() {
    let mut reference = None;
    let mut expected_payout = None;
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut worlds = 0;
    let mut certificate_checks = 0;
    for reverse in [false, true] {
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
            let feed = [0xd6; 32];
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
            let [long, short, keeper] = portfolios;
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
                    long,
                    &owners[1],
                    short,
                    POS_SCALE as i128,
                    PRICE,
                    0,
                );
            }
            let sequence = env.portfolio_matcher_sequence(keeper);
            let position_epoch = env.portfolio_position_epoch(short);
            let order = if reverse { [1, 0] } else { [0, 1] };
            set_test_clock(&mut env, 0, 101);
            env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
            let report = env.set_pyth_price_with_conf(&feed, CURRENT[0] as i64, -6, 0, 101);
            let mut tracked = vec![
                env.market,
                env.mint,
                env.vault,
                initial,
                report,
                env.admin.pubkey(),
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(|owner| owner.pubkey()));
            let full = crank_ix(&env, portfolios, &owners[2], report, &order, u64::MAX);
            let action = crank_ix(&env, portfolios, &owners[2], report, &[], u64::MAX);
            let stale = crank_ix(&env, portfolios, &owners[2], initial, &order, u64::MAX);
            peak_cu = peak_cu.max(submit(
                &mut env,
                &owners[2],
                &[full.clone()],
                &tracked,
                None,
            ));
            let accounts_before_catchup = frame(&env, &portfolios);
            set_test_clock(&mut env, 64, 101);
            peak_cu = peak_cu.max(submit(
                &mut env,
                &owners[2],
                &[full.clone()],
                &tracked,
                None,
            ));
            assert_eq!(frame(&env, &portfolios), accounts_before_catchup);
            for asset in &env.market_state().1.assets[..2] {
                assert_eq!(asset.slot_last, 32, "real multi-step catchup prefix");
            }
            assert!(!audit(&env, portfolios)[1]);
            assert_eq!(env.portfolio_position_epoch(short), position_epoch);
            assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
            let mut history = vec![checkpoint(&env, portfolios, tokens)];

            // The clean world measures the payout. Interrupted worlds retain its exact
            // amount and their own current intent bytes before the reward exists.
            let retained = expected_payout
                .map(|amount| payout_ix(&env, &owners[2], keeper, tokens[2], amount));
            if interrupted {
                let withdrawal = retained.as_ref().unwrap();
                peak_cu = peak_cu.max(abort_after_payout(
                    &mut env,
                    &owners[2],
                    &[
                        full.clone(),
                        action.clone(),
                        withdrawal.clone(),
                        stale.clone(),
                    ],
                    &tracked,
                ));
                rollbacks += 1;
                assert_eq!(checkpoint(&env, portfolios, tokens), history[0]);
                assert_eq!(env.portfolio_matcher_sequence(keeper), sequence);
                assert!(
                    !audit(&env, portfolios)[1],
                    "aborted refresh cannot leave current health behind"
                );
            }

            peak_cu = peak_cu.max(submit(
                &mut env,
                &owners[2],
                &[full.clone()],
                &tracked,
                None,
            ));
            let current = audit(&env, portfolios);
            assert!(current[1]);
            certificate_checks += current.into_iter().filter(|current| *current).count();
            assert_current_short(&env, short, 130_000, 209_000);
            assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
            assert_eq!(env.portfolio_matcher_sequence(keeper), sequence);
            history.push(checkpoint(&env, portfolios, tokens));
            let capital = env.portfolio_state(short).capital.get();
            let stock = env.market_state().1;

            let withdrawal = if interrupted {
                let withdrawal = retained.unwrap();
                // Recovery crosses the transaction boundary: observations were committed
                // above, so the identical no-tail liquidation and intent can now run.
                peak_cu = peak_cu.max(abort_after_payout(
                    &mut env,
                    &owners[2],
                    &[action.clone(), withdrawal.clone(), stale],
                    &tracked,
                ));
                rollbacks += 1;
                assert_eq!(checkpoint(&env, portfolios, tokens), history[1]);
                assert_eq!(env.portfolio_matcher_sequence(keeper), sequence);
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[2],
                    &[action, withdrawal.clone()],
                    &tracked,
                    None,
                ));
                withdrawal
            } else {
                peak_cu = peak_cu.max(submit(&mut env, &owners[2], &[action], &tracked, None));
                let payout = env.portfolio_state(keeper).capital.get();
                assert!(
                    payout > DEPOSITS[2],
                    "withdrawal must depend on the liquidation reward"
                );
                if let Some(expected) = expected_payout {
                    assert_eq!(payout, expected);
                } else {
                    expected_payout = Some(payout);
                }
                assert_eq!(env.portfolio_matcher_sequence(keeper), sequence);
                let withdrawal = payout_ix(&env, &owners[2], keeper, tokens[2], payout);
                if let Some(retained) = retained {
                    assert_eq!(
                        withdrawal, retained,
                        "reward creation must preserve the prepared intent"
                    );
                }
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    &owners[2],
                    &[withdrawal.clone()],
                    &tracked,
                    None,
                ));
                withdrawal
            };
            let payout = expected_payout.unwrap();
            let penalty = capital - env.portfolio_state(short).capital.get();
            let reward = payout - DEPOSITS[2];
            assert!(penalty > 0 && penalty <= 10_000 && reward > 0);
            assert_eq!(reward, penalty / 2);
            let group = env.market_state().1;
            assert_eq!(group.insurance - stock.insurance, penalty - reward);
            assert_eq!(
                group.c_tot + group.insurance + payout,
                stock.c_tot + stock.insurance
            );
            assert_eq!(env.portfolio_position_epoch(short), position_epoch + 1);
            assert_eq!(
                health_cert(&env.portfolio_state(short)).certified_liq_deficit,
                0
            );
            assert!(group.assets[..2]
                .iter()
                .any(|asset| asset.oi_eff_short_q < POS_SCALE));
            assert!(group.assets[..2]
                .iter()
                .all(|asset| asset.oi_eff_long_q == asset.oi_eff_short_q));
            assert_eq!(env.portfolio_matcher_sequence(keeper), sequence + 1);
            assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
            assert_eq!(u128::from(env.token_amount(tokens[2])), payout);
            assert_eq!(env.token_amount(tokens[0]), 0);
            assert_eq!(env.token_amount(tokens[1]), 0);
            assert_eq!(
                u128::from(env.token_amount(env.vault)) + payout,
                DEPOSITS.iter().sum()
            );
            certificate_checks += audit(&env, portfolios)
                .into_iter()
                .filter(|current| *current)
                .count();
            history.push(checkpoint(&env, portfolios, tokens));

            peak_cu = peak_cu.max(submit(
                &mut env,
                &owners[2],
                &[withdrawal],
                &tracked,
                Some((2, PercolatorError::EngineStale)),
            ));
            rollbacks += 1;
            assert_eq!(env.portfolio_matcher_sequence(keeper), sequence + 1);
            assert_eq!(u128::from(env.token_amount(tokens[2])), payout);
            if let Some(expected) = &reference {
                assert_eq!(
                    &history, expected,
                    "observation order and aborted payouts preserve every checkpoint"
                );
            } else {
                reference = Some(history);
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, rollbacks), (4, 8));
    assert!(certificate_checks >= 8);
    println!("row426 reward payout rollback: {worlds} public worlds, 4 post-SPL aborts, {rollbacks} exact rollbacks, {certificate_checks} full-refresh comparisons, payout={} atoms; peak={peak_cu} CU", expected_payout.unwrap());
}
