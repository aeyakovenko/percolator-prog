//! INV-020/024/053/056/060/061/080: after complete two-provider refresh, omission
//! of the selected asset's discovery hint preserves its authenticated fee and
//! reward attribution. Stale evidence rolls back refresh and paid liquidation.
//! Fresh public histories only; Clock, signer SOL and vendor reports are fixtures.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

// The adjacent staged-action owner budgets two-asset liquidation at 500,000 CU.
const TWO_ASSET_ACTION_CU_LIMIT: u64 = 500_000;

fn observation(
    env: &V16CuEnv,
    target: Pubkey,
    keeper: &Keypair,
    reward: Option<Pubkey>,
    legs: &[EpochMatrixLeg; 2],
    assets: &[usize],
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(keeper.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    accounts.extend(
        assets
            .iter()
            .map(|i| AccountMeta::new_readonly(legs[*i].account, false)),
    );
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: assets
                .iter()
                .map(|i| CrankObservationHint {
                    asset_index: *i as u16,
                    oracle_accounts: 1,
                })
                .collect(),
        }
        .encode(),
    }
}

fn transact(
    env: &mut V16CuEnv,
    keeper: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &[&env.payer, keeper],
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let cu = if let Some((index, expected)) = rejection {
        if instructions.len() == 2 {
            let prefix = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), instructions[0].clone()],
                Some(&env.payer.pubkey()),
                &[&env.payer, keeper],
                env.svm.latest_blockhash(),
            );
            env.svm
                .simulate_transaction(prefix.into())
                .expect("the identical prefix independently makes public progress");
        }
        let failure = env
            .svm
            .send_transaction(tx)
            .expect_err("evidence rejection");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(expected as u32))
        );
        let payer_index = keys
            .iter()
            .position(|key| *key == env.payer.pubkey())
            .unwrap();
        before[payer_index].as_mut().unwrap().lamports -= fee;
        assert_eq!(
            keys.iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            before,
            "every tracked/compiled Account rolls back, including exact payer fee"
        );
        failure.meta.compute_units_consumed
    } else {
        env.svm
            .send_transaction(tx)
            .expect("current mixed-provider progress")
            .compute_units_consumed
    };
    assert_cu_within(
        "mixed-provider observation transaction",
        cu,
        TWO_ASSET_ACTION_CU_LIMIT * instructions.len() as u64,
    );
    cu
}

fn census(env: &V16CuEnv, portfolios: [Pubkey; 3]) {
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "mixed-provider liquidation",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed-provider liquidation", &group, &accounts).unwrap();
    assert_source_credit_rates("mixed-provider liquidation", &group).unwrap();
    for account in &accounts {
        assert_current_certificate_matches_independent(
            "mixed-provider liquidation",
            &group,
            account,
        )
        .unwrap();
    }
}

