//! INV-006/003/017: retained v0 consent with real, publicly populated lookup tables.
//! Signatures bind table addresses, indices and privileges; runtime lookup resolution
//! supplies the keys on which the wrapper must still enforce roles and incarnations.

use super::*;
use percolator_prog::error::PercolatorError;
use solana_sdk::{
    account::Account,
    address_lookup_table::{instruction as lookup_ix, state::AddressLookupTable},
    address_lookup_table_account::AddressLookupTableAccount,
    instruction::InstructionError,
    signature::Keypair,
    slot_hashes::SlotHashes,
    transaction::Transaction,
};

const AMOUNT: u64 = 1_337;
const PREFIX_AMOUNT: u64 = 97;

fn deposit(env: &V16Svm, actor: usize, amount: u64) -> Instruction {
    let owner = &env.actors[actor];
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(owner.portfolio, false),
            AccountMeta::new(owner.source_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::Deposit {
            portfolio_id: env.primary_portfolio_id(actor),
            expected_sequence: env.primary_portfolio_matcher_sequence(actor),
            amount: amount.into(),
        }
        .encode(),
    }
}

fn table_control(env: &mut V16Svm, payer: &Keypair, authority: usize, ix: Instruction) {
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, &env.actors[authority].signer],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("public lookup control");
}

fn create_table(env: &mut V16Svm, payer: &Keypair, authority: usize) -> AddressLookupTableAccount {
    // LiteSVM's warp updates Clock only; use its authenticated initial SlotHashes
    // entry for the public lookup-program PDA derivation.
    let recent_slot = env.svm.get_sysvar::<SlotHashes>()[0].0;
    let owner = env.actors[authority].signer.pubkey();
    let (create, key) = lookup_ix::create_lookup_table_signed(owner, payer.pubkey(), recent_slot);
    table_control(env, payer, authority, create);
    let addresses = vec![
        env.market,
        env.actors[0].portfolio,
        env.actors[0].source_token,
        env.vault,
        spl_token::ID,
        env.actors[1].portfolio,
        env.actors[1].source_token,
        env.vault_authority,
    ];
    table_control(
        env,
        payer,
        authority,
        lookup_ix::extend_lookup_table(key, owner, Some(payer.pubkey()), addresses.clone()),
    );
    let account = env.svm.get_account(&key).unwrap();
    assert_eq!(account.owner, solana_sdk::address_lookup_table::program::ID);
    assert_eq!(
        AddressLookupTable::deserialize(&account.data)
            .unwrap()
            .addresses
            .as_ref(),
        addresses
    );
    AddressLookupTableAccount { key, addresses }
}

fn sign(
    env: &V16Svm,
    payer: &Keypair,
    table: &AddressLookupTableAccount,
    instructions: &[Instruction],
    actors: &[usize],
) -> VersionedTransaction {
    let mut ixs = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
    ];
    ixs.extend_from_slice(instructions);
    let message = v0::Message::try_compile(
        &payer.pubkey(),
        &ixs,
        std::slice::from_ref(table),
        env.svm.latest_blockhash(),
    )
    .unwrap();
    assert_eq!(message.address_table_lookups.len(), 1);
    assert_eq!(message.address_table_lookups[0].account_key, table.key);
    assert!(!message.account_keys.contains(&env.market));
    assert!(!message.account_keys.contains(&env.actors[0].portfolio));
    assert!(!message.account_keys.contains(&env.vault));
    let mut signers = vec![payer];
    signers.extend(actors.iter().map(|actor| &env.actors[*actor].signer));
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers).unwrap();
    tx.verify_and_hash_message().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn accounts(env: &V16Svm, tables: &[Pubkey]) -> Vec<(Pubkey, Option<Account>)> {
    let mut keys = vec![
        env.market,
        env.foreign_market,
        env.mint,
        env.vault,
        env.foreign_vault,
    ];
    keys.extend(env.all_token_account_data().into_iter().map(|(key, _)| key));
    keys.extend(tables);
    for actor in &env.actors {
        keys.extend([
            actor.signer.pubkey(),
            actor.portfolio,
            actor.matcher_context,
            actor.matcher_delegate,
        ]);
    }
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn rejected_bundle(
    env: &mut V16Svm,
    payer: &Keypair,
    table: Pubkey,
    tx: VersionedTransaction,
    error: PercolatorError,
) -> u64 {
    tx.verify_and_hash_message().unwrap();
    let before = accounts(env, &[table]);
    let mut fee_payer = env.svm.get_account(&payer.pubkey()).unwrap();
    fee_payer.lamports -= solana_sdk::fee::FeeStructure::default().lamports_per_signature
        * tx.signatures.len() as u64;
    let failed = env
        .svm
        .send_transaction(tx)
        .expect_err("loaded suffix must reject");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(3, InstructionError::Custom(error as u32))
    );
    assert_eq!(
        accounts(env, &[table]),
        before,
        "exact rollback includes loaded accounts and table"
    );
    assert_eq!(env.svm.get_account(&payer.pubkey()), Some(fee_payer));
    assert!(failed
        .meta
        .logs
        .contains(&format!("Program {} success", env.program_id)));
    assert!(failed
        .meta
        .logs
        .contains(&format!("Program {} success", spl_token::ID)));
    assert!(
        failed.meta.compute_units_consumed > 0 && failed.meta.compute_units_consumed <= TX_CU_LIMIT
    );
    failed.meta.compute_units_consumed
}

