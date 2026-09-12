//! INV-002/007/012/080/081/089: only committed replacement consumes a generation.
//! An SPL withdrawal, matured activation and sibling CPI disappear together when
//! retained oracle management names the retired generation. Original sibling consent stays usable.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

pub(super) fn sign(h: &History, instructions: &[Instruction]) -> Transaction {
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    let mut signers = vec![&h.env.payer];
    signers.extend(
        [&h.env.admin, &h.owners[0], &h.owners[1]]
            .into_iter()
            .filter(|signer| {
                instructions
                    .iter()
                    .flat_map(|ix| &ix.accounts)
                    .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
            }),
    );
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&h.env.payer.pubkey()),
        &signers,
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    let bytes = bincode::serialized_size(&tx).unwrap();
    assert!(
        bytes <= solana_sdk::packet::PACKET_DATA_SIZE as u64,
        "transaction bytes={bytes}"
    );
    tx
}

fn lifecycle(h: &History, asset: u16, activate: bool) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.env.admin.pubkey(), true),
            AccountMeta::new(h.env.market, false),
        ],
        data: ProgInstruction::UpdateAssetLifecycle {
            action: if activate {
                processor::ASSET_ACTION_ACTIVATE
            } else {
                processor::ASSET_ACTION_RETIRE
            },
            asset_index: asset,
            market_id: if activate {
                h.next_id
            } else {
                h.ids[asset as usize]
            },
            authority_epoch: h.env.control_sequences(0).authority_epoch,
            now_slot: h.slot,
            initial_price: if activate { PRICE } else { 0 },
            max_init_fee: 0,
            insurance_authority: h.env.admin.pubkey().to_bytes(),
            insurance_operator: h.env.admin.pubkey().to_bytes(),
            backing_bucket_authority: h.env.admin.pubkey().to_bytes(),
            oracle_authority: h.env.admin.pubkey().to_bytes(),
        }
        .encode(),
    }
}

fn reject(h: &mut History, tx: Transaction, prefix: usize) -> u64 {
    let keys = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.mint,
            h.env.vault,
            h.tokens[0],
            h.tokens[1],
            h.portfolios[0],
            h.portfolios[1],
            h.matcher.0,
            h.matcher.1,
            h.matcher.2,
        ])
        .collect::<std::collections::BTreeSet<_>>();
    let mut before = keys
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let failure =
        h.env.svm.send_transaction(tx).expect_err(
            "old-generation management rejects after the valid activation and CPI prefix",
        );
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            (2 + prefix) as u8,
            InstructionError::Custom(PercolatorError::AssetGenerationMismatch as u32)
        )
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", h.env.program_id))
            .count(),
        prefix
    );
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line == &format!("Program {} success", spl_token::ID)));
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line == &format!("Program {} success", h.matcher.0)));
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| line.starts_with(&format!("Program {} invoke", h.matcher.0)))
            .count(),
        1
    );
    for (key, account) in &mut before {
        if *key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            h.env.svm.get_account(key),
            *account,
            "exact Account rollback: {key}"
        );
    }
    h.assert_state();
    assert!(failure.meta.compute_units_consumed <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    failure.meta.compute_units_consumed
}

