//! Row 426: reward recipients recertify their unrelated loss and pending price lag.
//! Public System/SPL/ATA/wrapper construction; no economic Account injection or replay.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_020_reward_recipient_liquidation.rs"]
mod reward_recipient_liquidation;

#[path = "inv_020_cpi_keeper_observations.rs"]
mod cpi_keeper_observations;

const ENDOWMENTS: [u128; 4] = [10_000_000, 220_000, 300_000, 10_000_000];
const KEEPER_PRICE: u64 = 1_050_000;
const SHARE: u128 = 3_333;

#[track_caller]
fn transact(
    env: &mut V16CuEnv,
    signers: &[&Keypair],
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let payer = keys
        .iter()
        .position(|key| *key == env.payer.pubkey())
        .unwrap();
    before[payer].as_mut().unwrap().lamports -=
        tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some((index, error)) = rejection {
        let failure = result.expect_err("invalid observation suffix must roll back");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        for (key, account) in keys.iter().zip(&before) {
            assert_eq!(
                &env.svm.get_account(key),
                account,
                "complete rollback {key}"
            );
        }
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("honest public continuation")
            .compute_units_consumed
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), before[payer]);
    assert_cu_within("active keeper observation composition", cu, 900_000);
    cu
}

fn observation(
    env: &V16CuEnv,
    target: Pubkey,
    owner: &Keypair,
    reward: Option<Pubkey>,
    report: Pubkey,
    assets: &[u16],
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
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
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations,
        }
        .encode(),
    }
}

fn trade(
    env: &V16CuEnv,
    owners: &[Keypair; 4],
    portfolios: [Pubkey; 4],
    assets: &[u16],
    size: i128,
    batch: bool,
) -> Instruction {
    let price = |asset| if asset == 2 { KEEPER_PRICE } else { PRICE };
    let instruction = if batch {
        env.batch_trade_no_cpi_ix(
            portfolios[2],
            portfolios[3],
            assets
                .iter()
                .map(|&asset_index| BatchTradeLeg {
                    asset_index,
                    market_id: env.asset_market_id(asset_index),
                    size_q: size,
                    exec_price: price(asset_index),
                    fee_bps: 0,
                })
                .collect(),
        )
    } else {
        assert_eq!(assets.len(), 1);
        env.trade_no_cpi_ix(
            portfolios[2],
            portfolios[3],
            assets[0],
            size,
            price(assets[0]),
            0,
        )
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owners[2].pubkey(), true),
            AccountMeta::new(owners[3].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[2], false),
            AccountMeta::new(portfolios[3], false),
        ],
        data: instruction.encode(),
    }
}

fn census(env: &V16CuEnv, portfolios: [Pubkey; 4], tokens: [Pubkey; 4]) {
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    let mut data = env.svm.get_account(&env.market).unwrap().data;
    assert_market_stock_census(
        "active keeper",
        &group,
        &data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("active keeper", &group, &accounts).unwrap();
    for account in &accounts {
        assert_current_certificate_matches_independent("active keeper census", &group, account)
            .unwrap();
    }
    let (_, market) = state::market_view_mut(&mut data).unwrap();
    market.validate_shape().unwrap();
    for key in portfolios {
        let mut data = env.svm.get_account(&key).unwrap().data;
        state::portfolio_view_mut_for_market_slots(&mut data, 4)
            .unwrap()
            .validate_with_market(&market.as_view())
            .unwrap();
    }
    let supply = ENDOWMENTS.iter().sum::<u128>() as u64;
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, supply);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(
        env.token_amount(env.vault) + tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
        supply
    );
    assert_eq!(group.mode, MarketModeV16::Live);
}

