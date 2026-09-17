//! Row 428 / INV-064: consumed Live consent stays stale after independently
//! funded native replenishment and resolution, on both quote withdrawal rails.
//! Distinct owners retain independent asset epochs, ledgers and destinations.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

const ORIGINAL: u64 = 37;
const REFILL: u64 = 83;
const PEER: u64 = 61;
const SECONDARY: u64 = 200;
const LIMIT: u32 = 200_000;

#[test]
fn v16_native_replenishment_preserves_consumed_consent_across_resolved_quote_routes() {
    let mut peak = 0;
    let mut transactions = 0;
    let mut rollbacks = 0;
    let mut max_packet = 0;
    for first_rail in 0..2 {
        for signed_partial in [false, true] {
            let mut env = inv081_public_native_market_with_params(
                2,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let owners = [Keypair::new(), Keypair::new()];
            let wallets = owners.each_ref().map(|owner| owner.pubkey());
            let secondary = inv018_create_public_spl_mint(
                &mut env.svm,
                &env.payer,
                admin.pubkey(),
                spl_token::native_mint::DECIMALS,
            );
            env.update_base_unit_mints_with_cu(env.mint, secondary);
            for (asset, owner) in owners.iter().enumerate() {
                env.svm.airdrop(&owner.pubkey(), 1_000_000).unwrap();
                for kind in [
                    processor::ASSET_AUTH_INSURANCE,
                    processor::ASSET_AUTH_INSURANCE_OPERATOR,
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(owner),
                        asset as u16,
                        kind,
                        owner.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
            }
            let mints = [env.mint, secondary];
            let vaults = [
                env.vault,
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
            ];
            let recipients = wallets.map(|owner| {
                mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, owner, mint))
            });
            let sources = wallets.map(|owner| {
                let key = Keypair::new();
                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::create_account(
                        &env.payer.pubkey(),
                        &key.pubkey(),
                        rent,
                        TokenAccount::LEN as u64,
                        &spl_token::ID,
                    ),
                    &[&key],
                )
                .unwrap();
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &key.pubkey(),
                        &env.mint,
                        &owner,
                    )
                    .unwrap(),
                    &[],
                )
                .unwrap();
                key.pubkey()
            });
            let ledgers = wallets.map(|_| {
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    state::insurance_ledger_account_len(),
                    env.program_id,
                );
                key.pubkey()
            });
            for (source, amount) in sources.into_iter().zip([ORIGINAL + REFILL, PEER]) {
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::transfer(&env.payer.pubkey(), &source, amount),
                        spl_token::instruction::sync_native(&spl_token::ID, &source).unwrap(),
                    ],
                    &[],
                )
                .unwrap();
            }
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &secondary,
                        &vaults[1],
                        &admin.pubkey(),
                        &[],
                        SECONDARY,
                    )
                    .unwrap(),
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &secondary,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                ],
                &[&admin],
            )
            .unwrap();
            let program = env.program_id;
            let market = env.market;
            let vault_authority = env.vault_authority;
            let ids = [env.asset_market_id(0), env.asset_market_id(1)];
            let initial_controls = [env.control_sequences(0), env.control_sequences(1)];
            let top_up = |asset: usize, domain, epoch, intent, amount| Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new_readonly(wallets[asset], true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(sources[asset], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledgers[asset], false),
                ],
                data: ProgInstruction::TopUpInsuranceDomain {
                    domain,
                    market_id: ids[asset],
                    authority_epoch: epoch,
                    intent_id: intent,
                    amount,
                }
                .encode(),
            };
            for asset in 0..2 {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    top_up(
                        asset,
                        2 * asset as u16,
                        initial_controls[asset].authority_epoch,
                        initial_controls[asset].insurance_top_up + 1,
                        [ORIGINAL, PEER][asset].into(),
                    ),
                    &[&owners[asset]],
                )
                .unwrap();
            }
            let initial = env.market_state();
            let controls = [env.control_sequences(0), env.control_sequences(1)];
            let epochs = controls.map(|control| control.authority_epoch);
            let slot = env.svm.get_sysvar::<Clock>().slot;
            let withdrawal = |asset: usize, epoch, amount, rail: usize, signed| Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new_readonly(wallets[asset], signed),
                    AccountMeta::new(market, false),
                    AccountMeta::new(recipients[asset][rail], false),
                    AccountMeta::new(vaults[rail], false),
                    AccountMeta::new_readonly(vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledgers[asset], false),
                ],
                data: ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: asset as u16,
                    market_id: ids[asset],
                    authority_epoch: epoch,
                    amount,
                }
                .encode(),
            };
            let original = withdrawal(0, epochs[0], ORIGINAL.into(), first_rail, true);
            let stale_signed = withdrawal(0, epochs[0], ORIGINAL.into(), 1 - first_rail, true);
            let stale_unsigned = withdrawal(0, epochs[0], ORIGINAL.into(), 1 - first_rail, false);
            assert_eq!(original.data, stale_signed.data);
            assert_eq!(original.data, stale_unsigned.data);
            let refill = top_up(
                0,
                1,
                epochs[0] + 1,
                controls[0].insurance_top_up + 1,
                REFILL.into(),
            );
            let partial = withdrawal(0, epochs[0] + 1, 19, 1 - first_rail, signed_partial);
            let peer_partial = withdrawal(1, epochs[1], 11, 0, false);
            let resolve = Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new_readonly(admin.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::ResolveMarket {
                    asset_generation_frontier: initial.1.next_market_id,
                    authority_epoch: epochs[0] + 1,
                }
                .encode(),
            };
            let batches = [
                vec![original],
                vec![refill.clone(), stale_signed.clone()],
                vec![refill],
                vec![resolve],
                vec![stale_signed],
                vec![stale_unsigned.clone()],
                vec![partial.clone(), peer_partial.clone(), stale_unsigned],
                vec![partial, peer_partial],
                vec![withdrawal(
                    0,
                    epochs[0] + 1,
                    19,
                    1 - first_rail,
                    !signed_partial,
                )],
                vec![withdrawal(0, epochs[0] + 2, 64, 1 - first_rail, false)],
                vec![withdrawal(1, epochs[1] + 1, 50, 0, false)],
                vec![withdrawal(0, epochs[0] + 3, 1, 1 - first_rail, false)],
            ];
            // Every envelope, including successor epochs and opposite signer variants,
            // is fixed before the first debit. Unique limits avoid cache-only rejection.
            let envelopes: Vec<_> = batches
                .iter()
                .enumerate()
                .map(|(index, batch)| {
                    let mut instructions = vec![
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT - index as u32),
                    ];
                    instructions.extend_from_slice(batch);
                    let mut signers = vec![&env.payer];
                    for signer in [&admin, &owners[0], &owners[1]] {
                        if batch
                            .iter()
                            .flat_map(|ix| &ix.accounts)
                            .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                        {
                            signers.push(signer);
                        }
                    }
                    let tx = Transaction::new_signed_with_payer(
                        &instructions,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    let bytes = bincode::serialize(&tx).unwrap();
                    max_packet = max_packet.max(bytes.len());
                    assert!(bytes.len() <= 1_232);
                    assert_eq!(
                        usize::from(tx.message.header.num_required_signatures),
                        signers.len()
                    );
                    bytes
                })
                .collect();
            assert_eq!(
                bincode::deserialize::<Transaction>(&envelopes[7])
                    .unwrap()
                    .message
                    .header
                    .num_required_signatures,
                1 + u8::from(signed_partial)
            );
            for index in [5, 9, 10, 11] {
                assert_eq!(
                    bincode::deserialize::<Transaction>(&envelopes[index])
                        .unwrap()
                        .message
                        .header
                        .num_required_signatures,
                    1
                );
            }
            let tokens = [
                sources[0],
                sources[1],
                vaults[0],
                vaults[1],
                recipients[0][0],
                recipients[0][1],
                recipients[1][0],
                recipients[1][1],
            ];
            let mut tracked = vec![market, env.payer.pubkey(), admin.pubkey(), vault_authority];
            tracked.extend(tokens);
            tracked.extend(mints);
            tracked.extend(wallets);
            tracked.extend(ledgers);
            let frames: BTreeMap<_, _> = tracked
                .iter()
                .map(|key| (*key, env.svm.get_account(key)))
                .collect();
            let profiles = [0, 1].map(|asset| {
                state::read_asset_oracle_profile(&frames[&market].as_ref().unwrap().data, asset)
                    .unwrap()
            });
            let mut paid = [[0u64; 2]; 2];
            let mut debits = [0u64; 2];
            let mut replenished = false;
            let mut resolved = false;
            let mut history_peak = 0;
            let check = |env: &V16CuEnv,
                         paid: [[u64; 2]; 2],
                         debits: [u64; 2],
                         replenished: bool,
                         resolved: bool| {
                let deposited = [ORIGINAL + if replenished { REFILL } else { 0 }, PEER];
                let withdrawn = paid.map(|rails| rails.iter().sum::<u64>());
                let stock = [deposited[0] - withdrawn[0], deposited[1] - withdrawn[1]];
                let remaining = stock.iter().sum::<u64>();
                let mut expected = initial.clone();
                expected.1.insurance = remaining.into();
                expected.1.vault = remaining.into();
                expected.1.insurance_domain_budget_remaining_total = remaining.into();
                expected.1.insurance_domain_budget = vec![
                    if replenished { 0 } else { stock[0].into() },
                    if replenished { stock[0].into() } else { 0 },
                    stock[1].into(),
                    0,
                ];
                if resolved {
                    expected.1.mode = MarketModeV16::Resolved;
                    expected.1.current_slot = slot;
                    expected.1.resolved_slot = slot;
                }
                assert_eq!(
                    env.market_state(),
                    expected,
                    "complete economic and policy state"
                );
                assert_eq!(
                    (
                        expected.0._reserved_insurance_withdraw_max_bps,
                        expected.0._reserved_insurance_withdraw_deposits_only,
                        expected.0._reserved_insurance_withdraw_cooldown_slots,
                        expected.0._reserved_last_insurance_withdraw_slot
                    ),
                    (0, 0, 0, 0)
                );
                let market_account = env.svm.get_account(&market).unwrap();
                for asset in 0..2 {
                    let mut sequence = controls[asset];
                    sequence.authority_epoch += debits[asset];
                    sequence.insurance_top_up += u64::from(asset == 0 && replenished);
                    assert_eq!(env.control_sequences(asset), sequence);
                    assert_eq!(
                        state::read_asset_oracle_profile(&market_account.data, asset).unwrap(),
                        profiles[asset]
                    );
                    let record = state::InsuranceLedgerAccountV16 {
                        market_group: market.to_bytes(),
                        authority: wallets[asset].to_bytes(),
                        total_principal_atoms: stock[asset].into(),
                        total_deposited_atoms: deposited[asset].into(),
                        total_withdrawn_atoms: withdrawn[asset].into(),
                        cumulative_profit_atoms: 0,
                        cumulative_loss_atoms: 0,
                        last_observed_insurance_atoms: stock[asset].into(),
                    };
                    let mut ledger_frame = frames[&ledgers[asset]].clone().unwrap();
                    state::write_insurance_ledger(&mut ledger_frame.data, &record).unwrap();
                    assert_eq!(env.svm.get_account(&ledgers[asset]), Some(ledger_frame));
                    assert_eq!(withdrawn[asset] + stock[asset], deposited[asset]);
                }
                let physical = [
                    deposited.iter().sum::<u64>() - paid[0][0] - paid[1][0],
                    SECONDARY - paid[0][1] - paid[1][1],
                ];
                let amounts = [
                    if replenished { 0 } else { REFILL },
                    0,
                    physical[0],
                    physical[1],
                    paid[0][0],
                    paid[0][1],
                    paid[1][0],
                    paid[1][1],
                ];
                for (key, amount) in tokens.into_iter().zip(amounts) {
                    let mut frame = frames[&key].clone().unwrap();
                    let mut token = TokenAccount::unpack(&frame.data).unwrap();
                    if let COption::Some(rent) = token.is_native {
                        assert_eq!(frame.lamports, rent + token.amount);
                        frame.lamports = rent + amount;
                    }
                    token.amount = amount;
                    TokenAccount::pack(token, &mut frame.data).unwrap();
                    assert_eq!(
                        env.svm.get_account(&key),
                        Some(frame),
                        "complete custody frame {key}"
                    );
                }
                assert_eq!(
                    amounts[0] + amounts[1] + physical[0] + paid[0][0] + paid[1][0],
                    ORIGINAL + REFILL + PEER
                );
                assert_eq!(physical[1] + paid[0][1] + paid[1][1], SECONDARY);
                let mint = Mint::unpack(&env.svm.get_account(&secondary).unwrap().data).unwrap();
                assert_eq!(
                    (mint.supply, mint.mint_authority),
                    (SECONDARY, COption::None)
                );
                for key in [
                    mints[0],
                    mints[1],
                    wallets[0],
                    wallets[1],
                    admin.pubkey(),
                    vault_authority,
                ] {
                    assert_eq!(env.svm.get_account(&key), frames[&key]);
                }
                let mut metadata = market_account.clone();
                metadata.data = frames[&market].as_ref().unwrap().data.clone();
                assert_eq!(Some(metadata), frames[&market]);
                assert_market_stock_census(
                    "row428 native replenishment",
                    &expected.1,
                    &market_account.data,
                    &[],
                    remaining.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "row428 native replenishment",
                    &expected.1,
                    &[],
                )
                .unwrap();
                let mut data = market_account.data;
                state::market_view_mut(&mut data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            check(&env, paid, debits, replenished, resolved);
            for (index, bytes) in envelopes.into_iter().enumerate() {
                if matches!(index, 4 | 5 | 8 | 11) {
                    let amount = match index {
                        4 | 5 => ORIGINAL,
                        8 => 19,
                        _ => 1,
                    };
                    assert!(env.token_amount(vaults[1 - first_rail]) >= amount);
                    if index != 11 {
                        let group = env.market_state().1;
                        assert!(
                            group.insurance_domain_budget[0] + group.insurance_domain_budget[1]
                                >= amount.into()
                        );
                    }
                }
                let tx: Transaction = bincode::deserialize(&bytes).unwrap();
                assert_eq!(bincode::serialize(&tx).unwrap(), bytes);
                tx.verify().unwrap();
                let mut keys = tx.message.account_keys.clone();
                keys.extend_from_slice(&tracked);
                keys.sort_unstable();
                keys.dedup();
                let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
                let mut payer_frame = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer_frame.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let rejection = match index {
                    1 => Some((3, PercolatorError::EngineStale)),
                    4 | 5 | 8 => Some((2, PercolatorError::EngineStale)),
                    6 => Some((4, PercolatorError::EngineStale)),
                    11 => Some((2, PercolatorError::EngineLockActive)),
                    _ => None,
                };
                let result = env.svm.send_transaction(tx);
                let meta = if let Some((error_index, error)) = rejection {
                    let failure = result.expect_err("retained or exhausted consent must reject");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            error_index,
                            InstructionError::Custom(error as u32)
                        ),
                        "step {index}"
                    );
                    for (key, frame) in keys.iter().zip(before) {
                        if *key != env.payer.pubkey() {
                            assert_eq!(
                                env.svm.get_account(key),
                                frame,
                                "step {index}: complete rollback {key}"
                            );
                        }
                    }
                    rollbacks += 1;
                    failure.meta
                } else {
                    let meta = result.unwrap_or_else(|err| panic!("step {index}: {err:?}"));
                    match index {
                        0 => {
                            paid[0][first_rail] += ORIGINAL;
                            debits[0] += 1;
                        }
                        2 => replenished = true,
                        3 => resolved = true,
                        7 => {
                            paid[0][1 - first_rail] += 19;
                            debits[0] += 1;
                            paid[1][0] += 11;
                            debits[1] += 1;
                        }
                        9 => {
                            paid[0][1 - first_rail] += 64;
                            debits[0] += 1;
                        }
                        10 => {
                            paid[1][0] += 50;
                            debits[1] += 1;
                        }
                        _ => unreachable!(),
                    }
                    meta
                };
                let completed = match index {
                    0 | 1 | 2 | 9 | 10 => [1, 1],
                    3 => [1, 0],
                    6 | 7 => [2, 2],
                    _ => [0, 0],
                };
                for (program_id, count) in [program, spl_token::ID].into_iter().zip(completed) {
                    assert_eq!(
                        meta.logs
                            .iter()
                            .filter(|line| **line == format!("Program {program_id} success"))
                            .count(),
                        count,
                        "step {index}: completed prefix"
                    );
                }
                assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer_frame));
                assert!(meta.compute_units_consumed > 0);
                assert_cu_within(
                    "row428 native replenishment",
                    meta.compute_units_consumed,
                    LIMIT.into(),
                );
                history_peak = history_peak.max(meta.compute_units_consumed);
                transactions += 1;
                check(&env, paid, debits, replenished, resolved);
            }
            assert_eq!(
                paid.map(|rails| rails.iter().sum::<u64>()),
                [ORIGINAL + REFILL, PEER]
            );
            assert_eq!(env.market_state().1.insurance, 0);
            assert_eq!(debits, [3, 2]);
            peak = peak.max(history_peak);
            eprintln!("row428 native replenishment: first_rail={first_rail}, signed_partial={signed_partial}, peak={history_peak} CU");
        }
    }
    assert_eq!((transactions, rollbacks), (48, 24));
    eprintln!("row428 native replenishment: 4 histories, {transactions} transactions, {rollbacks} exact rollbacks, 12 restored transfers, 20 payouts, peak={peak} CU, max_packet={max_packet}");
}
