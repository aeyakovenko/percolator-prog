//! INV-020/045/053/054/060/061/071/080/081: fresh evidence after paid discovery.
//! Public System/SPL/wrapper construction; only Clock and external Pyth reports
//! are harness inputs. No protocol-account mutation or snapshot restoration.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_045_retained_penalty_handoff.rs"]
mod retained_penalty_handoff;

#[path = "inv_045_reward_policy_catchup.rs"]
mod reward_policy_catchup;

#[path = "inv_045_exposed_keeper_provenance.rs"]
mod exposed_keeper_provenance;

fn values(env: &V16CuEnv, portfolios: [Pubkey; 5]) -> [i128; 5] {
    portfolios.map(|key| {
        let account = env.portfolio_state(key);
        account.capital.get() as i128 + account.pnl.get()
    })
}

fn census(env: &V16CuEnv, portfolios: [Pubkey; 5]) -> [bool; 5] {
    let group = env.market_state().1;
    assert_eq!(group.backing_provider_earnings_total, 0);
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "authenticated reward handoff",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        env.token_amount(env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("authenticated reward handoff", &group, &accounts)
        .unwrap();
    assert_source_credit_rates("authenticated reward handoff", &group).unwrap();
    accounts.each_ref().map(|account| {
        assert_current_certificate_matches_independent(
            "authenticated reward handoff",
            &group,
            account,
        )
        .unwrap()
    })
}

fn observe(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    report: Option<Pubkey>,
    reward: Option<Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(signer, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    accounts.extend(report.map(|key| AccountMeta::new_readonly(key, false)));
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: crank_observations_with_accounts(0, 1),
        }
        .encode(),
    }
}

fn submit(
    env: &mut V16CuEnv,
    signer: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
) -> u64 {
    submit_with_cu_limit(
        env,
        signer,
        instructions,
        tracked,
        rejection,
        CRANK_CU_LIMIT,
    )
}

fn submit_with_cu_limit(
    env: &mut V16CuEnv,
    signer: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
    instruction_cu_limit: u64,
) -> u64 {
    env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &[&env.payer, signer],
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let network_fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let cu = match rejection {
        Some((index, expected)) => {
            if instructions.len() == 2 {
                let valid = Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), instructions[0].clone()],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, signer],
                    env.svm.latest_blockhash(),
                );
                env.svm
                    .simulate_transaction(valid.into())
                    .expect("the same fresh prefix must succeed independently");
            }
            let error = env
                .svm
                .send_transaction(tx)
                .expect_err("observation rejection");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(index, expected)
            );
            let payer_index = keys
                .iter()
                .position(|key| *key == env.payer.pubkey())
                .unwrap();
            before[payer_index].as_mut().unwrap().lamports -= network_fee;
            assert_eq!(
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before,
                "all tracked and compiled Accounts roll back except the exact network fee"
            );
            error.meta.compute_units_consumed
        }
        None => {
            env.svm
                .send_transaction(tx)
                .expect("authenticated progress")
                .compute_units_consumed
        }
    };
    assert_cu_within(
        "authenticated handoff transaction",
        cu,
        instruction_cu_limit * instructions.len() as u64,
    );
    cu
}

