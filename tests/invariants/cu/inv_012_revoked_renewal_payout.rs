//! INV-012 / row 412: a failed payout bundle cannot commit renewed authority.
//! After bilateral partial reduction revokes the LP grant, owner renewal, matcher
//! exit and SPL withdrawal roll back on a later SPL transfer failure. Old consent
//! stays unusable; identical instructions retry after a public token top-up.

use super::*;
use solana_sdk::fee::FeeStructure;

fn grant(h: &History) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new_readonly(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new_readonly(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: h.env.portfolio_id(h.portfolios[1]),
            expected_sequence: h.grant_sequence,
            position_epoch: h.env.portfolio_position_epoch(h.portfolios[1]),
            asset_generation_frontier: h.env.market_state().1.next_market_id,
            enabled: 1,
            trade_fee_cap_bps: FEE_CAP,
            expiry_slot: EXPIRY,
        }
        .encode(),
    }
}

fn withdrawal(h: &History, actor: usize) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[actor].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[actor], false),
            AccountMeta::new(h.tokens[actor], false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: h.env.withdraw_ix(h.portfolios[actor], CAPITAL).encode(),
    }
}

fn frame(h: &History, tx: &Transaction, recipient: Pubkey) -> Vec<(Pubkey, Option<Account>)> {
    tx.message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.mint,
            h.env.vault,
            h.tokens[0],
            h.tokens[1],
            h.owners[1].pubkey(),
            h.env.admin.pubkey(),
            recipient,
        ])
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect()
}

fn rolled_back(h: &History, tx: &Transaction, before: Vec<(Pubkey, Option<Account>)>) {
    let fee = FeeStructure::default().lamports_per_signature * tx.signatures.len() as u64;
    for (key, mut expected) in before {
        if key == h.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(h.env.svm.get_account(&key), expected, "rollback: {key}");
    }
}

