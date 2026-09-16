//! Row 423: 42 distinct feed identities/accounts, maximum active/source shape,
//! two-chunk backlog, late feed-binding rollback, and complete owner payouts.
//! Lookup-table creation/extension uses public instructions, not account injection.

use super::*;
use solana_sdk::{
    address_lookup_table::{
        instruction as lookup_ix, state::AddressLookupTable, AddressLookupTableAccount,
    },
    fee::FeeStructure,
    instruction::InstructionError,
    message::{v0, VersionedMessage},
    transaction::{TransactionError, VersionedTransaction},
};

#[test]
fn v16_program_42_distinct_feeds_max_source_backlog_has_bounded_public_exit() {
    run_hybrid_source_backlog_public_exit(
        usize::from(percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS),
        true,
    );
}

pub(super) fn fund_public_portfolio(env: &mut V16CuEnv, owner: &Keypair) -> Pubkey {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio.pubkey();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .expect("public portfolio initialization");
    env.portfolios.push(portfolio);
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &token,
            &env.admin.pubkey(),
            &[],
            2_000_000,
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("public collateral minting");
    env.send(
        env.deposit_ix(portfolio, 2_000_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("public funded deposit");
    assert_eq!(env.token_amount(token), 0);
    portfolio
}

pub(super) fn withdraw_to_public_ata(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    amount: u128,
) -> (Pubkey, u64) {
    let token = canonical_vault_ata(owner.pubkey(), env.mint);
    assert_eq!(env.token_amount(token), 0);
    let cu = env
        .send(
            env.withdraw_ix(portfolio, amount),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("public withdrawal into the original SPL ATA");
    (token, cu)
}

pub(super) struct Reports {
    pub(super) feeds: Vec<[[u8; 32]; 3]>,
    pub(super) stages: Vec<Vec<[Pubkey; 3]>>,
    assets: Vec<u16>,
    table: Option<AddressLookupTableAccount>,
}

impl Reports {
    pub(super) fn new(
        env: &mut V16CuEnv,
        assets: &[u16],
        start_slot: u64,
        backlog: u64,
        distinct: bool,
    ) -> Self {
        let feeds: Vec<_> = assets
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if distinct {
                    std::array::from_fn(|j| [1 + (3 * i + j) as u8; 32])
                } else {
                    [[0xe1; 32], [0xe2; 32], [0xe3; 32]]
                }
            })
            .collect();
        // Reports are external oracle inputs. Pre-create every dated report so a
        // public lookup table can mature before the first portfolio trade.
        let stages: Vec<Vec<[Pubkey; 3]>> = [
            (start_slot, 3_000_000),
            (start_slot + 1, 3_030_000),
            (start_slot + 2, 3_000_000),
            (start_slot + 2 + backlog, 2_850_000),
        ]
        .into_iter()
        .map(|(slot, price)| {
            let mut reports = Vec::new();
            for feed in feeds.iter().take(if distinct { assets.len() } else { 1 }) {
                reports.push(std::array::from_fn(|i| {
                    env.set_pyth_price(
                        &feed[i],
                        [price, 150_000_000, 200_000_000][i],
                        -6,
                        100 + slot as i64,
                    )
                }));
            }
            if !distinct {
                reports.resize(assets.len(), reports[0]);
            }
            reports
        })
        .collect();
        let table = distinct.then(|| {
            assert_eq!(assets.len(), 14);
            assert_eq!(feeds.iter().flatten().collect::<std::collections::BTreeSet<_>>().len(), 42);
            let addresses: Vec<_> = stages.iter().flatten().flatten().copied().collect();
            assert_eq!(addresses.len(), 168);
            assert_eq!(addresses.iter().collect::<std::collections::BTreeSet<_>>().len(), 168);
            let slot = env.svm.get_sysvar::<Clock>().slot;
            assert!(slot < start_slot, "lookup addresses must mature before use");
            let (create, key) = lookup_ix::create_lookup_table(env.payer.pubkey(), env.payer.pubkey(), slot);
            let submit = |env: &mut V16CuEnv, instruction| {
                env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[instruction], Some(&env.payer.pubkey()), &[&env.payer], env.svm.latest_blockhash(),
                );
                assert!(bincode::serialized_size(&tx).unwrap() <= 1232);
                env.svm.send_transaction(tx).expect("public lookup-table construction");
            };
            submit(env, create);
            for chunk in addresses.chunks(20) {
                let extend = lookup_ix::extend_lookup_table(key, env.payer.pubkey(), Some(env.payer.pubkey()), chunk.to_vec());
                submit(env, extend);
            }
            let account = env.svm.get_account(&key).unwrap();
            assert_eq!(account.owner, solana_sdk::address_lookup_table::program::id());
            let table = AddressLookupTable::deserialize(&account.data).unwrap();
            assert_eq!(table.addresses.as_ref(), addresses);
            assert_eq!(table.meta.last_extended_slot, slot);
            assert!(env.svm.get_sysvar::<solana_sdk::rent::Rent>().is_exempt(account.lamports, account.data.len()));
            println!("Distinct-feed lookup: 42 feeds, 168 dated reports, public create + 9 extensions, slot={slot}");
            AddressLookupTableAccount { key, addresses }
        });
        Self {
            feeds,
            stages,
            assets: assets.to_vec(),
            table,
        }
    }

    pub(super) fn account_keys(&self) -> Vec<Pubkey> {
        let mut keys: Vec<_> = self.stages.iter().flatten().flatten().copied().collect();
        keys.extend(self.table.iter().map(|table| table.key));
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    pub(super) fn crank(
        &self,
        env: &V16CuEnv,
        portfolio: Pubkey,
        now_slot: u64,
        assets: &[u16],
        stage: usize,
    ) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ];
        for asset in assets {
            let index = self.assets.iter().position(|a| a == asset).unwrap();
            accounts.extend(
                self.stages[stage][index]
                    .iter()
                    .map(|key| AccountMeta::new_readonly(*key, false)),
            );
        }
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: assets
                    .iter()
                    .map(|asset_index| CrankObservationHint {
                        asset_index: *asset_index,
                        oracle_accounts: 3,
                    })
                    .collect(),
            }
            .encode(),
        }
    }

    pub(super) fn transaction(
        &self,
        env: &mut V16CuEnv,
        instruction: Instruction,
    ) -> VersionedTransaction {
        env.svm.expire_blockhash();
        let full = instruction.accounts.len() == 45;
        let instructions = [heap_ix(), cu_ix(), instruction];
        let legacy = Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        );
        let tx = if let Some(table) = &self.table {
            let message = v0::Message::try_compile(
                &env.payer.pubkey(),
                &instructions,
                std::slice::from_ref(table),
                env.svm.latest_blockhash(),
            )
            .unwrap();
            if full {
                assert!(
                    bincode::serialized_size(&legacy).unwrap() > 1232,
                    "distinct-feed maximum requires address compression"
                );
                assert_eq!(message.address_table_lookups.len(), 1);
                assert_eq!(message.address_table_lookups[0].readonly_indexes.len(), 42);
                assert!(message.address_table_lookups[0].writable_indexes.is_empty());
            }
            VersionedTransaction::try_new(VersionedMessage::V0(message), &[&env.payer]).unwrap()
        } else {
            VersionedTransaction::from(legacy)
        };
        let bytes = bincode::serialized_size(&tx).unwrap();
        assert!(bytes <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
        assert_eq!(
            tx.signatures.len(),
            1,
            "cranks require only the payer signature"
        );
        if full && self.table.is_some() {
            println!("Distinct-feed crank: hints=14, references=42, loaded_readonly=42, wire_bytes={bytes}");
        }
        tx
    }

    pub(super) fn assert_wrong_tail_rollback(
        &self,
        env: &mut V16CuEnv,
        portfolio: Pubkey,
        peer: Pubkey,
        slot: u64,
    ) {
        // Put the sole unfinished asset first so its remaining 32 accrual steps
        // execute before a valid but wrong-domain report at the end of the tail.
        let mut assets = self.assets.clone();
        assets.rotate_right(1);
        assert_eq!(
            slot - env.market_state().1.assets[assets[0] as usize].slot_last,
            32
        );
        let mut ix = self.crank(env, portfolio, slot, &assets, 3);
        // Another asset's dated report keeps 42 distinct, valid oracle accounts.
        // Feed identity is checked before its timestamp can affect freshness.
        ix.accounts.last_mut().unwrap().pubkey = self.stages[2][13][2];
        let mut keys = self.account_keys();
        keys.extend([env.market, portfolio, peer, env.vault, env.mint]);
        let frames: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        let tx = self.transaction(env, ix);
        payer.lamports -= FeeStructure::default().lamports_per_signature;
        let failure = env
            .svm
            .send_transaction(tx)
            .expect_err("late wrong feed must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::InvalidOracleKey as u32)
            ),
            "{failure:?}"
        );
        assert_cu_within(
            "distinct-feed late binding rollback",
            failure.meta.compute_units_consumed,
            1_375_000,
        );
        assert_eq!(keys.iter().map(|key| env.svm.get_account(key)).collect::<Vec<_>>(), frames, "full Account rollback includes pending accrual, both portfolios, custody, every report and lookup table");
        assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
        println!(
            "Distinct-feed wrong-tail rollback: error={:?}, cu={}, framed_accounts={}",
            failure.err,
            failure.meta.compute_units_consumed,
            keys.len()
        );
    }
}
