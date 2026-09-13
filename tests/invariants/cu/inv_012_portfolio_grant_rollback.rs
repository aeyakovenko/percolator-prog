//! INV-003/007/012/080/081: rollback restores the original grant's owner consent
//! after an entire same-address close, System refund and portfolio reinitialization.
//! A valid replacement grant is a positive control for the complete bundle.

use super::*;
use generation_bundle_rollback::sign;
use solana_sdk::{
    fee::FeeStructure, instruction::InstructionError, system_program, transaction::TransactionError,
};

#[test]
fn v16_program_failed_portfolio_reincarnation_preserves_retained_owner_grant() {
    let mut evidence = Evidence::default();
    let mut peak_cu = 0;
    for route in [Route::Single(1), Route::Batch] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let owner = h.owners[1].pubkey();
            let portfolio = h.portfolios[1];
            let id = h.env.portfolio_id(portfolio);
            let sequence = h.grant_sequence;
            let next_id = h.env.market_state().0.next_portfolio_id;
            let owner_accounts = vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(portfolio, false),
            ];
            let grant = |portfolio_id, expected_sequence| Instruction {
                program_id: h.env.program_id,
                accounts: vec![
                    AccountMeta::new(owner, true),
                    AccountMeta::new_readonly(h.env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new_readonly(h.matcher.0, false),
                    AccountMeta::new_readonly(h.matcher.1, false),
                    AccountMeta::new_readonly(h.matcher.2, false),
                ],
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id,
                    expected_sequence,
                    enabled: 1,
                    trade_fee_cap_bps: FEE_CAP,
                    expiry_slot: EXPIRY,
                }
                .encode(),
            };
            let retained_ix = grant(id, sequence);
            let repaired_ix = grant(next_id, 0);
            let retained = sign(&h, &[retained_ix.clone()]);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let before = h.frame();
            h.env
                .svm
                .simulate_transaction(retained.clone().into())
                .expect("owner's original grant is admissible before closing");
            assert_eq!(h.frame(), before);
            let prefix = vec![
                Instruction {
                    program_id: h.env.program_id,
                    accounts: vec![
                        AccountMeta::new(owner, true),
                        AccountMeta::new(h.env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(h.tokens[1], false),
                        AccountMeta::new(h.env.vault, false),
                        AccountMeta::new_readonly(h.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: h.env.withdraw_ix(portfolio, CAPITAL).encode(),
                },
                Instruction {
                    program_id: h.env.program_id,
                    accounts: owner_accounts.clone(),
                    data: ProgInstruction::ClosePortfolio {
                        portfolio_id: id,
                        expected_sequence: sequence + 1,
                        position_epoch: h.epoch,
                    }
                    .encode(),
                },
                system_instruction::transfer(
                    &owner,
                    &portfolio,
                    h.env
                        .svm
                        .minimum_balance_for_rent_exemption(h.env.portfolio_account_len),
                ),
                Instruction {
                    program_id: h.env.program_id,
                    accounts: owner_accounts,
                    data: ProgInstruction::InitPortfolio.encode(),
                },
            ];
            let mut control = prefix.clone();
            control.push(repaired_ix);
            h.env
                .svm
                .simulate_transaction(sign(&h, &control).into())
                .expect("the full lifecycle bundle accepts fresh incarnation consent");
            assert_eq!(h.frame(), before);
            let mut ixs = prefix;
            ixs.push(retained_ix);
            let tx = sign(&h, &ixs);
            let keys = tx
                .message
                .account_keys
                .iter()
                .copied()
                .chain([
                    h.env.mint,
                    h.env.vault,
                    h.tokens[0],
                    h.portfolios[0],
                    h.owners[0].pubkey(),
                ])
                .collect::<std::collections::BTreeSet<_>>();
            let mut frame = keys
                .into_iter()
                .map(|key| (key, h.env.svm.get_account(&key)))
                .collect::<Vec<_>>();
            let fee = FeeStructure::default().lamports_per_signature
                * u64::from(tx.message.header.num_required_signatures);
            let failure = h
                .env
                .svm
                .send_transaction(tx)
                .expect_err("an old portfolio grant cannot authorize its replacement");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    6,
                    InstructionError::Custom(PercolatorError::EngineStale as u32)
                )
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", h.env.program_id))
                    .count(),
                3
            );
            for program in [spl_token::ID, system_program::ID] {
                assert!(failure
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {program} success")));
            }
            assert!(!failure
                .meta
                .logs
                .iter()
                .any(|line| line.starts_with(&format!("Program {} invoke", h.matcher.0))));
            for (key, account) in &mut frame {
                if *key == h.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    h.env.svm.get_account(key),
                    *account,
                    "exact account, rent and SPL rollback: {key}"
                );
            }
            assert!(failure.meta.compute_units_consumed <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
            peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
            assert_eq!(h.env.market_state().0.next_portfolio_id, next_id);
            assert_eq!(h.env.portfolio_id(portfolio), id);
            h.assert_state();
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            h.env
                .svm
                .send_transaction(retained)
                .expect("rollback preserves the exact pre-signed owner grant");
            h.grant_sequence += 1;
            h.assert_state();
            let sizes = match route {
                Route::Single(_) => [0, direction, 0],
                Route::Batch => [0, direction, -2 * direction],
            };
            h.fill(route, sizes, direction < 0, &mut evidence);
            h.fill(route, sizes.map(|size| -size), direction > 0, &mut evidence);
            h.withdraw_all(direction < 0, &mut evidence);
            for actor in 0..2 {
                let token =
                    TokenAccount::unpack(&h.env.svm.get_account(&h.tokens[actor]).unwrap().data)
                        .unwrap();
                assert_eq!(token.owner, h.owners[actor].pubkey());
                assert_eq!(token.mint, h.env.mint);
                assert_eq!(token.amount as u128, CAPITAL);
            }
            evidence.worlds += 1;
        }
    }
    assert_eq!(evidence.worlds, 4);
    eprintln!("INV-003/012 portfolio grant rollback: worlds=4, lifecycle_rollbacks=4, restored_retained_grants=4, cpi_fills=8, owner_exits=8, peak_rollback_cu={peak_cu}");
}
