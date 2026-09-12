//! INV-080 - Error propagation and exact rollback.
//!
//! Normative obligation: Engine or wrapper rejection reaches the instruction boundary as Err,
//! and SVM rollback preserves persistent bytes, tokens, and economic lamports exactly.
//!
//! Bounded public evidence: both orders of a successful ClosePortfolio/Deposit prefix followed
//! by an over-withdraw from an independent funded owner. The vault can fund the withdrawal;
//! the engine rejects the owner's insufficient capital. Exact rollback restores the closed
//! account, its rent transferred to the slab, and the deposit's SPL and accounting effects.
//! Unchanged standalone prefix instructions and a capital-exact withdrawal then succeed.
//!
//! Only public initialization and instructions construct economic state. The SVM network-fee
//! payer is excluded only from lamport equality. Other routes, error classes, lifecycle states,
//! and supported-shape products remain open; this is not an engine proof or status promotion.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use litesvm::types::TransactionMetadata;
use percolator_prog::error::PercolatorError;
use solana_sdk::{
    instruction::InstructionError,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};
use std::collections::BTreeSet;

fn assert_public_error_and_exact_rollback(
    env: &mut V16Svm,
    transaction: Transaction,
    instruction_index: u8,
    expected_error: InstructionError,
) -> TransactionMetadata {
    let signature = transaction.signatures[0];
    let fee_payer = transaction.message.account_keys[0];
    let payer_before = env.svm.get_account(&fee_payer).expect("fee payer");
    let rejected_instruction = &transaction.message.instructions[usize::from(instruction_index)];
    assert_eq!(
        transaction.message.account_keys[usize::from(rejected_instruction.program_id_index)],
        env.program_id,
        "the rejected instruction must be a public wrapper route"
    );
    assert!(
        transaction
            .message
            .instructions
            .iter()
            .all(|instruction| !instruction.accounts.contains(&0)),
        "the network-fee payer must have no instruction account role"
    );

    // The shared trace frames the last instruction, so also cover the entire bundle and all
    // tracked economic accounts, including unrelated portfolios, matcher contexts, and custody.
    let mut keys: BTreeSet<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .chain(transaction.message.account_keys.iter().copied())
        .chain(env.actors.iter().map(|actor| actor.signer.pubkey()))
        .chain([env.foreign_actor.signer.pubkey()])
        .collect();
    keys.remove(&fee_payer);
    let before: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect();

    env.land_retained(transaction)
        .expect_err("public rejection must abort the transaction");
    let rejected = env
        .svm
        .get_transaction(&signature)
        .expect("the SVM recorded the attempted transaction")
        .as_ref()
        .expect_err("the recorded public transaction must fail");
    assert_eq!(
        rejected.err,
        TransactionError::InstructionError(instruction_index, expected_error),
        "the exact error must reach the requested public instruction boundary"
    );
    let metadata = rejected.meta.clone();
    for (key, account) in before {
        assert_eq!(
            env.svm.get_account(&key),
            account,
            "rejection changed account {key}: bytes, tokens, lamports, or metadata"
        );
    }
    let mut payer_after = env.svm.get_account(&fee_payer).expect("fee payer remains");
    assert!(payer_after.lamports <= payer_before.lamports);
    payer_after.lamports = payer_before.lamports;
    assert_eq!(payer_after, payer_before, "only network fees may differ");
    metadata
}

