//! INV-073/078: an unpaid source claimant can normalize expired backing without custody.
//! Unlike the paid-receipt recreation histories, the absent owner's first payout and receipt
//! are still pending. A missing-ATA suffix rolls back expiry and a peer's actual SPL top-up;
//! keeper-only normalization, ATA recreation and settlement then return all senior capital.

use super::*;
use crate::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn land(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    rent: u64,
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    env.svm.expire_blockhash();
    let ixs: Vec<_> = [heap_ix(), cu_ix()]
        .into_iter()
        .chain(instructions.iter().cloned())
        .collect();
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let meta = match &result {
        Ok(meta) => {
            payer.lamports -= rent;
            meta
        }
        Err(failure) => &failure.meta,
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
    assert_cu_within(
        "unpaid source custody retry",
        meta.compute_units_consumed,
        if instructions.len() == 1 {
            CUSTODY_CU_LIMIT
        } else {
            600_000
        },
    );
    result
}

#[test]
fn v16_program_unpaid_source_claimant_recovers_missing_custody_after_expiry_rollback() {
    const CAPITAL: u128 = 1_000;
    const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
    const RESIDUAL: u128 = 501 + 350;
    const SUPPLY: u128 = 3_852;
    let expected = FACES.map(|face| {
        if face == 0 {
            0
        } else {
            CAPITAL + face * RESIDUAL / 3_000
        }
    });
    let mut world = World::before_receipts();
    for _ in 0..8 {
        if world.receipt(0).present {
            break;
        }
        world.land(&[world.payout(0, false)], false).unwrap();
    }
    let early = world.receipt(0);
    assert!(early.present && !early.finalized);
    assert_eq!(early.paid_effective, 700 * 501 / 3_000);
    assert_eq!(world.env.token_amount(world.actors[0].token), 1_116);
    let source = world.env.portfolio_state(world.actors[2].portfolio);
    assert_eq!(source.capital.get(), CAPITAL);
    assert_eq!(source.pnl.get(), 1_000);
    assert_eq!(
        state::portfolio_source_domain(&source, 3)
            .source_claim_bound_num
            .get(),
        1_000 * BOUND_SCALE
    );
    assert!(!world.receipt(2).present && !world.receipt(4).present);
    assert_eq!(world.env.token_amount(world.actors[2].token), 0);

    let owners: [Pubkey; 5] = std::array::from_fn(|i| world.actors[i].owner.pubkey());
    let portfolios: [Pubkey; 5] = std::array::from_fn(|i| world.actors[i].portfolio);
    let tokens: [Pubkey; 5] = std::array::from_fn(|i| world.actors[i].token);
    let payouts: [Instruction; 5] = std::array::from_fn(|i| world.payout(i, false));
    let topup = world.payout(0, true);
    let provider_token = world.provider_token;
    send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        spl_token::instruction::close_account(
            &spl_token::ID,
            &tokens[2],
            &owners[2],
            &owners[2],
            &[],
        )
        .unwrap(),
        &[&world.actors[2].owner],
    )
    .unwrap();
    let owner_lamports = world.env.svm.get_account(&owners[2]).unwrap().lamports;
    send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        system_instruction::transfer(&owners[2], &world.env.payer.pubkey(), owner_lamports),
        &[&world.actors[2].owner],
    )
    .unwrap();
    let mut env = world.env;
    drop(world.actors);
    assert!(owners.iter().all(|owner| *owner != env.payer.pubkey()));
    assert_ne!(env.admin.pubkey(), env.payer.pubkey());
    let absent_owner = env.svm.get_account(&owners[2]);
    let absent_token = env.svm.get_account(&tokens[2]);
    for absent in [&absent_owner, &absent_token] {
        assert!(absent.as_ref().is_none_or(|a| {
            a.lamports == 0 && a.data.is_empty() && a.owner == solana_sdk::system_program::ID
        }));
    }
    let tracked: Vec<_> = [
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        env.admin.pubkey(),
        provider_token,
        solana_sdk::sysvar::clock::id(),
    ]
    .into_iter()
    .chain(owners)
    .chain(portfolios)
    .chain(tokens)
    .collect();
    let frame = |env: &V16CuEnv| {
        tracked
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };

    // Retained CloseResolved bytes use authenticated time, four slots past backing expiry.
    env.svm.warp_to_slot(17);
    let before = frame(&env);
    let initial = env.market_state().1;
    assert_eq!(initial.source_backing_buckets[3].expiry_slot, 13);
    assert_eq!(
        initial.source_backing_buckets[3].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(
        initial.source_credit[3].fresh_reserved_backing_num,
        350 * BOUND_SCALE
    );
    assert_eq!(initial.resolved_payout_ledger.snapshot_residual, 501);
    let failed = land(
        &mut env,
        &[payouts[2].clone(), topup.clone(), payouts[2].clone()],
        0,
    )
    .expect_err("first source payout requires the missing ATA");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(
            4,
            InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
        )
    );
    for (program, count) in [(env.program_id, 2), (spl_token::ID, 1)] {
        assert_eq!(
            failed
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count,
            "expiry and peer transfer must execute before custody rejection"
        );
    }
    assert_eq!(
        frame(&env),
        before,
        "exact economic, custody, receipt and rent rollback"
    );
    let rollback_cu = failed.meta.compute_units_consumed;

    let normalize_cu = land(&mut env, &payouts[2..3], 0)
        .unwrap()
        .compute_units_consumed;
    let normalized = env.market_state().1;
    assert_eq!(
        normalized.source_backing_buckets[3].status,
        BackingBucketStatusV16::Expired
    );
    assert_eq!(normalized.source_credit[3].fresh_reserved_backing_num, 0);
    assert_eq!(normalized.resolved_payout_ledger.snapshot_slot, 12);
    assert_eq!(
        normalized.resolved_payout_ledger.snapshot_residual,
        RESIDUAL
    );
    assert_eq!(normalized.vault, initial.vault);
    assert_eq!(env.svm.get_account(&tokens[2]), absent_token);
    assert_eq!(env.portfolio_state(portfolios[2]).capital.get(), CAPITAL);
    assert!(!resolved_receipt(&env.portfolio_state(portfolios[2])).present);
    assert_eq!(resolved_receipt(&env.portfolio_state(portfolios[0])), early);

    let recreate = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new_readonly(owners[2], false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let repair_cu = land(&mut env, &[recreate, payouts[2].clone()], rent)
        .expect("unrelated keeper recreates custody and pays the source claimant")
        .compute_units_consumed;
    let receipt = resolved_receipt(&env.portfolio_state(portfolios[2]));
    assert!(receipt.present && !receipt.finalized);
    assert_eq!(receipt.terminal_positive_claim_face, 1_000);
    assert_eq!(receipt.prior_bound_contribution_num, 1_000 * BOUND_SCALE);
    assert_eq!(receipt.live_released_face_at_receipt, 0);
    assert_eq!(receipt.paid_effective, 1_000 * RESIDUAL / 3_000);
    assert_eq!(env.token_amount(tokens[2]) as u128, expected[2]);
    assert_eq!(env.portfolio_state(portfolios[2]).capital.get(), 0);
    assert_eq!(env.market_state().1.vault + expected[2], normalized.vault);
    assert_eq!(env.svm.get_account(&owners[2]), absent_owner);

    let mut peak_cu = normalize_cu.max(repair_cu).max(rollback_cu);
    let mut calls = 0;
    for _ in 0..8 {
        for actor in [4, 0, 2] {
            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                continue;
            }
            let meta = land(&mut env, &payouts[actor..=actor], 0)
                .expect("funded claimants must finish without owner or reserve signatures");
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            calls += 1;
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&env, *key))
        {
            break;
        }
    }
    for (actor, portfolio) in portfolios.into_iter().enumerate() {
        assert!(resolved_portfolio_is_terminal(&env, portfolio));
        assert_eq!(env.token_amount(tokens[actor]) as u128, expected[actor]);
    }
    let terminal = env.market_state().1;
    assert_eq!(
        [
            terminal.c_tot,
            terminal.pnl_pos_tot,
            terminal.source_claim_bound_total_num
        ],
        [0; 3]
    );
    assert_eq!(terminal.vault, 2);
    assert_eq!(env.token_amount(env.vault), 2);
    assert_eq!(env.token_amount(provider_token), 1);
    assert_eq!(terminal.vault + expected.iter().sum::<u128>() + 1, SUPPLY);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        SUPPLY
    );
    assert_eq!(env.svm.get_account(&owners[2]), absent_owner);
    for (key, old) in tracked.iter().zip(before) {
        if ![
            env.market,
            env.vault,
            portfolios[0],
            portfolios[2],
            portfolios[4],
            tokens[0],
            tokens[2],
            tokens[4],
        ]
        .contains(key)
        {
            assert_eq!(env.svm.get_account(key), old, "unrelated account {key}");
        }
    }
    println!("INV-073/078 unpaid source custody: normalize CU {normalize_cu}; repair+payout CU {repair_cu}; rollback CU {rollback_cu}; peak CU {peak_cu}; terminal calls {calls}; payouts {expected:?}");
}