#[test]
fn v16_program_failed_renewal_exit_payout_preserves_revocation_and_instruction_retry() {
    let mut evidence = Evidence::default();
    let mut peak_bundle_cu = 0;
    for route in [Route::Single(1), Route::Batch] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let recipient = create_ata_for_test(
                &mut h.env.svm,
                &h.env.payer,
                h.env.admin.pubkey(),
                h.env.mint,
            );
            let size = direction * POS_SCALE as i128;
            h.fill(route, [0, 2 * size, 0], false, &mut evidence);
            let old = h.sign(&h.instruction(route, [0, -size, 0], false));
            let old_bytes = bincode::serialize(&old).unwrap();
            h.simulate(&old, 0, &mut evidence);

            let mut reduction = writer(&h, false, -size, h.epoch);
            reduction.data = h
                .env
                .trade_no_cpi_ix(h.portfolios[0], h.portfolios[1], 1, -size, PRICE, 0)
                .encode();
            h.env
                .svm
                .send_transaction(signed_writer(&h, vec![reduction]))
                .expect("owner-signed partial reduction commits automatic revocation");
            h.positions[1] -= size;
            h.epoch += 1;
            assert_revoked(&h);
            let sequences = h.portfolios.map(|p| h.env.portfolio_matcher_sequence(p));
            let current_ix = h.instruction(route, [0, -size, 0], false);
            let current_old_grant = h.sign(&current_ix);
            let mut renewed_exit = current_ix.clone();
            match &mut renewed_exit {
                ProgInstruction::TradeCpi {
                    account_b_matcher_sequence,
                    ..
                }
                | ProgInstruction::BatchTradeCpi {
                    account_b_matcher_sequence,
                    ..
                } => {
                    *account_b_matcher_sequence = h.grant_sequence + 1;
                }
                _ => unreachable!(),
            }

            // All consent in this bundle is signed AFTER the revoking transition.
            // The final transfer is owner-authorized but lacks one external token.
            let mut prefix = vec![grant(&h), cpi(&h, &renewed_exit), withdrawal(&h, 0)];
            prefix.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &h.tokens[0],
                    &recipient,
                    &h.owners[0].pubkey(),
                    &[],
                    CAPITAL as u64 + 1,
                )
                .unwrap(),
            );
            let bundle = signed_writer(&h, prefix.clone());
            let bundle_bytes = bincode::serialize(&bundle).unwrap();
            let before = frame(&h, &bundle, recipient);
            let failed = h.env.svm.send_transaction(bundle.clone()).expect_err(
                "late SPL insufficiency rolls back the renewed grant and economic exit",
            );
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32
                    )
                )
            );
            for (program, successes) in
                [(h.env.program_id, 3), (h.matcher.0, 1), (spl_token::ID, 1)]
            {
                assert_eq!(
                    failed
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    successes
                );
            }
            rolled_back(&h, &bundle, before);
            peak_bundle_cu = peak_bundle_cu.max(failed.meta.compute_units_consumed);
            assert_revoked(&h);
            assert_eq!(h.env.token_amount(recipient), 0);

            for (tx, error) in [
                (old.clone(), PercolatorError::EngineStale),
                (current_old_grant.clone(), PercolatorError::Unauthorized),
            ] {
                let before = frame(&h, &tx, recipient);
                reject(&mut h, tx.clone(), error, &mut evidence);
                rolled_back(&h, &tx, before);
                assert_revoked(&h);
            }
            assert_eq!(bincode::serialize(&old).unwrap(), old_bytes);

            // Only external token balances/supply change before the instruction retry.
            let wrapper_before = [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1]
                .map(|key| h.env.svm.get_account(&key));
            send_raw_tx(
                &mut h.env.svm,
                &h.env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &h.env.mint,
                    &h.tokens[0],
                    &h.env.admin.pubkey(),
                    &[],
                    1,
                )
                .unwrap(),
                &[&h.env.admin],
            )
            .expect("public SPL top-up enables the retained transfer suffix");
            assert_eq!(
                [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1]
                    .map(|key| h.env.svm.get_account(&key)),
                wrapper_before
            );
            assert_eq!(h.env.token_amount(h.tokens[0]), 1);
            assert_eq!(bincode::serialize(&bundle).unwrap(), bundle_bytes);
            // A landed failure consumes its signature in normal runtime history.
            h.env.svm.expire_blockhash();
            let retry = signed_writer(&h, prefix);
            let mut renewed_message = bundle.message.clone();
            renewed_message.recent_blockhash = retry.message.recent_blockhash;
            assert_eq!(retry.message, renewed_message);
            assert_ne!(
                retry.message.recent_blockhash,
                bundle.message.recent_blockhash
            );
            let meta = h
                .env
                .svm
                .send_transaction(retry)
                .expect("unchanged post-revocation owner consent retries the entire payout");
            peak_bundle_cu = peak_bundle_cu.max(meta.compute_units_consumed);
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", h.matcher.0))
                    .count(),
                1
            );
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                2
            );
            let config = h.env.portfolio_matcher_config(h.portfolios[1]);
            assert_eq!(config.enabled(), 1);
            assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
            assert_eq!(config.matcher_program, h.matcher.0.to_bytes());
            assert_eq!(config.matcher_context, h.matcher.1.to_bytes());
            assert_eq!(config.matcher_delegate, h.matcher.2.to_bytes());
            assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), EXPIRY);
            for (actor, portfolio) in h.portfolios.iter().enumerate() {
                let state = h.env.portfolio_state(*portfolio);
                assert_eq!(state.owner, h.owners[actor].pubkey().to_bytes());
                assert_eq!(state.capital.get(), if actor == 0 { 0 } else { CAPITAL });
                assert_eq!(state.pnl.get(), 0);
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&state)),
                    0
                );
                assert_eq!(h.env.portfolio_position_epoch(*portfolio), h.epoch + 1);
                assert_eq!(
                    h.env.portfolio_matcher_sequence(*portfolio),
                    sequences[actor] + 1
                );
                assert_eq!(h.env.token_amount(h.tokens[actor]), 0);
            }
            assert_eq!(h.env.token_amount(recipient) as u128, CAPITAL + 1);
            let group = h.env.market_state().1;
            assert_eq!(
                (group.c_tot, group.vault, group.insurance),
                (CAPITAL, CAPITAL, 0)
            );
            for asset in 0..3 {
                assert_eq!(group.assets[asset].market_id, h.ids[asset]);
                assert_eq!(
                    (
                        group.assets[asset].oi_eff_long_q,
                        group.assets[asset].oi_eff_short_q
                    ),
                    (0, 0)
                );
            }
            assert_eq!(h.env.token_amount(h.env.vault) as u128, CAPITAL);
            let payout = withdrawal(&h, 1);
            send_raw_tx(&mut h.env.svm, &h.env.payer, payout, &[&h.owners[1]])
                .expect("the other owner's full principal remains withdrawable");
            assert_eq!(h.env.token_amount(h.tokens[1]) as u128, CAPITAL);
            assert_eq!(h.env.portfolio_state(h.portfolios[1]).capital.get(), 0);
            let group = h.env.market_state().1;
            assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
            assert_eq!(h.env.token_amount(h.env.vault), 0);
            let supply = Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
                .unwrap()
                .supply;
            assert_eq!(supply as u128, 2 * CAPITAL + 1);
            assert_eq!(
                supply,
                h.env.token_amount(recipient) + h.env.token_amount(h.tokens[1])
            );
            evidence.worlds += 1;
        }
    }
    assert_eq!(evidence.worlds, 4);
    assert_eq!(evidence.rejections, 8);
    assert!(peak_bundle_cu <= 1_400_000);
    println!("row412 renewal/exit/payout: 4 worlds, 4 late SPL rollbacks, 8 old-consent rollbacks, 4 unchanged-instruction funded retries, 8 full principal payouts; peak bundle CU={peak_bundle_cu}, denied consumer CU={}", evidence.rejection_cu);
}
