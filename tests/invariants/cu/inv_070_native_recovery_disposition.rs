//! INV-070 / row418: native claims survive Recovery force-close and reach actual
//! terminal disposal. Price direction and force-close partition are crossed;
//! unsynced vault lamports remain external to every user claim and engine stock.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [1_000, 1_300];
const TOTAL: u64 = CAPITAL[0] + CAPITAL[1];
const RAW: u64 = 37;
const LIMIT: u64 = 500_000;

fn native_frame(empty: &Account, amount: u64, unsynced: u64) -> Account {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.mint, spl_token::native_mint::ID);
    assert_eq!(token.is_native, COption::Some(empty.lamports));
    assert_eq!(token.amount, 0);
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + unsynced;
    expected
}

fn transaction(env: &V16CuEnv, ixs: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    instructions.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    tx
}

#[test]
fn v16_program_native_recovery_claims_have_bounded_terminal_disposition() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    for exit in [90u64, 110] {
        for split in [false, true] {
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let owners = [Keypair::new(), Keypair::new()];
            let wallets = owners.each_ref().map(Signer::pubkey);
            let tokens =
                wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
            let destination =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_tokens = [tokens[0], tokens[1], env.vault, destination]
                .map(|key| env.svm.get_account(&key).unwrap());
            let mint_frame = env.svm.get_account(&env.mint);
            let mut portfolios = [Pubkey::default(); 2];
            for i in 0..2 {
                env.svm.airdrop(&wallets[i], 1_000_000_000).unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                portfolios[i] = key.pubkey();
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(wallets[i], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                    ],
                    &[&owners[i]],
                )
                .unwrap();
                env.portfolios.push(portfolios[i]);
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::transfer(&wallets[i], &tokens[i], CAPITAL[i]),
                        spl_token::instruction::sync_native(&spl_token::ID, &tokens[i]).unwrap(),
                    ],
                    &[&owners[i]],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                    vec![
                        AccountMeta::new(wallets[i], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[i]],
                )
                .unwrap();
            }
            env.configure_permissionless_resolve_with_cu(100, 3);
            env.configure_auth_mark_for_asset_as_admin(0, 0, 100);
            env.trade_with_cu(
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                2 * POS_SCALE as i128,
                100,
                0,
            );
            env.svm.warp_to_slot(1);
            env.push_auth_mark_with_cu(1, exit);
            for portfolio in portfolios {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: crank_observations(0),
                    },
                );
            }
            let pnl = 2 * (i128::from(exit) - 100);
            let payouts = [
                u64::try_from(i128::from(CAPITAL[0]) + pnl).unwrap(),
                u64::try_from(i128::from(CAPITAL[1]) - pnl).unwrap(),
            ];
            for i in 0..2 {
                let p = env.portfolio_state(portfolios[i]);
                assert_eq!(p.capital.get() as i128 + p.pnl.get(), payouts[i].into());
            }
            let winner = usize::from(pnl < 0);
            assert_eq!(env.portfolio_state(portfolios[winner]).pnl.get(), pnl.abs());
            let economic_keys = [env.market, portfolios[0], portfolios[1]];
            let before_donation = economic_keys.map(|key| env.svm.get_account(&key));
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&admin.pubkey(), &env.vault, RAW),
                &[&admin],
            )
            .unwrap();
            assert_eq!(
                economic_keys.map(|key| env.svm.get_account(&key)),
                before_donation
            );
            env.svm.warp_to_slot(2);
            env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 2, 0);
            assert_eq!(
                env.market_state().1.assets[0].lifecycle,
                AssetLifecycleV16::Recovery
            );
            env.svm.warp_to_slot(5);

            let tracked = [
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                portfolios[0],
                portfolios[1],
                tokens[0],
                tokens[1],
                wallets[0],
                wallets[1],
                admin.pubkey(),
                destination,
            ];
            let census = |env: &V16CuEnv, paid: [u64; 2], oi: u128| {
                let (_, group) = env.market_state();
                let ps = portfolios.map(|key| env.portfolio_state(key));
                let remaining = TOTAL - paid.iter().sum::<u64>();
                assert_eq!(group.vault, remaining.into());
                assert_eq!(group.insurance, 0);
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert_eq!(
                    group.source_claim_bound_total_num,
                    if paid[winner] == 0 {
                        pnl.unsigned_abs() * BOUND_SCALE
                    } else {
                        0
                    }
                );
                assert_eq!(
                    (
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q
                    ),
                    (oi, oi)
                );
                assert_eq!(
                    env.svm.get_account(&env.vault),
                    Some(native_frame(&empty_tokens[2], remaining, RAW))
                );
                for i in 0..2 {
                    assert_eq!(
                        env.svm.get_account(&tokens[i]),
                        Some(native_frame(&empty_tokens[i], paid[i], 0))
                    );
                }
                assert_eq!(
                    env.svm.get_account(&destination),
                    Some(empty_tokens[3].clone())
                );
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                assert_market_stock_census(
                    "native Recovery disposition",
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &ps,
                    remaining.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("native Recovery disposition", &group, &ps)
                    .unwrap();
            };
            census(&env, [0, 0], 2 * POS_SCALE);
            let chunks = if split {
                vec![POS_SCALE, POS_SCALE]
            } else {
                vec![2 * POS_SCALE]
            };
            let mut peak = 0;
            let mut remaining_q = 2 * POS_SCALE;
            for close_q in chunks {
                let force_close = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ],
                    data: ProgInstruction::ForceCloseAbandonedAsset {
                        asset_index: 0,
                        now_slot: 5,
                        close_q,
                    }
                    .encode(),
                };
                // An ordinary unfunded SPL transfer aborts after Recovery progress.
                let suffix = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &destination,
                    &tokens[0],
                    &admin.pubkey(),
                    &[],
                    1,
                )
                .unwrap();
                env.svm.expire_blockhash();
                let tx = transaction(&env, &[force_close.clone(), suffix], &[&admin]);
                let mut keys = tx.message.account_keys.clone();
                keys.extend(tracked);
                keys.sort_unstable();
                keys.dedup();
                let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
                let failure = env.svm.send_transaction(tx).expect_err("late SPL suffix");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(
                            spl_token::error::TokenError::InsufficientFunds as u32
                        )
                    )
                );
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", env.program_id))
                        .count(),
                    1
                );
                for (key, mut expected) in keys.iter().zip(before) {
                    if *key == env.payer.pubkey() {
                        expected.as_mut().unwrap().lamports -=
                            2 * FeeStructure::default().lamports_per_signature;
                    }
                    assert_eq!(
                        env.svm.get_account(key),
                        expected,
                        "complete Recovery rollback {key}"
                    );
                }
                peak = peak.max(failure.meta.compute_units_consumed);
                census(&env, [0, 0], remaining_q);
                env.svm.expire_blockhash();
                let tx = transaction(&env, &[force_close], &[]);
                let before: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
                let meta = env
                    .svm
                    .send_transaction(tx)
                    .expect("unchanged Recovery retry");
                peak = peak.max(meta.compute_units_consumed);
                for (key, expected) in tracked.iter().zip(before) {
                    if !economic_keys.contains(key) {
                        assert_eq!(
                            env.svm.get_account(key),
                            expected,
                            "Recovery custody frame {key}"
                        );
                    }
                }
                remaining_q -= close_q;
                census(&env, [0, 0], remaining_q);
            }
            for portfolio in portfolios {
                assert!(!has_active_leg_for_asset(
                    &env.portfolio_state(portfolio),
                    0
                ));
            }
            peak = peak.max(env.resolve());
            env.svm.warp_to_slot(8);
            let mut paid = [0; 2];
            // Settle the losing claim first so this finite schedule has no funding dependency.
            let loser = usize::from(pnl > 0);
            for i in [loser, 1 - loser] {
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(wallets[i], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                    .encode(),
                };
                let tx = transaction(&env, &[ix], &[]);
                peak = peak.max(
                    env.svm
                        .send_transaction(tx)
                        .expect("public Recovery claim payout")
                        .compute_units_consumed,
                );
                assert!(resolved_portfolio_is_terminal(&env, portfolios[i]));
                paid[i] = payouts[i];
                census(&env, paid, 0);
            }
            assert_eq!(paid.iter().sum::<u64>(), TOTAL);
            for i in 0..2 {
                peak = peak.max(env.close_portfolio_with_cu(&owners[i], portfolios[i]));
            }
            let (_, terminal) = env.market_state();
            assert_eq!(
                (
                    terminal.materialized_portfolio_count,
                    terminal.vault,
                    terminal.c_tot,
                    terminal.pnl_pos_tot
                ),
                (0, 0, 0, 0)
            );
            assert_market_stock_census(
                "native Recovery terminal",
                &terminal,
                &env.svm.get_account(&env.market).unwrap().data,
                &[],
                0,
            )
            .unwrap();
            assert_reservation_encumbrance_census("native Recovery terminal", &terminal, &[])
                .unwrap();
            let market_before = env.svm.get_account(&env.market).unwrap();
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let tx = transaction(&env, &[close], &[&admin]);
            peak = peak.max(
                env.svm
                    .send_transaction(tx)
                    .expect("one bounded native terminal close")
                    .compute_units_consumed,
            );
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            let mut expected_admin = admin_before;
            expected_admin.lamports +=
                market_before.lamports - tombstone_rent + empty_tokens[2].lamports + RAW;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(
                env.svm.get_account(&destination),
                Some(empty_tokens[3].clone())
            );
            assert_eq!(env.svm.get_account(&env.mint), mint_frame);
            for i in 0..2 {
                assert_eq!(
                    env.svm.get_account(&tokens[i]),
                    Some(native_frame(&empty_tokens[i], payouts[i], 0))
                );
                let mut wallet = env.svm.get_account(&wallets[i]).unwrap();
                let unwrap = spl_token::instruction::close_account(
                    &spl_token::ID,
                    &tokens[i],
                    &wallets[i],
                    &wallets[i],
                    &[],
                )
                .unwrap();
                let tx = transaction(&env, &[unwrap], &[&owners[i]]);
                peak = peak.max(
                    env.svm
                        .send_transaction(tx)
                        .expect("owner native redemption")
                        .compute_units_consumed,
                );
                wallet.lamports += empty_tokens[i].lamports + payouts[i];
                assert_eq!(env.svm.get_account(&wallets[i]), Some(wallet));
                assert!(env
                    .svm
                    .get_account(&tokens[i])
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                assert_eq!(env.svm.get_account(&env.market), Some(tombstone.clone()));
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
            }
            assert_cu_within("native Recovery terminal disposition", peak, LIMIT);
            eprintln!("row418 native Recovery: exit={exit}, split={split}, payouts={payouts:?}, unsynced={RAW}, peak_CU={peak}");
        }
    }
}
