//! INV-008/010/024/031/064/080/081: returning withdrawn insurance to the vault
//! cannot erase a committed funding intent or consume an aborted one.
//!
//! Unlike the refund and failed-top-up controls, both opposing custody CPIs
//! complete inside one transaction here. Their net token delta is zero, but an
//! odd-atom refill changes the long/short stock distribution and consumes the
//! shared funding lane. A later duplicate or SPL error must undo both changes,
//! including lazy telemetry; alternate-route recovery must remain executable.
//! Authority epochs and generation stay fixed throughout this bounded history.
//!
//! Row428 remains OPEN: the intrinsic sequence asserted here is the shared
//! insurance TOP-UP sequence. Withdrawal has no stock-sequence field. Exhausted
//! stock rejects across signed-live/unsigned-resolved admission, which is not
//! proof that a paid withdrawal stays stale against independently added stock.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_insurance_round_trip_consumption_survives_cross_route_retry() {
    const AMOUNT: u64 = 37;
    const PEER: u64 = 53;
    const SUPPLY: u64 = AMOUNT + PEER;
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut rolled_back_transfers = 0;
    for direct_first in [false, true] {
        for telemetry in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let source =
                create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
            let sink =
                create_ata_for_test(&mut env.svm, &env.payer, Keypair::new().pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &source,
                    &env.admin.pubkey(),
                    &[],
                    SUPPLY,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
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
            for (domain, amount) in [(0, AMOUNT), (2, PEER)] {
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain,
                        market_id: env.asset_market_id(domain / 2),
                        authority_epoch: 0,
                        intent_id: 1,
                        amount: amount.into(),
                    },
                    vec![
                        AccountMeta::new(env.admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&env.admin.insecure_clone()],
                )
                .unwrap();
            }
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger,
                state::insurance_ledger_account_len(),
                env.program_id,
            );
            let empty_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
            assert!(empty_ledger.data.iter().all(|byte| *byte == 0));
            let controls = [env.control_sequences(0), env.control_sequences(1)];
            let profiles = [0, 1].map(|asset| {
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    asset,
                )
                .unwrap()
            });
            let market_ids = [env.asset_market_id(0), env.asset_market_id(1)];
            let program_id = env.program_id;
            let append_ledger = |mut ix: Instruction| {
                if telemetry {
                    ix.accounts.push(AccountMeta::new(ledger.pubkey(), false));
                }
                ix
            };
            let withdrawal = append_ledger(Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 0,
                    market_id: market_ids[0],
                    authority_epoch: controls[0].authority_epoch,
                    amount: AMOUNT.into(),
                }
                .encode(),
            });
            let top_up_accounts = vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            let refill = |direct, intent_id| {
                append_ledger(Instruction {
                    program_id,
                    accounts: top_up_accounts.clone(),
                    data: if direct {
                        ProgInstruction::TopUpInsurance {
                            market_id: market_ids[0],
                            authority_epoch: controls[0].authority_epoch,
                            intent_id,
                            amount: AMOUNT.into(),
                        }
                    } else {
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: 1,
                            market_id: market_ids[0],
                            authority_epoch: controls[0].authority_epoch,
                            intent_id,
                            amount: AMOUNT.into(),
                        }
                    }
                    .encode(),
                })
            };
            let retained = [refill(direct_first, 2), refill(!direct_first, 2)];
            let round_trip = |top_up: &Instruction| vec![withdrawal.clone(), top_up.clone()];
            let signed = |env: &V16CuEnv, instructions: &[Instruction], nonce, unsigned| {
                let mut message = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - nonce),
                ];
                message.extend_from_slice(instructions);
                let signers = if unsigned {
                    vec![&env.payer]
                } else {
                    vec![&env.payer, &env.admin]
                };
                Transaction::new_signed_with_payer(
                    &message,
                    Some(&env.payer.pubkey()),
                    &signers,
                    env.svm.latest_blockhash(),
                )
            };
            let keys = [
                env.market,
                env.vault,
                source,
                sink,
                env.mint,
                ledger.pubkey(),
                env.admin.pubkey(),
                env.vault_authority,
            ];
            let tokens = [env.vault, source, sink];
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            assert_eq!(Mint::unpack(&mint_frame.data).unwrap().supply, SUPPLY);
            assert_eq!(
                Mint::unpack(&mint_frame.data).unwrap().mint_authority,
                COption::None
            );
            let check = |env: &V16CuEnv, budgets: [u128; 4], rounds: u64, target_paid: bool| {
                let group = env.market_state().1;
                let remaining = budgets.iter().sum::<u128>();
                assert_eq!(group.insurance_domain_budget, budgets);
                assert_eq!(group.insurance_domain_spent, [0; 4]);
                assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
                assert_eq!(
                    (group.insurance, group.vault, group.c_tot),
                    (remaining, remaining, 0)
                );
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                for asset in 0..2 {
                    let mut expected = controls[asset];
                    expected.insurance_top_up += if asset == 0 { rounds } else { 0 };
                    assert_eq!(env.control_sequences(asset), expected);
                    assert_eq!(group.assets[asset].market_id, market_ids[asset]);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            asset
                        )
                        .unwrap(),
                        profiles[asset]
                    );
                }
                for (index, key) in tokens.into_iter().enumerate() {
                    let mut expected = token_frames[index].clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = match index {
                        0 => remaining as u64,
                        1 => SUPPLY - remaining as u64,
                        _ => 0,
                    };
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key).unwrap(), expected);
                }
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                let record = env.svm.get_account(&ledger.pubkey()).unwrap();
                if telemetry && rounds != 0 {
                    assert_eq!(
                        state::read_insurance_ledger(&record.data).unwrap(),
                        state::InsuranceLedgerAccountV16 {
                            market_group: env.market.to_bytes(),
                            authority: env.admin.pubkey().to_bytes(),
                            total_principal_atoms: if target_paid { 0 } else { AMOUNT.into() },
                            total_deposited_atoms: u128::from(rounds * AMOUNT),
                            total_withdrawn_atoms: u128::from(
                                (rounds + u64::from(target_paid)) * AMOUNT
                            ),
                            cumulative_profit_atoms: 0,
                            cumulative_loss_atoms: 0,
                            last_observed_insurance_atoms: if target_paid {
                                0
                            } else {
                                AMOUNT.into()
                            },
                        }
                    );
                } else {
                    assert_eq!(record, empty_ledger);
                }
            };
            let mut land = |env: &mut V16CuEnv,
                            tx: Transaction,
                            rejection: Option<(u8, u32)>,
                            transfers: usize| {
                tx.verify().unwrap();
                let before: Vec<_> = keys
                    .iter()
                    .chain(&tx.message.account_keys)
                    .map(|key| (*key, env.svm.get_account(key)))
                    .collect();
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let result = env.svm.send_transaction(tx);
                let meta = if let Some((index, error)) = rejection {
                    let error_result =
                        result.expect_err("rejection must preserve the entire stock round trip");
                    assert_eq!(
                        error_result.err,
                        TransactionError::InstructionError(index, InstructionError::Custom(error))
                    );
                    for (key, account) in before {
                        if key != env.payer.pubkey() {
                            assert_eq!(
                                env.svm.get_account(&key),
                                account,
                                "exact rollback at {key}"
                            );
                        }
                    }
                    rollbacks += 1;
                    rolled_back_transfers += transfers;
                    error_result.meta
                } else {
                    result.expect("unconsumed stock continuation stays live")
                };
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", spl_token::ID))
                        .count(),
                    transfers
                );
                assert_cu_within(
                    "insurance round trip",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
            };

            // Both alternate successful envelopes exist before any attempt; never rebind them.
            let pending = [0, 1].map(|route| {
                signed(
                    &env,
                    &round_trip(&retained[route]),
                    10 + route as u32,
                    false,
                )
            });
            let pending_wire = pending.each_ref().map(|tx| bincode::serialize(tx).unwrap());
            check(&env, [AMOUNT.into(), 0, PEER.into(), 0], 0, false);
            let mut duplicate = round_trip(&retained[0]);
            // The second withdrawal supplies a funded source for the stale top-up's preflight.
            duplicate.extend(round_trip(&retained[1]));
            let tx = signed(&env, &duplicate, 1, false);
            land(
                &mut env,
                tx,
                Some((5, PercolatorError::EngineStale as u32)),
                3,
            );
            check(&env, [AMOUNT.into(), 0, PEER.into(), 0], 0, false);
            let mut late_error = round_trip(&retained[0]);
            late_error.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &source,
                    &sink,
                    &env.admin.pubkey(),
                    &[],
                    1,
                )
                .unwrap(),
            );
            let tx = signed(&env, &late_error, 2, false);
            land(
                &mut env,
                tx,
                Some((4, spl_token::error::TokenError::InsufficientFunds as u32)),
                2,
            );
            check(&env, [AMOUNT.into(), 0, PEER.into(), 0], 0, false);

            assert_eq!(bincode::serialize(&pending[1]).unwrap(), pending_wire[1]);
            land(&mut env, pending[1].clone(), None, 2);
            let distribution = |direct| {
                if direct {
                    [18, 19, PEER.into(), 0]
                } else {
                    [0, 37, PEER.into(), 0]
                }
            };
            check(&env, distribution(!direct_first), 1, false);
            assert_eq!(
                tokens.map(|key| env.svm.get_account(&key).unwrap()),
                token_frames,
                "committed round trip restores identical custody but consumes funding consent"
            );
            assert_eq!(bincode::serialize(&pending[0]).unwrap(), pending_wire[0]);
            land(
                &mut env,
                pending[0].clone(),
                Some((3, PercolatorError::EngineStale as u32)),
                1,
            );
            check(&env, distribution(!direct_first), 1, false);
            let fresh = refill(direct_first, 3);
            let tx = signed(&env, &round_trip(&fresh), 3, false);
            land(&mut env, tx, None, 2);
            check(&env, distribution(direct_first), 2, false);
            for (route, top_up) in retained.iter().enumerate() {
                let tx = signed(&env, &round_trip(top_up), 4 + route as u32, false);
                land(
                    &mut env,
                    tx,
                    Some((3, PercolatorError::EngineStale as u32)),
                    1,
                );
                check(&env, distribution(direct_first), 2, false);
            }

            // Cross the admission route with no role/epoch change and enough peer custody
            // to pass token preflight. Rejection must come from this asset's depleted stock.
            let mut unsigned = withdrawal.clone();
            unsigned.accounts[0].is_signer = false;
            if direct_first {
                let tx = signed(&env, &[withdrawal.clone()], 6, false);
                land(&mut env, tx, None, 1);
                check(&env, [0, 0, PEER.into(), 0], 2, true);
            }
            env.resolve();
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            if !direct_first {
                let tx = signed(&env, &[unsigned.clone()], 7, true);
                land(&mut env, tx, None, 1);
            }
            check(&env, [0, 0, PEER.into(), 0], 2, true);
            let retry = if direct_first {
                unsigned
            } else {
                withdrawal.clone()
            };
            let tx = signed(&env, &[retry], 8, direct_first);
            land(
                &mut env,
                tx,
                Some((2, PercolatorError::EngineLockActive as u32)),
                0,
            );
            check(&env, [0, 0, PEER.into(), 0], 2, true);
            let peer = Instruction {
                accounts: withdrawal.accounts[..6].to_vec(),
                data: ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 1,
                    market_id: market_ids[1],
                    authority_epoch: controls[1].authority_epoch,
                    amount: PEER.into(),
                }
                .encode(),
                program_id,
            };
            let tx = signed(&env, &[peer], 9, false);
            land(&mut env, tx, None, 1);
            check(&env, [0; 4], 2, true);
        }
    }
    assert_eq!((rollbacks, rolled_back_transfers), (24, 32));
    println!("INV-008 insurance round trip: 4 histories, 24 exact rollbacks, 32 rolled-back SPL transfers, peak {peak_cu} CU; row428 OPEN");
}
