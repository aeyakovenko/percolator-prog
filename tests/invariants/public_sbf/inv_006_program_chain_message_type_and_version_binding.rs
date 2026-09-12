//! INV-006 - Program, chain, message-type, and version binding.
//!
//! A retained request in the deployed wrapper is a signed Solana transaction, so the signature
//! covers the invoked program, every account key (including the market), the exact instruction
//! bytes, and the recent blockhash. This test mutates each signed domain after signing and requires
//! the transaction boundary to reject before any persistent effect. The instruction decoder's
//! exhaustive schema/version obligations remain owned by INV-022.
//!
//! Guarantee boundary: Solana has no explicit genesis hash in a legacy transaction message. This
//! evidence establishes practical cluster binding through the signed recent blockhash and its
//! bounded validity window; it does not claim an application-level genesis-domain field exists.

use super::support::v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT};
use percolator_prog::ix::Instruction as ProgInstruction;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0, Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signer,
    transaction::{TransactionError, VersionedTransaction},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistentSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    token_accounts: Vec<(Pubkey, Vec<u8>)>,
    token_supply: u128,
}

fn snapshot(env: &V16Svm) -> PersistentSnapshot {
    PersistentSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        token_accounts: env.all_token_account_data(),
        token_supply: env.token_supply_observed(),
    }
}

fn program_instruction_index(env: &V16Svm, tx: &solana_sdk::transaction::Transaction) -> usize {
    tx.message
        .instructions
        .iter()
        .position(|ix| tx.message.account_keys[usize::from(ix.program_id_index)] == env.program_id)
        .expect("retained transaction invokes Percolator")
}

fn assert_tamper_rejected_without_effect(
    label: &str,
    mutate: impl FnOnce(&V16Svm, &mut solana_sdk::transaction::Transaction),
) {
    let mut env = V16Svm::new([0x06; 32], MarketConfig::default());
    let mut retained = env.build_retained_deposit(0, 1_337);
    let before = snapshot(&env);
    mutate(&env, &mut retained);

    let error = env
        .land_retained(retained)
        .expect_err("post-signature mutation must reject");
    assert_eq!(snapshot(&env), before, "{label}: exact persistent rollback");
    assert!(
        error.contains("SignatureFailure")
            || error.contains("TransactionSignatureVerificationFailure"),
        "{label}: expected signature-bound rejection, got {error}"
    );
}

#[test]
fn retained_transaction_binds_program_market_kind_schema_and_blockhash() {
    assert_tamper_rejected_without_effect("program id", |env, tx| {
        let ix_index = program_instruction_index(env, tx);
        let alternate_program_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| *key == spl_token::ID)
            .expect("deposit carries the SPL Token program");
        tx.message.instructions[ix_index].program_id_index =
            u8::try_from(alternate_program_index).expect("compiled account index fits u8");
    });

    assert_tamper_rejected_without_effect("market pubkey", |env, tx| {
        let market_index = tx
            .message
            .account_keys
            .iter()
            .position(|key| *key == env.market)
            .expect("deposit carries the primary market");
        tx.message.account_keys[market_index] = env.foreign_market;
    });

    assert_tamper_rejected_without_effect("instruction kind", |env, tx| {
        let ix_index = program_instruction_index(env, tx);
        tx.message.instructions[ix_index].data = ProgInstruction::Withdraw {
            portfolio_id: env.primary_portfolio_id(0),
            expected_sequence: 0,
            amount: 1_337,
        }
        .encode();
    });

    assert_tamper_rejected_without_effect("instruction schema bytes", |env, tx| {
        let ix_index = program_instruction_index(env, tx);
        tx.message.instructions[ix_index].data.push(0);
    });

    assert_tamper_rejected_without_effect("recent blockhash", |_env, tx| {
        tx.message.recent_blockhash = Hash::new_unique();
    });
}