#[test]
fn v16_program_active_keeper_reward_recertifies_unrelated_loss_across_partial_refresh() {
    let mut peak = [0; 4];
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut reference = None;
    for reverse in [false, true] {
        for complete_first in [false, true] {
            for batch in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 4,
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
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let feed = [0xbc; 32];
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
                for asset in [1, 2, 3] {
                    env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
                }
                let owners = std::array::from_fn(|_| Keypair::new());
                let funded =
                    [0, 1, 2, 3].map(|i| funded_owner(&mut env, &owners[i], ENDOWMENTS[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let [peer, target, keeper, keeper_peer] = portfolios;
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
                for asset in [0, 1] {
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
                env.trade_asset_with_cu(
                    2,
                    &owners[3],
                    keeper_peer,
                    &owners[2],
                    keeper,
                    POS_SCALE as i128,
                    PRICE,
                    0,
                );
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
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                set_test_clock(&mut env, 0, 101);
                env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
                env.push_auth_mark_for_asset_as_admin(2, u64::MAX, KEEPER_PRICE);
                let report = env.set_pyth_price_with_conf(&feed, CURRENT[0] as i64, -6, 0, 101);
                tracked.push(report);
                let full = if reverse { [3, 2, 1, 0] } else { [0, 1, 2, 3] };
                let target_only = if reverse { [1, 0] } else { [0, 1] };
                let stage = observation(&env, target, &owners[2], Some(keeper), report, &full);
                peak[0] = peak[0].max(transact(&mut env, &[&owners[2]], &[stage], &tracked, None));
                let prefix = frame(&env, &portfolios);
                set_test_clock(&mut env, 64, 102);
                let stage = observation(&env, target, &owners[2], Some(keeper), report, &full);
                peak[0] = peak[0].max(transact(&mut env, &[&owners[2]], &[stage], &tracked, None));
                assert_eq!(frame(&env, &portfolios), prefix);
                assert_eq!(
                    [0, 1, 2].map(|i| env.market_state().1.assets[i].slot_last),
                    [32; 3]
                );
                let refresh =
                    observation(&env, target, &owners[2], Some(keeper), report, &target_only);
                peak[0] = peak[0].max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[refresh],
                    &tracked,
                    None,
                ));
                assert_current_short(&env, target, 130_000, 209_000);
                assert_eq!(env.svm.get_account(&keeper).unwrap(), prefix[2].1);
                assert_eq!(env.market_state().1.assets[2].slot_last, 32);
                census(&env, portfolios, tokens);

                let keeper_leg = active_leg_for_asset(&env.portfolio_state(keeper), 2);
                let liquidate =
                    observation(&env, target, &owners[2], Some(keeper), report, &target_only);
                let admit = trade(&env, &owners, portfolios, &[3], -(POS_SCALE as i128), batch);
                let stale = observation(&env, keeper, &owners[2], None, initial, &[0]);
                // Reaching instruction 4 proves that both the reward and new position were
                // committed within the transaction before the stale report rejects its suffix.
                peak[1] = peak[1].max(transact(
                    &mut env,
                    &[&owners[2], &owners[3]],
                    &[liquidate.clone(), admit.clone(), stale],
                    &tracked,
                    Some((4, PercolatorError::OracleStale)),
                ));
                rollbacks += 1;
                peak[0] = peak[0].max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[liquidate],
                    &tracked,
                    None,
                ));
                let remaining = env.market_state().1.assets[0].oi_eff_short_q;
                assert!(remaining > 0 && remaining < POS_SCALE);
                let closed = POS_SCALE - remaining;
                let penalty = ((closed * u128::from(CURRENT[0])).div_ceil(POS_SCALE))
                    .div_ceil(100)
                    .min(10_000);
                let reward = penalty * SHARE / 10_000;
                assert!(reward > 0);
                assert_eq!(env.portfolio_state(target).capital.get(), 130_000 - penalty);
                assert_eq!(
                    env.portfolio_state(keeper).capital.get(),
                    ENDOWMENTS[2] + reward
                );
                assert!(!health_cert(&env.portfolio_state(keeper)).valid);
                assert_eq!(
                    active_leg_for_asset(&env.portfolio_state(keeper), 2),
                    keeper_leg
                );
                assert_eq!(env.market_state().1.insurance, penalty - reward);
                assert_eq!(env.svm.get_account(&keeper_peer).unwrap(), prefix[3].1);
                let paid_target = env.svm.get_account(&target).unwrap();
                let omitted = observation(&env, keeper, &owners[2], None, report, &target_only);
                peak[1] = peak[1].max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[omitted],
                    &tracked,
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
                rollbacks += 1;

                if complete_first {
                    for account in [peer, keeper, keeper_peer] {
                        let refresh = observation(&env, account, &owners[2], None, report, &full);
                        peak[0] = peak[0].max(transact(
                            &mut env,
                            &[&owners[2]],
                            &[refresh],
                            &tracked,
                            None,
                        ));
                    }
                }
                // The trade can use the entire committed market state while asset 2 still
                // has catchup work. Its adverse lag must survive recipient recertification.
                peak[2] = peak[2].max(transact(
                    &mut env,
                    &[&owners[2], &owners[3]],
                    &[admit],
                    &tracked,
                    None,
                ));
                let expected = ENDOWMENTS[2] + reward - u128::from(KEEPER_PRICE - PRICE);
                let observed_price = if complete_first {
                    KEEPER_PRICE
                } else {
                    1_032_000
                };
                let observed_loss = u128::from(observed_price - PRICE);
                let lag = u128::from(KEEPER_PRICE - observed_price);
                for (key, value) in [
                    (keeper, ENDOWMENTS[2] + reward - observed_loss),
                    (keeper_peer, ENDOWMENTS[3] + observed_loss),
                ] {
                    let account = env.portfolio_state(key);
                    assert!(assert_current_certificate_matches_independent(
                        "reward recipient admission",
                        &env.market_state().1,
                        &account
                    )
                    .unwrap());
                    assert_eq!(
                        account.capital.get() as i128 + account.pnl.get(),
                        value as i128
                    );
                    assert_eq!(
                        health_cert(&account).certified_initial_req,
                        u128::from(PRICE + observed_price) / 10
                            + if key == keeper { lag } else { 0 },
                        "side-specific adverse lag for {key}"
                    );
                    assert_eq!(
                        health_cert(&account).certified_maintenance_req,
                        health_cert(&account).certified_initial_req
                    );
                    assert_eq!(health_cert(&account).certified_liq_deficit, 0);
                    for asset in [2, 3] {
                        assert_eq!(
                            active_leg_for_asset(&account, asset).basis_pos_q,
                            if key == keeper { -1 } else { 1 } * POS_SCALE as i128
                        );
                    }
                }
                assert_eq!(
                    env.market_state().1.assets[2].slot_last,
                    if complete_first { 64 } else { 32 }
                );
                census(&env, portfolios, tokens);
                if !complete_first {
                    let refresh = observation(&env, peer, &owners[2], None, report, &full);
                    peak[0] = peak[0].max(transact(
                        &mut env,
                        &[&owners[2]],
                        &[refresh],
                        &tracked,
                        None,
                    ));
                }
                for asset in [3, 2] {
                    let close = trade(
                        &env,
                        &owners,
                        portfolios,
                        &[asset],
                        POS_SCALE as i128,
                        !batch,
                    );
                    peak[2] = peak[2].max(transact(
                        &mut env,
                        &[&owners[2], &owners[3]],
                        &[close],
                        &tracked,
                        None,
                    ));
                }
                assert_eq!(env.portfolio_state(keeper).capital.get(), expected);
                let withdrawal = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[2].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(keeper, expected).encode(),
                };
                peak[3] = peak[3].max(transact(
                    &mut env,
                    &[&owners[2]],
                    &[withdrawal],
                    &tracked,
                    None,
                ));
                assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
                assert_eq!(env.token_amount(tokens[2]) as u128, expected);
                assert_eq!(env.svm.get_account(&target).unwrap(), paid_target);
                assert_eq!(env.market_state().1.insurance, penalty - reward);
                let insurance = penalty - reward;
                assert_eq!(
                    env.market_state().1.insurance_domain_budget,
                    [insurance / 2, insurance - insurance / 2, 0, 0, 0, 0, 0, 0]
                );
                census(&env, portfolios, tokens);
                let outcome = (
                    remaining,
                    penalty,
                    reward,
                    expected,
                    portfolios.map(|key| {
                        let account = env.portfolio_state(key);
                        (account.capital.get(), account.pnl.get())
                    }),
                    [0, 1, 2, 3].map(|i| {
                        (
                            env.market_state().1.assets[i].oi_eff_long_q,
                            env.market_state().1.assets[i].oi_eff_short_q,
                        )
                    }),
                );
                if let Some(reference) = &reference {
                    assert_eq!(&outcome, reference);
                } else {
                    reference = Some(outcome);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks), (8, 16));
    println!("active keeper: {worlds} worlds, {rollbacks} exact rollbacks; CU [observation/liquidation,rejection,trade,payout]={peak:?}; economics={reference:?}");
}
