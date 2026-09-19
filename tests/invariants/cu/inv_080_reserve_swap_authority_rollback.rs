//! INV-080: a retained reserve swap rejects after two successful token CPIs and
//! an A-B-A authority handoff. Rollback restores both custody rails and consent.
//! Both native orientations use public construction after native-mint genesis;
//! exact Account images include rent, unsynced SOL, mint supply and network fees.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_retained_swap_epoch_error_restores_native_custody_and_authority_roundtrip() {
    const AMOUNT: u64 = 37;
    const FUNDS: u64 = 4 * AMOUNT;
    const UNSYNCED: u64 = 11;
    let mut peak = 0;
    for native_primary in [false, true] {
        let mut env = inv081_public_native_market();
        let admin = env.admin.insecure_clone();
        let interim = Keypair::new();
        env.svm.airdrop(&interim.pubkey(), 1_000_000).unwrap();
        let native = env.mint;
        let spl = inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 9);
        let mints = if native_primary {
            [native, spl]
        } else {
            [spl, native]
        };
        env.send(
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: mints[0].to_bytes(),
                secondary_mint: mints[1].to_bytes(),
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new_readonly(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(mints[0], false),
                AccountMeta::new_readonly(mints[1], false),
                AccountMeta::new_readonly(env.vault, false),
            ],
            &[&admin],
        )
        .unwrap();
        let vaults = mints.map(|mint| canonical_vault_ata(env.vault_authority, mint));
        create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, spl);
        let wallets =
            mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint));
        let custody = [wallets[0], vaults[0], vaults[1], wallets[1]];
        for (mint, key) in [(mints[0], wallets[0]), (mints[1], vaults[1])] {
            let funding = if mint == native {
                vec![
                    system_instruction::transfer(&admin.pubkey(), &key, FUNDS),
                    spl_token::instruction::sync_native(&spl_token::ID, &key).unwrap(),
                ]
            } else {
                vec![spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mint,
                    &key,
                    &admin.pubkey(),
                    &[],
                    FUNDS,
                )
                .unwrap()]
            };
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();
        }
        // Surplus SOL stays outside the native token amount on both transfer endpoints.
        let native_keys = if native_primary {
            &custody[..2]
        } else {
            &custody[2..]
        };
        let mut seal_and_donate = vec![spl_token::instruction::set_authority(
            &spl_token::ID,
            &spl,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap()];
        seal_and_donate.extend(
            native_keys
                .iter()
                .map(|key| system_instruction::transfer(&admin.pubkey(), key, UNSYNCED)),
        );
        send_raw_ixs(&mut env.svm, &env.payer, seal_and_donate, &[&admin]).unwrap();
        for (i, key) in custody.iter().enumerate() {
            let account = env.svm.get_account(key).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(token.amount, if i % 2 == 0 { FUNDS } else { 0 });
            if let COption::Some(rent) = token.is_native {
                assert_eq!(account.lamports, rent + token.amount + UNSYNCED);
            }
        }
        let sequences = env.control_sequences(0);
        let swap = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(admin.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(wallets[0], false),
                AccountMeta::new(vaults[0], false),
                AccountMeta::new(wallets[1], false),
                AccountMeta::new(vaults[1], false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::SwapSecondaryForPrimary {
                amount: AMOUNT.into(),
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        };
        let handoff = |from: &Keypair, to: &Keypair, authority_epoch| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(from.pubkey(), true),
                AccountMeta::new_readonly(to.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            data: ProgInstruction::UpdateAuthority {
                authority_epoch,
                new_pubkey: to.pubkey().to_bytes(),
            }
            .encode(),
        };
        let prefix = [
            swap.clone(),
            handoff(&admin, &interim, sequences.authority_epoch),
            handoff(&interim, &admin, sequences.authority_epoch + 1),
        ];
        let sign = |ixs: &[Instruction], roundtrip: bool| {
            let mut instructions = vec![heap_ix(), cu_ix()];
            instructions.extend_from_slice(ixs);
            let mut signers = vec![&env.payer, &admin];
            if roundtrip {
                signers.push(&interim);
            }
            let tx = Transaction::new_signed_with_payer(
                &instructions,
                Some(&env.payer.pubkey()),
                &signers,
                env.svm.latest_blockhash(),
            );
            tx.verify().unwrap();
            assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
            tx
        };
        // Capture all signatures before execution. Retry never refreshes consent or blockhash.
        let retained = sign(std::slice::from_ref(&swap), false);
        let retained_bytes = bincode::serialize(&retained).unwrap();
        let valid_prefix = sign(&prefix, true);
        let mut rejected = prefix.to_vec();
        rejected.push(swap);
        let rejected = sign(&rejected, true);
        let mut keys = rejected.message.account_keys.clone();
        keys.extend(mints);
        keys.sort_unstable();
        keys.dedup();

        let mut land = |env: &mut V16CuEnv, tx: Transaction, reject: bool, roundtrip: bool| {
            let mut expected: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            for (key, account) in keys.iter().zip(&mut expected) {
                if *key == env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= FeeStructure::default()
                        .lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                }
                if reject {
                    continue;
                }
                if let Some(i) = custody.iter().position(|k| k == key) {
                    let raw = account.as_mut().unwrap();
                    let mut token = TokenAccount::unpack(&raw.data).unwrap();
                    if i % 2 == 0 {
                        token.amount -= AMOUNT;
                        if token.is_native.is_some() {
                            raw.lamports -= AMOUNT;
                        }
                    } else {
                        token.amount += AMOUNT;
                        if token.is_native.is_some() {
                            raw.lamports += AMOUNT;
                        }
                    }
                    TokenAccount::pack(token, &mut raw.data).unwrap();
                }
                if roundtrip && *key == env.market {
                    let mut next = sequences;
                    next.authority_epoch += 2;
                    state::write_asset_control_sequences(
                        &mut account.as_mut().unwrap().data,
                        0,
                        &next,
                    )
                    .unwrap();
                }
            }
            let result = env.svm.send_transaction(tx);
            let meta = if reject {
                let failure = result.expect_err(
                    "retained swap must return the epoch error after the successful prefix",
                );
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        5,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )
                );
                failure.meta
            } else {
                result.expect("unchanged pre-signed continuation must remain executable")
            };
            for (program, successes) in [
                (env.program_id, if roundtrip { 3 } else { 1 }),
                (spl_token::ID, 2),
            ] {
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    successes
                );
            }
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| line.as_str() == "Program log: Instruction: Transfer")
                    .count(),
                2
            );
            for (key, account) in keys.iter().zip(expected) {
                assert_eq!(
                    env.svm.get_account(key),
                    account,
                    "native_primary={native_primary}, exact Account {key}"
                );
            }
            assert_cu_within(
                "reserve swap authority rollback",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            peak = peak.max(meta.compute_units_consumed);
        };
        land(&mut env, rejected, true, true);
        assert_eq!(env.control_sequences(0), sequences);
        assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
        land(&mut env, retained, false, false);
        // Committing the identical prefix proves both handoffs and both transfers were valid.
        land(&mut env, valid_prefix, false, true);
        assert_eq!(
            custody.map(|key| env.token_amount(key)),
            [
                FUNDS - 2 * AMOUNT,
                2 * AMOUNT,
                FUNDS - 2 * AMOUNT,
                2 * AMOUNT
            ]
        );
        assert_eq!(env.market_state().0.marketauth, admin.pubkey().to_bytes());
        assert_eq!(
            env.control_sequences(0).authority_epoch,
            sequences.authority_epoch + 2
        );
    }
    println!("INV-080 reserve swap ABA: 2 native orientations, 2 exact epoch rollbacks, 4 committed swaps; peak {peak} CU");
}