#[test]
fn retained_deposit_signatures_cannot_cross_legacy_and_v0_message_versions() {
    for (label, sign_v0) in [("legacy to v0", false), ("v0 to legacy", true)] {
        let mut env = V16Svm::new([0x26; 32], MarketConfig::default());
        let owner = env.actors[0].signer.pubkey();
        let source = env.actors[0].source_token;
        let sequence = env.primary_portfolio_matcher_sequence(0);
        let amount = 1_337u64;
        let deposit = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.actors[0].portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::Deposit {
                portfolio_id: env.primary_portfolio_id(0),
                expected_sequence: sequence,
                amount: u128::from(amount),
            }
            .encode(),
        };
        let legacy = Message::new_with_blockhash(
            &[
                ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
                deposit,
            ],
            Some(&owner),
            &env.svm.latest_blockhash(),
        );
        let before_accounts: Vec<_> = legacy
            .account_keys
            .iter()
            .map(|key| (*key, env.svm.get_account(key)))
            .collect();
        // Empty lookups keep all keys, flags, instructions, and the blockhash identical.
        let versioned = VersionedMessage::V0(v0::Message {
            header: legacy.header,
            account_keys: legacy.account_keys.clone(),
            recent_blockhash: legacy.recent_blockhash,
            instructions: legacy.instructions.clone(),
            address_table_lookups: vec![],
        });
        let legacy = VersionedMessage::Legacy(legacy);
        let (original, alternate) = if sign_v0 {
            (versioned, legacy)
        } else {
            (legacy, versioned)
        };
        let control = VersionedTransaction::try_new(original, &[&env.actors[0].signer])
            .expect("sign the original message version");
        let mut tampered = control.clone();
        tampered.message = alternate;
        tampered
            .sanitize()
            .expect("the alternate envelope is structurally valid");
        let before = snapshot(&env);
        let before_capital = env.primary_portfolio(0).capital.get();
        let before_source = env.token_amount(source);
        let before_vault = env.token_amount(env.vault);

        let error = env
            .svm
            .send_transaction(tampered)
            .expect_err("a retained signature cannot authorize the other message version");
        assert_eq!(error.err, TransactionError::SignatureFailure, "{label}");
        assert_eq!(snapshot(&env), before, "{label}: persistent state is exact");
        for (key, account) in before_accounts {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "{label}: full account {key}, including payer lamports, is unchanged"
            );
        }

        let meta = env
            .svm
            .send_transaction(control)
            .expect("the correctly signed version must still execute the public deposit");
        assert!(meta.compute_units_consumed <= TX_CU_LIMIT, "{label}");
        assert_eq!(
            env.primary_portfolio(0).capital.get(),
            before_capital + u128::from(amount)
        );
        assert_eq!(env.token_amount(source), before_source - amount);
        assert_eq!(env.token_amount(env.vault), before_vault + amount);
        assert_eq!(env.primary_portfolio_matcher_sequence(0), sequence + 1);
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    }
}

#[test]
fn unmodified_retained_transaction_still_executes() {
    let mut env = V16Svm::new([0x16; 32], MarketConfig::default());
    let before_capital = env.primary_portfolio(0).capital.get();
    let before_source = env.token_amount(env.actors[0].source_token);
    let before_vault = env.token_amount(env.vault);
    let retained = env.build_retained_deposit(0, 1_337);

    env.land_retained(retained)
        .expect("unmodified retained deposit must land");

    assert_eq!(
        env.primary_portfolio(0).capital.get(),
        before_capital + 1_337
    );
    assert_eq!(
        env.token_amount(env.actors[0].source_token),
        before_source - 1_337
    );
    assert_eq!(env.token_amount(env.vault), before_vault + 1_337);
    assert_eq!(
        env.token_supply_observed(),
        env.initial_token_supply,
        "the signed control conserves external quote supply"
    );
}

#[test]
fn deployed_wrapper_has_no_detached_signature_interpreter() {
    let source = include_str!("../../../src/v16_program.rs");
    for forbidden in [
        "ed25519_program",
        "secp256k1_program",
        "secp256r1_program",
        "sysvar::instructions",
        "load_instruction_at_checked",
        "load_instruction_at_relative",
    ] {
        assert!(
            !source.contains(forbidden),
            "adding detached-signature surface {forbidden:?} requires an explicit typed domain header"
        );
    }

    let signer_guard = source
        .split("fn expect_signer")
        .nth(1)
        .and_then(|tail| tail.split("fn expect_writable").next())
        .expect("production signer guard remains source-visible");
    assert!(
        signer_guard.contains("if !ai.is_signer"),
        "wrapper authorization must continue to consume the SVM-authenticated signer bit"
    );
    assert_eq!(
        source
            .matches("Instruction::decode(instruction_data)?")
            .count(),
        1,
        "all deployed instruction bytes must continue to enter one strict decoder"
    );
}
