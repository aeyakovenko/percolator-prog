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

use super::support::v16_svm::{MarketConfig, V16Svm};
use percolator_prog::ix::Instruction as ProgInstruction;
use solana_sdk::{hash::Hash, pubkey::Pubkey};

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
