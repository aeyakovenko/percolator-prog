//! INV-027/024/053/060/073/080/081: reward credit does not pay the recipient's
//! own elapsed fees. Differently aged, never-exposed owners first settle their
//! liabilities, then admit risk at independently computed equity and fully exit.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_never_exposed_reward_recipient_settles_own_fees_before_first_risk() {
    const BIRTH: [u64; 2] = [1, 3];
    const FUNDS: [u128; 2] = [2_000, 1_000];
    const REWARD_SLOT: u64 = 5;
    const OPEN_SLOT: u64 = 7;
    const SHARE: u16 = 3_333;
    let charged = BIRTH.map(|slot| FEE_RATE * u128::from(OPEN_SLOT - slot));
    let reward = FEE_RATE * u128::from(REWARD_SLOT - BIRTH[0]) * u128::from(SHARE) / 10_000;
    let capital = [FUNDS[0] - charged[0], FUNDS[1] + reward - charged[1]];
    let insurance = charged.iter().sum::<u128>() - reward;
    let size = (capital[1] * POS_SCALE / u128::from(PRICE)) as i128;
    let too_large = size + (POS_SCALE / u128::from(PRICE)) as i128;
    let requirement = |q: i128| (q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert_eq!((reward, capital, insurance), (9, [1_958, 981], 61));
    assert_eq!((requirement(size), requirement(too_large)), (981, 982));
    let mut peak_cu = 0;
    let mut worlds = 0;
    for split_recipient_fee in [false, true] {
        for recipient_taker in [false, true] {
            for batch in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: FEE_RATE,
                        maintenance_margin_bps: 5_000,
                        initial_margin_bps: 10_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(BIRTH[0]);
                env.configure_auth_mark_for_asset_as_admin(0, BIRTH[0], PRICE);
                env.update_maintenance_fee_policy_with_cu(SHARE);
                let owners = [Keypair::new(), Keypair::new()];
                let mut portfolios = [Pubkey::default(); 2];
                let mut tokens = [Pubkey::default(); 2];
                let observer_owner = Keypair::new();
                let observer = public_portfolio(&mut env, &observer_owner);
                for i in 0..2 {
                    env.svm.warp_to_slot(BIRTH[i]);
                    portfolios[i] = public_portfolio(&mut env, &owners[i]);
                    tokens[i] = public_deposit(&mut env, &owners[i], portfolios[i], FUNDS[i]);
                }
                let mint = env.svm.get_account(&env.mint);
                env.svm.warp_to_slot(REWARD_SLOT);
                env.sync_maintenance_fee_with_cu(portfolios[0], Some(portfolios[1]), REWARD_SLOT);
                assert_eq!(
                    env.portfolio_state(portfolios[1]).capital.get(),
                    FUNDS[1] + reward
                );
                assert_eq!(
                    env.portfolio_state(portfolios[1]).last_fee_slot.get(),
                    BIRTH[1]
                );
                if split_recipient_fee {
                    env.sync_maintenance_fee_with_cu(portfolios[1], None, REWARD_SLOT);
                }
                let owners_before = portfolios.map(|key| env.svm.get_account(&key));
                for slot in REWARD_SLOT + 1..=OPEN_SLOT {
                    env.svm.warp_to_slot(slot);
                    env.crank(
                        observer,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations(0),
                        },
                    );
                }
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    owners_before
                );
                for portfolio in portfolios {
                    let account = env.portfolio_state(portfolio);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(account.pnl.get(), 0);
                }
                let [taker, lp] = if recipient_taker { [1, 0] } else { [0, 1] };
                let make_trade = |env: &V16CuEnv, quantity| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[taker].pubkey(), true),
                        AccountMeta::new(owners[lp].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[taker], false),
                        AccountMeta::new(portfolios[lp], false),
                    ],
                    data: if batch {
                        env.batch_trade_no_cpi_ix(
                            portfolios[taker],
                            portfolios[lp],
                            vec![BatchTradeLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q: quantity,
                                exec_price: PRICE,
                                fee_bps: 0,
                            }],
                        )
                    } else {
                        env.trade_no_cpi_ix(
                            portfolios[taker],
                            portfolios[lp],
                            0,
                            quantity,
                            PRICE,
                            0,
                        )
                    }
                    .encode(),
                };
                let mut prefix = vec![];
                for i in [taker, lp] {
                    prefix.push(Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                        ],
                        data: ProgInstruction::SyncMaintenanceFee {
                            now_slot: OPEN_SLOT,
                        }
                        .encode(),
                    });
                    prefix.push(Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(observer_owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                        ],
                        data: ProgInstruction::PermissionlessCrank {
                            now_slot: OPEN_SLOT,
                            observations: crank_observations(0),
                        }
                        .encode(),
                    });
                }
                let mut tracked = vec![
                    env.market,
                    env.vault,
                    env.mint,
                    observer,
                    env.admin.pubkey(),
                    observer_owner.pubkey(),
                ];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                for (quantity, reject) in [(too_large, true), (size, false)] {
                    env.svm.expire_blockhash();
                    let mut instructions = vec![heap_ix(), cu_ix()];
                    instructions.extend(prefix.clone());
                    instructions.push(make_trade(&env, quantity));
                    let tx = Transaction::new_signed_with_payer(
                        &instructions,
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &owners[0], &owners[1], &observer_owner],
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    let mut keys = tracked.clone();
                    keys.extend(tx.message.account_keys.iter().copied());
                    keys.sort_unstable();
                    keys.dedup();
                    let mut before: Vec<_> =
                        keys.iter().map(|key| env.svm.get_account(key)).collect();
                    let network_fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    if reject {
                        let error = env
                            .svm
                            .send_transaction(tx)
                            .expect_err("first risk must fit the reward recipient's net equity");
                        assert_eq!(
                            error.err,
                            TransactionError::InstructionError(
                                6,
                                InstructionError::Custom(
                                    PercolatorError::EngineInvalidConfig as u32
                                )
                            )
                        );
                        let payer = keys
                            .iter()
                            .position(|key| *key == env.payer.pubkey())
                            .unwrap();
                        before[payer].as_mut().unwrap().lamports -= network_fee;
                        assert_eq!(
                            keys.iter()
                                .map(|key| env.svm.get_account(key))
                                .collect::<Vec<_>>(),
                            before
                        );
                        peak_cu = peak_cu.max(error.meta.compute_units_consumed);
                    } else {
                        peak_cu = peak_cu.max(
                            env.svm
                                .send_transaction(tx)
                                .expect("exact first admission after own liabilities")
                                .compute_units_consumed,
                        );
                    }
                }
                let check = |env: &V16CuEnv, paid: [u128; 2], open: bool| {
                    let group = env.market_state().1;
                    let accounts = [portfolios[0], portfolios[1], observer]
                        .map(|key| env.portfolio_state(key));
                    for i in 0..2 {
                        assert_eq!(accounts[i].capital.get(), capital[i] - paid[i]);
                        assert_eq!(accounts[i].last_fee_slot.get(), OPEN_SLOT);
                        assert_eq!(accounts[i].pnl.get(), 0);
                        assert_eq!(env.token_amount(tokens[i]) as u128, paid[i]);
                        let current = assert_current_certificate_matches_independent(
                            "reward recipient first risk",
                            &group,
                            &accounts[i],
                        )
                        .unwrap();
                        if open {
                            assert!(
                                current,
                                "first admission requires both current certificates"
                            );
                            let cert = health_cert(&accounts[i]);
                            assert_eq!(cert.certified_equity, capital[i] as i128);
                            assert_eq!(cert.certified_initial_req, requirement(size));
                        } else {
                            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                                &accounts[i]
                            )));
                        }
                    }
                    assert_eq!(group.insurance, insurance);
                    assert_eq!(
                        group.c_tot,
                        capital.iter().sum::<u128>() - paid.iter().sum::<u128>()
                    );
                    assert_eq!(
                        group.vault,
                        FUNDS.iter().sum::<u128>() - paid.iter().sum::<u128>()
                    );
                    assert_eq!(env.svm.get_account(&env.mint), mint);
                    assert_market_stock_census(
                        "reward recipient first risk",
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        env.token_amount(env.vault) as u128,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(
                        "reward recipient first risk",
                        &group,
                        &accounts,
                    )
                    .unwrap();
                };
                check(&env, [0; 2], true);
                let close = make_trade(&env, -size);
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![heap_ix(), cu_ix(), close],
                    &[&owners[0], &owners[1]],
                )
                .unwrap();
                check(&env, [0; 2], false);
                let mut paid = [0; 2];
                for i in [lp, taker] {
                    let cu = env
                        .send(
                            env.withdraw_ix(portfolios[i], capital[i]),
                            vec![
                                AccountMeta::new(owners[i].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                                AccountMeta::new(tokens[i], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[i]],
                        )
                        .unwrap();
                    assert_cu_within("first-risk owner payout", cu, CUSTODY_CU_LIMIT);
                    paid[i] = capital[i];
                    check(&env, paid, false);
                }
                for i in [taker, lp] {
                    env.close_portfolio_with_cu(&owners[i], portfolios[i]);
                }
                assert_eq!(env.market_state().1.vault, insurance);
                assert_eq!(env.market_state().1.c_tot, 0);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_cu_within(
        "first-risk fee/refresh/trade bundle",
        peak_cu,
        2 * CUSTODY_CU_LIMIT,
    );
    println!("reward recipient first risk: worlds={worlds}, exact_rollbacks={worlds}, peak_bundle_cu={peak_cu}");
}
