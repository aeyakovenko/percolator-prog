//! INV-014/008: a successful reserve debit consumes its authorizing epoch.
//! The Live selector is a control; the Resolved selector covers the terminal path
//! where payout is permissionless but the signed epoch must still be one-shot.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::fee::FeeStructure;

fn check_reserve_debit_epoch(resolved: bool) {
    const BUDGETS: [u128; 4] = [19, 41, 23, 47];
    const DEBIT: u128 = 29;
    let supply = BUDGETS.iter().sum::<u128>();
    let mut violations = Vec::new();
    let mut peak_cu = 0;

    for asset in 0..2 {
        for telemetry in [false, true] {
            let label = format!("resolved={resolved}, asset={asset}, telemetry={telemetry}");
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let wallet = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &wallet,
                    &admin.pubkey(),
                    &[],
                    supply as u64,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            for (domain, amount) in BUDGETS.into_iter().enumerate() {
                let controls = env.control_sequences(domain / 2);
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: domain as u16,
                        market_id: env.asset_market_id(domain as u16 / 2),
                        authority_epoch: controls.authority_epoch,
                        intent_id: next_control_sequence(controls.insurance_top_up),
                        amount,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallet, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&admin],
                )
                .unwrap();
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let ledger = telemetry.then(Keypair::new);
            if let Some(ledger) = &ledger {
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    ledger,
                    state::insurance_ledger_account_len(),
                    env.program_id,
                );
            }
            if resolved {
                env.resolve();
            }
            let mode = if resolved {
                MarketModeV16::Resolved
            } else {
                MarketModeV16::Live
            };
            let before = env.market_state().1;
            assert_eq!(before.mode, mode);
            assert_eq!(before.insurance_domain_budget, BUDGETS);
            assert_eq!((before.c_tot, before.materialized_portfolio_count), (0, 0));
            assert_eq!((before.insurance, before.vault), (supply, supply));
            assert_eq!(env.token_amount(wallet), 0);
            let controls = [0, 1].map(|index| env.control_sequences(index));
            let profiles = [0, 1].map(|index| {
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    index,
                )
                .unwrap()
            });
            let framed_keys = [env.mint, admin.pubkey(), env.vault_authority];
            let framed = framed_keys.map(|key| env.svm.get_account(&key));
            let mut accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(wallet, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            if let Some(ledger) = &ledger {
                accounts.push(AccountMeta::new(ledger.pubkey(), false));
            }
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    cu_ix(),
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: asset as u16,
                            market_id: env.asset_market_id(asset as u16),
                            authority_epoch: controls[asset].authority_epoch,
                            amount: DEBIT,
                        }
                        .encode(),
                    },
                ],
                Some(&env.payer.pubkey()),
                &[&env.payer, &admin],
                env.svm.latest_blockhash(),
            );
            tx.verify().unwrap();
            assert!(
                bincode::serialized_size(&tx).unwrap()
                    <= solana_sdk::packet::PACKET_DATA_SIZE as u64
            );
            let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature
                * u64::from(tx.message.header.num_required_signatures);
            let meta = env
                .svm
                .send_transaction(tx)
                .expect("ordinary authorized reserve payout");
            assert_cu_within(&label, meta.compute_units_consumed, CUSTODY_CU_LIMIT);
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
            assert_eq!(framed_keys.map(|key| env.svm.get_account(&key)), framed);

            let mut budgets = BUDGETS;
            let long = budgets[2 * asset].min(DEBIT);
            budgets[2 * asset] -= long;
            budgets[2 * asset + 1] -= DEBIT - long;
            let after = env.market_state().1;
            assert_eq!(after.mode, mode);
            assert_eq!(after.insurance_domain_budget, budgets);
            assert_eq!(after.insurance_domain_spent, [0; 4]);
            assert_eq!(
                after.insurance_domain_budget_remaining_total,
                supply - DEBIT
            );
            assert_eq!(
                (after.insurance, after.vault),
                (supply - DEBIT, supply - DEBIT)
            );
            assert_eq!((after.c_tot, after.materialized_portfolio_count), (0, 0));
            assert_eq!(env.token_amount(wallet), DEBIT as u64);
            assert_eq!(env.token_amount(env.vault), (supply - DEBIT) as u64);
            for index in 0..2 {
                assert_eq!(
                    after.assets[index].market_id,
                    before.assets[index].market_id
                );
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        index,
                    )
                    .unwrap(),
                    profiles[index],
                );
                let mut expected = controls[index];
                if index == asset {
                    expected.authority_epoch += 1;
                }
                let actual = env.control_sequences(index);
                if actual != expected {
                    violations.push(format!(
                        "{label}, control asset={index}: expected {expected:?}, got {actual:?}"
                    ));
                }
            }
            if let Some(ledger) = ledger {
                assert_eq!(
                    state::read_insurance_ledger(
                        &env.svm.get_account(&ledger.pubkey()).unwrap().data
                    )
                    .unwrap(),
                    state::InsuranceLedgerAccountV16 {
                        market_group: env.market.to_bytes(),
                        authority: admin.pubkey().to_bytes(),
                        total_principal_atoms: 0,
                        total_deposited_atoms: 0,
                        total_withdrawn_atoms: DEBIT,
                        cumulative_profit_atoms: 0,
                        cumulative_loss_atoms: 0,
                        last_observed_insurance_atoms: BUDGETS[2 * asset] + BUDGETS[2 * asset + 1]
                            - DEBIT,
                    },
                );
            }
        }
    }
    eprintln!(
        "reserve debit epoch: resolved={resolved}, payouts=4, epoch violations={}, peak CU={peak_cu}",
        violations.len()
    );
    assert!(
        violations.is_empty(),
        "each paid debit must consume exactly its authorizing epoch:\n{}",
        violations.join("\n")
    );
}

#[test]
fn v16_live_insurance_debit_consumes_reserve_authority_epoch() {
    check_reserve_debit_epoch(false);
}

#[test]
fn v16_resolved_insurance_debit_consumes_reserve_authority_epoch() {
    check_reserve_debit_epoch(true);
}