fn run_authenticated_handoff(
    max_funding: u64,
    fresh_prices: &[u64],
    shares: &[u16],
    raw_prints: &[u64],
) {
    const ACCEPTED: u64 = ENTRY - 2_400;
    // MARK is below the accepted frontier: the premium hits the negative cap.
    // Funding rounds down before the per-lot index is scaled by ADL_ONE.
    let funding_per_lot =
        -(-i128::from(max_funding) * i128::from(ACCEPTED)).div_euclid(1_000_000_000);
    assert_eq!(funding_per_lot, i128::from(max_funding != 0));
    let mut peak_cu = 0;
    let mut late_rejections = 0;
    let mut rewarded_rollbacks = 0;
    let mut worlds = 0;
    for &fresh_price in fresh_prices {
        for &share in shares {
            let mut reference = None;
            for &raw_print in raw_prints {
                for publish_first in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            max_abs_funding_e9_per_slot: max_funding,
                            ..production_risk_params()
                        },
                    );
                    set_test_clock(&mut env, 1, 100);
                    env.update_liquidation_fee_policy_with_cu(share);
                    let feed = [0x59; 32];
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
                    .unwrap();
                    let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
                    let funded =
                        std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
                    let portfolios = funded.map(|pair| pair.0);
                    let tokens = funded.map(|pair| pair.1);
                    let [target, peer, trader_a, trader_b, keeper] = portfolios;
                    env.trade_asset_with_cu(
                        0,
                        &owners[0],
                        target,
                        &owners[1],
                        peer,
                        (100 * POS_SCALE) as i128,
                        ENTRY,
                        0,
                    );
                    census(&env, portfolios);
                    set_test_clock(&mut env, 5, 1_000);
                    let clock_only = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
                    submit(&mut env, &owners[4], &[clock_only], &portfolios, None);
                    env.trade_asset_with_cu(
                        0,
                        &owners[2],
                        trader_a,
                        &owners[3],
                        trader_b,
                        POS_SCALE as i128,
                        raw_print,
                        0,
                    );
                    let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
                    let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
                    let paid = fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
                    let mut expected = FUNDS.map(i128::from);
                    expected[2] -= paid as i128;
                    expected[3] -= paid as i128;
                    assert_eq!(values(&env, portfolios), expected);
                    assert_eq!(env.market_state().1.insurance, 2 * paid);
                    assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
                    assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
                    let staged = env.market_state().1.assets[0];
                    assert_eq!((staged.f_long_num, staged.f_short_num), (0, 0));
                    assert_eq!(staged.slot_last, 5);
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    assert_eq!(profile.funding_mark_e6, MARK);
                    census(&env, portfolios);

                    set_test_clock(&mut env, 6, 1_001);
                    let fresh =
                        env.set_pyth_price_with_conf(&feed, fresh_price as i64, -6, 0, 1_001);
                    let equivocal =
                        env.set_pyth_price_with_conf(&feed, fresh_price as i64 + 1, -6, 0, 1_001);
                    let stale = env.set_pyth_price_with_conf(&feed, fresh_price as i64, -6, 0, 940);
                    let mut tracked = vec![
                        env.market,
                        env.mint,
                        env.vault,
                        env.admin.pubkey(),
                        initial,
                        fresh,
                        equivocal,
                        stale,
                    ];
                    tracked.extend(portfolios);
                    tracked.extend(tokens);
                    tracked.extend(owners.each_ref().map(Signer::pubkey));
                    let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
                    let framed = [
                        initial,
                        fresh,
                        equivocal,
                        stale,
                        env.admin.pubkey(),
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        owners[2].pubkey(),
                        owners[3].pubkey(),
                        owners[4].pubkey(),
                    ];
                    let provider_and_signer_accounts = framed.map(|key| env.svm.get_account(&key));
                    let valid =
                        observe(&env, target, owners[4].pubkey(), Some(fresh), Some(keeper));
                    let conflicting = observe(
                        &env,
                        target,
                        owners[4].pubkey(),
                        Some(equivocal),
                        Some(keeper),
                    );
                    if publish_first {
                        let publish = observe(&env, keeper, owners[4].pubkey(), Some(fresh), None);
                        peak_cu =
                            peak_cu.max(submit(&mut env, &owners[4], &[publish], &tracked, None));
                        assert_eq!(values(&env, portfolios), expected);
                        census(&env, portfolios);
                    }
                    let mut liquidation = None;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        let before_values = values(&env, portfolios);
                        let before_cert = health_cert(&env.portfolio_state(target));
                        peak_cu = peak_cu.max(submit(
                            &mut env,
                            &owners[4],
                            &[valid.clone(), conflicting.clone()],
                            &tracked,
                            Some((
                                3,
                                InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                            )),
                        ));
                        late_rejections += 1;
                        let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                        peak_cu = peak_cu.max(submit(
                            &mut env,
                            &owners[4],
                            &[valid.clone()],
                            &tracked,
                            None,
                        ));
                        let current = census(&env, portfolios);
                        let (profile, after) = env.market_state();
                        assert_eq!(after.assets[0].effective_price, ACCEPTED);
                        assert_eq!(after.assets[0].raw_oracle_target_price, fresh_price);
                        assert_eq!(profile.oracle_target_publish_time, 1_001);
                        assert_eq!(profile.last_good_oracle_slot, 6);
                        let funding_index = funding_per_lot * ADL_ONE as i128;
                        assert_eq!(
                            (after.assets[0].f_long_num, after.assets[0].f_short_num),
                            (funding_index, -funding_index)
                        );
                        assert_eq!(after.assets[0].slot_last, 6);
                        assert_eq!(
                            framed.map(|key| env.svm.get_account(&key)),
                            provider_and_signer_accounts
                        );
                        assert_eq!(
                            [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                            peers
                        );
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                        if closed == 0 {
                            assert_eq!(values(&env, portfolios)[4], before_values[4]);
                            assert_eq!(after.insurance, before.insurance);
                            assert!(current[0]);
                            assert!(
                                health_cert(&env.portfolio_state(target)).certified_liq_deficit > 0
                            );
                            for (report, reward_tail, error) in [
                                (None, None, InstructionError::NotEnoughAccountKeys),
                                (
                                    Some(stale),
                                    Some(keeper),
                                    InstructionError::Custom(PercolatorError::OracleStale as u32),
                                ),
                                (
                                    Some(equivocal),
                                    Some(keeper),
                                    InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                                ),
                            ] {
                                let invalid =
                                    observe(&env, target, owners[4].pubkey(), report, reward_tail);
                                peak_cu = peak_cu.max(submit(
                                    &mut env,
                                    &owners[4],
                                    &[invalid],
                                    &tracked,
                                    Some((2, error)),
                                ));
                            }
                            continue;
                        }
                        let penalty = fee(closed, ACCEPTED, 5);
                        let reward = penalty * u128::from(share) / 10_000;
                        assert!(before_cert.certified_liq_deficit > 0 && current[0]);
                        assert_eq!(
                            health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                            0
                        );
                        assert!(closed < 100 * POS_SCALE && reward > 0);
                        for wrong_price in [ENTRY, ACCEPTED_PRINT, raw_print, MARK, fresh_price] {
                            assert_ne!(penalty, fee(closed, wrong_price, 5));
                        }
                        let mut after_values = before_values;
                        after_values[0] -= penalty as i128;
                        after_values[4] += reward as i128;
                        assert_eq!(values(&env, portfolios), after_values);
                        assert_eq!(after.insurance - before.insurance, penalty - reward);
                        rewarded_rollbacks += 1;
                        liquidation = Some((closed, penalty, reward));
                        break;
                    }
                    let (closed, penalty, reward) =
                        liquidation.expect("bounded rewarded liquidation");

                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        &owners[4],
                        &[valid],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                    ));
                    // Complete ADL and source accounting before checking every owner's entitlement.
                    for _ in 0..8 {
                        for index in [1, 2, 3, 0] {
                            if census(&env, portfolios)[index] {
                                continue;
                            }
                            let portfolio = portfolios[index];
                            let ix =
                                observe(&env, portfolio, owners[4].pubkey(), Some(fresh), None);
                            peak_cu =
                                peak_cu.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
                            census(&env, portfolios);
                        }
                        if census(&env, portfolios)[..4].iter().all(|current| *current) {
                            break;
                        }
                    }
                    assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
                    let loss = i128::from(ENTRY - ACCEPTED) - funding_per_lot;
                    expected[0] -= 100 * loss + penalty as i128;
                    expected[1] += 100 * loss;
                    expected[2] -= loss;
                    expected[3] += loss;
                    expected[4] += reward as i128;
                    assert_eq!(values(&env, portfolios), expected);
                    let expected_pnl = [0, 100 * loss, 0, loss, 0];
                    for i in 0..5 {
                        let account = env.portfolio_state(portfolios[i]);
                        assert_eq!(account.pnl.get(), expected_pnl[i]);
                        assert_eq!(account.capital.get() as i128, expected[i] - expected_pnl[i]);
                    }
                    assert_eq!(env.market_state().1.insurance, 2 * paid + penalty - reward);
                    if max_funding != 0 {
                        println!("funded handoff: publish_first={publish_first}, paid_per_trader={paid}, closed={closed}, penalty={penalty}, reward={reward}, owner_values={expected:?}");
                    }
                    let payout = FUNDS[4] as u128 + reward;
                    let withdraw_cu = env
                        .send(
                            env.withdraw_ix(keeper, payout),
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
                        .expect("keeper realizes only original principal and authenticated reward");
                    assert_cu_within("authenticated keeper payout", withdraw_cu, CUSTODY_CU_LIMIT);
                    expected[4] = 0;
                    assert_eq!(values(&env, portfolios), expected);
                    assert_eq!(env.token_amount(tokens[4]) as u128, payout);
                    assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
                    assert_eq!(env.svm.get_account(&env.mint), custody[0]);
                    assert_eq!(
                        framed.map(|key| env.svm.get_account(&key)),
                        provider_and_signer_accounts
                    );
                    census(&env, portfolios);
                    let group = env.market_state().1;
                    assert_eq!(
                        expected.iter().sum::<i128>() + group.insurance as i128,
                        group.vault as i128
                    );
                    assert_eq!(
                        group.vault + payout,
                        FUNDS.iter().map(|value| *value as u128).sum::<u128>()
                    );
                    let outcome = (
                        closed,
                        penalty,
                        reward,
                        expected,
                        group.insurance,
                        group.vault,
                    );
                    if let Some(reference) = &reference {
                        assert_eq!(
                            &outcome, reference,
                            "raw prints and report handoff partitions converge"
                        );
                    } else {
                        reference = Some(outcome);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        worlds,
        fresh_prices.len() * shares.len() * raw_prints.len() * 2
    );
    assert_eq!(rewarded_rollbacks, worlds);
    println!("authenticated handoff: max_funding={max_funding}, funding_per_lot={funding_per_lot}, worlds={worlds}, late_rollbacks={late_rejections}, reward_rollbacks={rewarded_rollbacks}, peak_transaction_cu={peak_cu}");
}

#[test]
fn v16_program_paid_discovery_fresh_handoff_authenticates_liquidation_and_keeper_exit() {
    run_authenticated_handoff(
        0,
        &[MARK, MARK - 1_000],
        &[3_333, 10_000],
        &[980_000, 900_000],
    );
}

#[test]
fn v16_program_nonzero_funding_fresh_handoff_preserves_owner_and_keeper_entitlement() {
    run_authenticated_handoff(1_000, &[MARK], &[3_333], &[980_000]);
}