#[test]
fn retained_v0_lookup_entries_bind_signed_domains_roles_and_retry() {
    let mut env = V16Svm::new([0x66; 32], MarketConfig::default());
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let table = create_table(&mut env, &payer, 4);
    let alternate = create_table(&mut env, &payer, 3);
    env.warp_to_slot(2);
    let request = deposit(&env, 0, AMOUNT);
    let retained = sign(&env, &payer, &table, std::slice::from_ref(&request), &[0]);
    let before_simulation = accounts(&env, &[table.key, alternate.key, payer.pubkey()]);
    let simulation = env
        .svm
        .simulate_transaction(retained.clone())
        .expect("retained loaded deposit is initially live");
    assert!(
        simulation.compute_units_consumed > 0 && simulation.compute_units_consumed <= TX_CU_LIMIT
    );
    assert_eq!(
        accounts(&env, &[table.key, alternate.key, payer.pubkey()]),
        before_simulation
    );
    let VersionedMessage::V0(message) = &retained.message else {
        unreachable!()
    };
    let mut writable = message.address_table_lookups[0].writable_indexes.clone();
    writable.sort_unstable();
    assert_eq!(writable, [0, 1, 2, 3]);
    assert_eq!(message.address_table_lookups[0].readonly_indexes, [4]);

    let authority = env.actors[4].signer.pubkey();
    let appended = vec![env.foreign_market, env.foreign_actor.portfolio];
    table_control(
        &mut env,
        &payer,
        4,
        lookup_ix::extend_lookup_table(
            table.key,
            authority,
            Some(payer.pubkey()),
            appended.clone(),
        ),
    );
    table_control(
        &mut env,
        &payer,
        4,
        lookup_ix::freeze_lookup_table(table.key, authority),
    );
    let table_account = env.svm.get_account(&table.key).unwrap();
    let decoded = AddressLookupTable::deserialize(&table_account.data).unwrap();
    assert_eq!(decoded.meta.authority, None);
    assert_eq!(&decoded.addresses[..table.addresses.len()], table.addresses);
    assert_eq!(&decoded.addresses[table.addresses.len()..], appended);

    for mutation in 0..4 {
        let mut tampered = retained.clone();
        let VersionedMessage::V0(message) = &mut tampered.message else {
            unreachable!()
        };
        let lookup = &mut message.address_table_lookups[0];
        match mutation {
            0 => lookup.account_key = alternate.key,
            1 => {
                *lookup
                    .writable_indexes
                    .iter_mut()
                    .find(|index| **index == 1)
                    .unwrap() = 5
            }
            2 => lookup.readonly_indexes[0] = 7,
            3 => lookup.writable_indexes.swap(0, 1),
            _ => unreachable!(),
        }
        tampered.sanitize().unwrap();
        let before = accounts(&env, &[table.key, alternate.key]);
        let fee_payer = env.svm.get_account(&payer.pubkey());
        let failed = env
            .svm
            .send_transaction(tampered)
            .expect_err("signed lookup bytes cannot change");
        assert_eq!(
            failed.err,
            TransactionError::SignatureFailure,
            "lookup mutation {mutation}"
        );
        assert_eq!(accounts(&env, &[table.key, alternate.key]), before);
        assert_eq!(env.svm.get_account(&payer.pubkey()), fee_payer);
    }

    let prefix = deposit(&env, 1, PREFIX_AMOUNT);
    let mut peak = 0;
    for alias in [true, false] {
        let mut suffix = request.clone();
        let error = if alias {
            suffix.accounts[2].pubkey = env.market;
            PercolatorError::InvalidAccountKind
        } else {
            suffix.accounts[2].is_writable = false;
            PercolatorError::ExpectedWritable
        };
        let tx = sign(&env, &payer, &table, &[prefix.clone(), suffix], &[1, 0]);
        peak = peak.max(rejected_bundle(&mut env, &payer, table.key, tx, error));
    }

    let before = env.primary_portfolio(0).capital.get();
    let source = env.token_amount(env.actors[0].source_token);
    let destination = env.token_amount(env.actors[0].destination_token);
    let vault = env.token_amount(env.vault);
    let group = env.primary_market_state().1;
    let sequence = env.primary_portfolio_matcher_sequence(0);
    let meta = env
        .svm
        .send_transaction(retained)
        .expect("unchanged retained signature survives append, freeze and rolled-back prefixes");
    peak = peak.max(meta.compute_units_consumed);
    assert!(peak <= TX_CU_LIMIT);
    assert_eq!(
        env.primary_portfolio(0).capital.get(),
        before + u128::from(AMOUNT)
    );
    assert_eq!(env.primary_portfolio_matcher_sequence(0), sequence + 1);
    assert_eq!(
        env.token_amount(env.actors[0].source_token),
        source - AMOUNT
    );
    assert_eq!(env.token_amount(env.vault), vault + AMOUNT);
    assert_eq!(
        env.primary_market_state().1.c_tot,
        group.c_tot + u128::from(AMOUNT)
    );
    assert_eq!(
        env.primary_market_state().1.vault,
        group.vault + u128::from(AMOUNT)
    );
    assert_eq!(env.primary_market_state().1.insurance, group.insurance);
    let replay = sign(&env, &payer, &alternate, &[prefix, request], &[1, 0]);
    peak = peak.max(rejected_bundle(
        &mut env,
        &payer,
        alternate.key,
        replay,
        PercolatorError::EngineStale,
    ));
    env.withdraw_primary(0, AMOUNT.into())
        .expect("public payout remains live");
    assert_eq!(env.primary_portfolio(0).capital.get(), before);
    assert_eq!(
        env.token_amount(env.actors[0].destination_token),
        destination + AMOUNT
    );
    assert_eq!(env.token_amount(env.vault), vault);
    assert_eq!(env.primary_market_state().1.c_tot, group.c_tot);
    assert_eq!(env.primary_market_state().1.vault, group.vault);
    assert_eq!(env.svm.get_account(&table.key), Some(table_account));
    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    println!("INV-006 loaded domains: 4 signature rejections, 3 prefix rollbacks (alias, readonly, alternate-table replay), 1 unchanged retained deposit, 1 payout; peak CU={peak}");
}