#[test]
fn v16_program_mixed_provider_liquidation_omissions_preserve_exact_entitlements() {
    const SHARE: u128 = 3_333;
    const LOSS: u128 = (CURRENT[0] - PRICE) as u128 + (CURRENT[1] - PRICE) as u128;
    const EQUITY: u128 = DEPOSITS[1] - LOSS;
    const MARGIN: u128 = (CURRENT[0] as u128 + CURRENT[1] as u128) / 10;
    assert_eq!((LOSS, EQUITY, MARGIN), (90_000, 130_000, 209_000));
    let mut peak = [0; 5];
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut reference = None;
    for other_provider in [
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ] {
        for reverse in [false, true] {
            for omit_selected_after_refresh in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        initial_price: PRICE,
                        min_nonzero_mm_req: 599,
                        min_nonzero_im_req: 600,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
                        liquidation_fee_bps: 100,
                        liquidation_fee_cap: 10_000,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let legs = [EpochMatrixProvider::Pyth, other_provider].map(|provider| {
                    new_epoch_matrix_leg(&mut env, provider, worlds, 0, PRICE, 100, 1)
                });
                for (i, leg) in legs.iter().enumerate() {
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        i as u16,
                        1,
                        0,
                        [leg.feed, [0; 32], [0; 32]],
                        &[leg.account],
                        1,
                        100,
                        0,
                        0,
                        100,
                        100,
                    )
                    .unwrap();
                }
                let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                let funded = [0, 1, 2].map(|i| funded_owner(&mut env, &owners[i], DEPOSITS[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let [peer, target, keeper] = portfolios;
                for asset in 0..2 {
                    env.trade_asset_with_cu(
                        asset,
                        &owners[0],
                        peer,
                        &owners[1],
                        target,
                        POS_SCALE as i128,
                        PRICE,
                        0,
                    );
                }
                let owners_before = portfolios.map(|key| env.svm.get_account(&key));
                let position_epoch = env.portfolio_position_epoch(target);
                let full = if reverse { [1, 0] } else { [0, 1] };
                set_test_clock(&mut env, 1, 101);
                for (i, leg) in legs.iter().enumerate() {
                    write_epoch_matrix_leg(&mut env, *leg, CURRENT[i], 101, 1);
                }
                // Authenticate both new targets without consuming their next-slot accrual.
                let stage = observation(&env, keeper, &owners[2], None, &legs, &full);
                peak[0] = peak[0].max(transact(&mut env, &owners[2], &[stage], &portfolios, None));
                for i in 0..2 {
                    assert_eq!(env.svm.get_account(&portfolios[i]), owners_before[i]);
                }
                assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
                for i in 0..2 {
                    let asset = env.market_state().1.assets[i];
                    assert_eq!(
                        (asset.effective_price, asset.raw_oracle_target_price),
                        (PRICE, CURRENT[i])
                    );
                }
                set_test_clock(&mut env, 2, 102);
                let old_pyth = EpochMatrixLeg {
                    account: Pubkey::new_unique(),
                    ..legs[0]
                };
                write_epoch_matrix_leg(&mut env, old_pyth, PRICE, 100, 1);
                let old_reports = [old_pyth, legs[1]];
                let mut tracked = vec![
                    env.market,
                    env.vault,
                    env.mint,
                    env.admin.pubkey(),
                    old_pyth.account,
                ];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                tracked.extend(legs.map(|leg| leg.account));
                let custody_keys: Vec<_> = tracked
                    .iter()
                    .copied()
                    .filter(|key| ![env.market, peer, target, keeper].contains(key))
                    .collect();
                let custody = frame(&env, &custody_keys);

                for refreshed in [false, true] {
                    let good = observation(
                        &env,
                        target,
                        &owners[2],
                        Some(keeper),
                        &legs,
                        if refreshed && omit_selected_after_refresh {
                            &[1]
                        } else {
                            &full
                        },
                    );
                    let stale =
                        observation(&env, target, &owners[2], Some(keeper), &old_reports, &full);
                    peak[1] = peak[1].max(transact(
                        &mut env,
                        &owners[2],
                        &[stale.clone()],
                        &tracked,
                        Some((2, PercolatorError::OracleStale)),
                    ));
                    // The late rejection must undo a real refresh or a real paid liquidation.
                    peak[2] = peak[2].max(transact(
                        &mut env,
                        &owners[2],
                        &[good.clone(), stale],
                        &tracked,
                        Some((3, PercolatorError::OracleStale)),
                    ));
                    rollbacks += 2;
                    peak[3] = peak[3].max(transact(&mut env, &owners[2], &[good], &tracked, None));
                    census(&env, portfolios);
                    let group = env.market_state().1;
                    let account = env.portfolio_state(target);
                    assert!(assert_current_certificate_matches_independent(
                        "current mixed target",
                        &group,
                        &account
                    )
                    .unwrap());
                    assert_eq!(
                        env.portfolio_position_epoch(target),
                        position_epoch + u64::from(refreshed)
                    );
                    if !refreshed {
                        assert_eq!(account.capital.get(), EQUITY);
                        let cert = health_cert(&account);
                        assert_eq!(
                            (
                                cert.certified_equity,
                                cert.certified_maintenance_req,
                                cert.certified_liq_deficit
                            ),
                            (EQUITY as i128, MARGIN, MARGIN - EQUITY)
                        );
                        assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
                        assert_eq!(group.insurance, 0);
                        for asset in &group.assets[..2] {
                            assert_eq!(
                                (asset.oi_eff_long_q, asset.oi_eff_short_q),
                                (POS_SCALE, POS_SCALE)
                            );
                        }
                    }
                }
                let group = env.market_state().1;
                let remaining = group.assets[0].oi_eff_short_q;
                assert!(remaining > 0 && remaining < POS_SCALE);
                assert_eq!(group.assets[0].oi_eff_long_q, remaining);
                assert_eq!(
                    (
                        group.assets[1].oi_eff_long_q,
                        group.assets[1].oi_eff_short_q
                    ),
                    (POS_SCALE, POS_SCALE)
                );
                let closed = POS_SCALE - remaining;
                let notional = (closed * u128::from(CURRENT[0])).div_ceil(POS_SCALE);
                let penalty = (notional * 100).div_ceil(10_000).min(10_000);
                let reward = penalty * SHARE / 10_000;
                let insurance = penalty - reward;
                assert!(penalty > 0 && reward > 0 && insurance > 0);
                assert_eq!(env.portfolio_state(target).capital.get(), EQUITY - penalty);
                assert_eq!(env.portfolio_state(target).pnl.get(), 0);
                assert_eq!(
                    env.portfolio_state(keeper).capital.get(),
                    DEPOSITS[2] + reward
                );
                assert_eq!(group.insurance, insurance);
                assert_eq!(
                    &group.insurance_domain_budget[..4],
                    &[insurance / 2, insurance - insurance / 2, 0, 0]
                );
                assert_eq!(group.vault, DEPOSITS.iter().sum::<u128>());
                assert_eq!(env.svm.get_account(&peer), owners_before[0]);
                assert_eq!(frame(&env, &custody_keys), custody);
                let cert = health_cert(&env.portfolio_state(target));
                assert_eq!(cert.certified_liq_deficit, 0);
                assert_eq!(
                    cert.certified_maintenance_req,
                    (remaining * u128::from(CURRENT[0])).div_ceil(POS_SCALE * 10)
                        + u128::from(CURRENT[1]) / 10
                );
                let repeated = observation(&env, target, &owners[2], Some(keeper), &legs, &full);
                peak[1] = peak[1].max(transact(
                    &mut env,
                    &owners[2],
                    &[repeated],
                    &tracked,
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
                rollbacks += 1;

                let refresh_peer = observation(&env, peer, &owners[2], Some(keeper), &legs, &full);
                peak[3] = peak[3].max(transact(
                    &mut env,
                    &owners[2],
                    &[refresh_peer],
                    &tracked,
                    None,
                ));
                let peer_account = env.portfolio_state(peer);
                assert_eq!(
                    peer_account.capital.get() as i128 + peer_account.pnl.get(),
                    (DEPOSITS[0] + LOSS) as i128
                );
                assert!(assert_current_certificate_matches_independent(
                    "current mixed peer",
                    &env.market_state().1,
                    &peer_account
                )
                .unwrap());
                let paid = DEPOSITS[2] + reward;
                let exposed_before_payout = [peer, target].map(|key| env.svm.get_account(&key));
                peak[4] = peak[4].max(
                    env.send(
                        env.withdraw_ix(keeper, paid),
                        vec![
                            AccountMeta::new(owners[2].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(keeper, false),
                            AccountMeta::new(tokens[2], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[2]],
                    )
                    .unwrap(),
                );
                assert_cu_within("mixed-provider keeper payout", peak[4], CUSTODY_CU_LIMIT);
                assert_eq!(
                    tokens.map(|key| env.token_amount(key) as u128),
                    [0, 0, paid]
                );
                assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
                assert_eq!(
                    [peer, target].map(|key| env.svm.get_account(&key)),
                    exposed_before_payout
                );
                assert_eq!(env.market_state().1.insurance, insurance);
                assert_eq!(
                    env.market_state().1.vault,
                    DEPOSITS.iter().sum::<u128>() - paid
                );
                census(&env, portfolios);
                let outcome = (
                    remaining,
                    penalty,
                    reward,
                    insurance,
                    cert.certified_maintenance_req,
                    portfolios.map(|key| {
                        let account = env.portfolio_state(key);
                        (account.capital.get(), account.pnl.get())
                    }),
                    tokens.map(|key| env.token_amount(key)),
                );
                if let Some(reference) = &reference {
                    assert_eq!(&outcome, reference);
                } else {
                    reference = Some(outcome);
                }
                println!("mixed liquidation {other_provider:?}, reverse={reverse}, omit_selected={omit_selected_after_refresh}: closed={closed}, penalty={penalty}, keeper={reward}, insurance={insurance}");
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks), (8, 40));
    println!("INV-020 mixed-provider liquidation: worlds={worlds}, exact_rollbacks={rollbacks}, CU [stage,reject,late_rollback,action,payout]={peak:?}");
}