#[test]
fn v16_program_failed_asset_activation_restores_retained_sibling_cpi_and_frontier() {
    let mut evidence = Evidence::default();
    let mut peak_rejection = 0;
    for asset in [1u16, 2] {
        for batch in [false, true] {
            for direction in [-1i128, 1] {
                let mut h = History::new();
                let management = ProgInstruction::ConfigureAuthMark {
                    asset_index: asset,
                    market_id: h.ids[asset as usize],
                    now_slot: h.slot,
                    initial_mark_e6: PRICE,
                    observation_sequence: next_control_sequence(
                        h.env.control_sequences(asset as usize).oracle_observation,
                    ),
                    authority_epoch: h.env.control_sequences(asset as usize).authority_epoch,
                };
                let stale_management = Instruction {
                    program_id: h.env.program_id,
                    accounts: vec![
                        AccountMeta::new(h.env.admin.pubkey(), true),
                        AccountMeta::new(h.env.market, false),
                    ],
                    data: management.encode(),
                };
                let before = h.frame();
                h.env
                    .svm
                    .simulate_transaction(sign(&h, &[stale_management.clone()]).into())
                    .expect("retained management is admissible before retirement");
                assert_eq!(h.frame(), before);
                let retirement = sign(&h, &[lifecycle(&h, asset, false)]);
                h.env
                    .svm
                    .send_transaction(retirement)
                    .expect("retire the empty slot before its cooldown");
                h.slot += 1;
                h.env.svm.warp_to_slot(h.slot);
                for index in 0..3 {
                    if index != asset {
                        h.env
                            .configure_auth_mark_for_asset_as_admin(index, h.slot, PRICE);
                    }
                }
                let sibling = 3 - asset;
                let route = if batch {
                    Route::Batch
                } else {
                    Route::Single(sibling)
                };
                let mut sizes = [0; 3];
                sizes[sibling as usize] = 2 * direction;
                let retained_ix = h.instruction(route, sizes, asset == 2);
                let retained = h.sign(&retained_ix);
                h.simulate(&retained, 0, &mut evidence);
                let withdrawal = Instruction {
                    program_id: h.env.program_id,
                    accounts: vec![
                        AccountMeta::new(h.owners[0].pubkey(), true),
                        AccountMeta::new(h.env.market, false),
                        AccountMeta::new(h.portfolios[0], false),
                        AccountMeta::new(h.tokens[0], false),
                        AccountMeta::new(h.env.vault, false),
                        AccountMeta::new_readonly(h.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: h.env.withdraw_ix(h.portfolios[0], 7).encode(),
                };
                let activation = lifecycle(&h, asset, true);
                let cpi = Instruction {
                    program_id: h.env.program_id,
                    accounts: vec![
                        AccountMeta::new(h.owners[0].pubkey(), true),
                        AccountMeta::new(h.env.market, false),
                        AccountMeta::new(h.portfolios[0], false),
                        AccountMeta::new(h.portfolios[1], false),
                        AccountMeta::new_readonly(h.matcher.0, false),
                        AccountMeta::new(h.matcher.1, false),
                        AccountMeta::new_readonly(h.matcher.2, false),
                    ],
                    data: retained_ix.encode(),
                };
                let prefix = [withdrawal, activation.clone(), cpi];
                let before = h.frame();
                h.env
                    .svm
                    .simulate_transaction(sign(&h, &prefix).into())
                    .expect("SPL, matured activation and sibling CPI are jointly admissible");
                assert_eq!(h.frame(), before);
                let mut bundle = prefix.to_vec();
                bundle.push(stale_management);
                let mut repaired = management;
                if let ProgInstruction::ConfigureAuthMark { market_id, .. } = &mut repaired {
                    *market_id = h.next_id;
                }
                let mut control = bundle.clone();
                control.last_mut().unwrap().data = repaired.encode();
                h.env
                    .svm
                    .simulate_transaction(sign(&h, &control).into())
                    .expect(
                        "repairing only the retired generation makes the full bundle admissible",
                    );
                assert_eq!(h.frame(), before);
                let tx = sign(&h, &bundle);
                peak_rejection = peak_rejection.max(reject(&mut h, tx, prefix.len()));
                h.simulate(&retained, 0, &mut evidence);
                let meta = h
                    .env
                    .svm
                    .send_transaction(retained)
                    .expect("the exact pre-signed CPI remains live after rollback");
                assert!(meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", h.matcher.0)));
                h.positions = sizes;
                h.epoch += 1;
                h.assert_state();
                h.fill(route, sizes.map(|size| -size), asset != 2, &mut evidence);

                // The same activation consumes precisely the frontier restored by
                // rollback; new owner consent then admits the replacement asset.
                let tx = sign(&h, &[activation]);
                h.env
                    .svm
                    .send_transaction(tx)
                    .expect("replacement retry consumes exactly one generation");
                h.ids[asset as usize] = h.next_id;
                h.next_id += 1;
                for index in 0..3 {
                    h.env
                        .configure_auth_mark_for_asset_as_admin(index, h.slot, PRICE);
                }
                h.assert_state();
                h.replace(GRANT, &mut evidence);
                let replacement_route = if batch {
                    Route::Batch
                } else {
                    Route::Single(asset)
                };
                if !batch {
                    sizes[sibling as usize] = 0;
                }
                sizes[asset as usize] = -3 * direction;
                h.fill(replacement_route, sizes, asset == 2, &mut evidence);
                h.fill(
                    replacement_route,
                    sizes.map(|size| -size),
                    asset != 2,
                    &mut evidence,
                );
                h.withdraw_all(asset == 2, &mut evidence);
                for actor in 0..2 {
                    let token = TokenAccount::unpack(
                        &h.env.svm.get_account(&h.tokens[actor]).unwrap().data,
                    )
                    .unwrap();
                    assert_eq!(token.owner, h.owners[actor].pubkey());
                    assert_eq!(token.mint, h.env.mint);
                    assert_eq!(token.amount as u128, CAPITAL);
                }
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    eprintln!("INV-002/012 generation rollback: worlds=8, rejected_bundles=8, rolled_back_spl_and_cpi_prefixes=8, restored_retained_fills=8, committed_replacements=8, owner_exits=16, peak_rejection_cu={peak_rejection}");
}