#[test]
fn retained_v0_lookup_portfolio_aba_rolls_back_prefix_and_allows_fresh_consent() {
    let mut peak = 0;
    for owner_roundtrip in [false, true] {
        let mut env = V16Svm::new([0x76; 32], MarketConfig::default());
        let payer = Keypair::new();
        env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
        let table = create_table(&mut env, &payer, 4);
        env.warp_to_slot(2);
        let prefix = deposit(&env, 1, PREFIX_AMOUNT);
        let suffix = deposit(&env, 0, AMOUNT);
        let retained = sign(&env, &payer, &table, &[prefix.clone(), suffix], &[1, 0]);
        let before_simulation = accounts(&env, &[table.key, payer.pubkey()]);
        let simulation = env
            .svm
            .simulate_transaction(retained.clone())
            .expect("both loaded deposits initially succeed");
        assert!(
            simulation.compute_units_consumed > 0
                && simulation.compute_units_consumed <= TX_CU_LIMIT
        );
        assert_eq!(
            accounts(&env, &[table.key, payer.pubkey()]),
            before_simulation
        );
        let old_id = env.primary_portfolio_id(0);
        let table_before = env.svm.get_account(&table.key);
        let capital = env.primary_portfolio(0).capital.get();
        env.withdraw_primary(0, capital).unwrap();
        env.close_primary_portfolio(0).unwrap();
        if owner_roundtrip {
            let (intermediate, current) = env
                .cycle_closed_primary_portfolio_through_owner(0, 2)
                .unwrap();
            assert!(old_id < intermediate && intermediate < current);
        } else {
            env.fund_closed_primary_portfolio(0, 1_000_000_000).unwrap();
            env.reinitialize_primary_portfolio(0).unwrap();
        }
        assert!(env.primary_portfolio_id(0) > old_id);
        assert_eq!(env.svm.get_account(&table.key), table_before);
        // Renew the sequence and signature, retaining only the obsolete ID. The
        // incarnation check must reject independently of sequence/cache admission.
        let mut stale_id_only = deposit(&env, 0, AMOUNT);
        stale_id_only.data = ProgInstruction::Deposit {
            portfolio_id: old_id,
            expected_sequence: env.primary_portfolio_matcher_sequence(0),
            amount: AMOUNT.into(),
        }
        .encode();
        let tx = sign(
            &env,
            &payer,
            &table,
            &[prefix.clone(), stale_id_only],
            &[1, 0],
        );
        peak = peak.max(rejected_bundle(
            &mut env,
            &payer,
            table.key,
            tx,
            PercolatorError::EngineProvenanceMismatch,
        ));
        peak = peak.max(rejected_bundle(
            &mut env,
            &payer,
            table.key,
            retained,
            PercolatorError::EngineProvenanceMismatch,
        ));

        let actors = [0, 1];
        let capital = actors.map(|actor| env.primary_portfolio(actor).capital.get());
        let sources = actors.map(|actor| env.token_amount(env.actors[actor].source_token));
        let destinations =
            actors.map(|actor| env.token_amount(env.actors[actor].destination_token));
        let sequences = actors.map(|actor| env.primary_portfolio_matcher_sequence(actor));
        let vault = env.token_amount(env.vault);
        let group = env.primary_market_state().1;
        let fresh = sign(
            &env,
            &payer,
            &table,
            &[prefix, deposit(&env, 0, AMOUNT)],
            &[1, 0],
        );
        let meta = env
            .svm
            .send_transaction(fresh)
            .expect("fresh incarnation repairs the same loaded bundle");
        peak = peak.max(meta.compute_units_consumed);
        assert_eq!(env.token_amount(env.vault), vault + AMOUNT + PREFIX_AMOUNT);
        assert_eq!(
            env.primary_market_state().1.c_tot,
            group.c_tot + u128::from(AMOUNT + PREFIX_AMOUNT)
        );
        assert_eq!(
            env.primary_market_state().1.vault,
            group.vault + u128::from(AMOUNT + PREFIX_AMOUNT)
        );
        assert_eq!(env.primary_market_state().1.insurance, group.insurance);
        for (actor, amount) in [AMOUNT, PREFIX_AMOUNT].into_iter().enumerate() {
            assert_eq!(
                env.primary_portfolio(actor).capital.get(),
                capital[actor] + u128::from(amount)
            );
            assert_eq!(
                env.primary_portfolio_matcher_sequence(actor),
                sequences[actor] + 1
            );
            assert_eq!(
                env.token_amount(env.actors[actor].source_token),
                sources[actor] - amount
            );
            env.withdraw_primary(actor, amount.into()).unwrap();
            assert_eq!(env.primary_portfolio(actor).capital.get(), capital[actor]);
            assert_eq!(
                env.token_amount(env.actors[actor].destination_token),
                destinations[actor] + amount
            );
        }
        assert_eq!(env.token_amount(env.vault), vault);
        assert_eq!(env.primary_market_state().1.c_tot, group.c_tot);
        assert_eq!(env.primary_market_state().1.vault, group.vault);
        assert_eq!(env.svm.get_account(&table.key), table_before);
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    }
    assert!(peak <= TX_CU_LIMIT);
    println!("INV-003 loaded ABA: 2 worlds, 4 prefix rollbacks, 2 fresh bundles, 4 payouts; peak CU={peak}");
}