#[test]
fn v16_program_engine_error_rolls_back_successful_close_and_deposit_prefix() {
    const CLOSING: usize = 0;
    const DEPOSITING: usize = 1;
    const WITHDRAWING: usize = 2;
    const DEPOSIT: u128 = 7;
    const CAPITAL: u128 = 10;

    for close_first in [false, true] {
        let config = MarketConfig {
            actor_deposits: [0, 20, CAPITAL, 30, 40],
            ..MarketConfig::default()
        };
        let mut env = V16Svm::new([0x80 ^ u8::from(close_first); 32], config);
        let closing_portfolio = env.actors[CLOSING].portfolio;
        let closing_before = env.svm.get_account(&closing_portfolio).unwrap();
        let market_lamports_before = env.account_lamports(env.market);
        let source = env.actors[DEPOSITING].source_token;
        let destination = env.actors[WITHDRAWING].destination_token;
        let source_before = env.token_amount(source);
        let destination_before = env.token_amount(destination);
        let vault_before = env.token_amount(env.vault);
        let supply_before = env.mint_supply();
        let group_before = env.primary_market_state().1;
        assert!(closing_before.lamports > 0 && !closing_before.data.is_empty());
        assert_eq!(env.primary_portfolio(CLOSING).capital.get(), 0);
        assert_eq!(env.primary_portfolio(WITHDRAWING).capital.get(), CAPITAL);
        assert!(u128::from(vault_before) > CAPITAL + 1);
        assert!(u128::from(source_before) >= DEPOSIT);

        let close = env.build_retained_close_primary_portfolio(CLOSING);
        let deposit = env.build_retained_deposit(DEPOSITING, DEPOSIT);
        let prefix = if close_first {
            [close, deposit]
        } else {
            [deposit, close]
        };
        let over_withdraw = env.build_retained_withdrawal(WITHDRAWING, CAPITAL + 1);
        let valid_withdraw = env.build_retained_withdrawal(WITHDRAWING, CAPITAL);
        let rejected_bundle = env.bundle_retained_transactions(&[
            prefix[0].clone(),
            prefix[1].clone(),
            over_withdraw,
        ]);
        let rejection_index = u8::try_from(rejected_bundle.message.instructions.len() - 1).unwrap();
        let expected_code = PercolatorError::EngineLockActive as u32;
        assert_ne!(expected_code, 0);
        env.begin_public_trace();

        let rejection = assert_public_error_and_exact_rollback(
            &mut env,
            rejected_bundle,
            rejection_index,
            InstructionError::Custom(expected_code),
        );
        let wrapper_success = format!("Program {} success", env.program_id);
        assert_eq!(
            rejection
                .logs
                .iter()
                .filter(|line| **line == wrapper_success)
                .count(),
            2,
            "both prefix instructions must execute successfully before the engine rejection"
        );
        assert!(
            rejection
                .logs
                .contains(&format!("Program {} success", spl_token::ID)),
            "the prefix must include a successful SPL transfer"
        );

        // Replay the unchanged signed prefix one instruction at a time so every successful
        // public trace step has its own account roles and independently checked effects.
        for (index, transaction) in prefix.into_iter().enumerate() {
            env.land_retained(transaction)
                .expect("rollback must leave each unchanged prefix instruction executable");
            let closed = close_first || index == 1;
            let deposited = !close_first || index == 1;
            let deposited_amount = if deposited { DEPOSIT } else { 0 };
            if closed {
                assert_eq!(env.account_lamports(closing_portfolio), 0);
                assert!(env
                    .svm
                    .get_account(&closing_portfolio)
                    .is_none_or(|account| account.data.is_empty()));
            } else {
                assert_eq!(
                    env.svm.get_account(&closing_portfolio),
                    Some(closing_before.clone())
                );
            }
            assert_eq!(
                env.account_lamports(env.market),
                market_lamports_before + if closed { closing_before.lamports } else { 0 }
            );
            assert_eq!(
                env.token_amount(source),
                source_before - deposited_amount as u64
            );
            assert_eq!(
                env.token_amount(env.vault),
                vault_before + deposited_amount as u64
            );
            assert_eq!(
                env.primary_portfolio(DEPOSITING).capital.get(),
                20 + deposited_amount
            );
            assert_eq!(env.primary_portfolio(WITHDRAWING).capital.get(), CAPITAL);
            assert_eq!(env.token_amount(destination), destination_before);
            let group = env.primary_market_state().1;
            assert_eq!(group.c_tot, group_before.c_tot + deposited_amount);
            assert_eq!(group.vault, group_before.vault + deposited_amount);
            assert_eq!(env.mint_supply(), supply_before);
        }

        env.land_retained(valid_withdraw)
            .expect("the exact-capital withdrawal remains live after the engine rejection");
        assert_eq!(env.primary_portfolio(WITHDRAWING).capital.get(), 0);
        assert_eq!(
            env.token_amount(destination),
            destination_before + CAPITAL as u64
        );
        assert_eq!(env.token_amount(source), source_before - DEPOSIT as u64);
        assert_eq!(
            env.token_amount(env.vault),
            vault_before + DEPOSIT as u64 - CAPITAL as u64
        );
        let group = env.primary_market_state().1;
        assert_eq!(group.c_tot, group_before.c_tot + DEPOSIT - CAPITAL);
        assert_eq!(group.vault, group_before.vault + DEPOSIT - CAPITAL);
        assert_eq!(env.mint_supply(), supply_before);

        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("public-only exact rollback and retries");
        assert_eq!(trace.out_of_band_economic_mutations, 0);
        assert_eq!(trace.steps.len(), 4);
        assert!(!trace.steps[0].succeeded);
        assert_eq!(trace.steps[0].rejected_exact_writable_rollback, Some(true));
        assert_eq!(trace.steps[0].rejected_no_program_lamport_delta, Some(true));
        assert!(trace.steps[1..].iter().all(|step| step.succeeded));
        eprintln!(
            "INV-080 close_first={close_first}: exact EngineLockActive at instruction \
             {rejection_index}, two successful prefixes rolled back ({} CU), rent={} lamports, \
             deposit={DEPOSIT}, valid withdrawal={CAPITAL}",
            rejection.compute_units_consumed, closing_before.lamports
        );
    }
}
