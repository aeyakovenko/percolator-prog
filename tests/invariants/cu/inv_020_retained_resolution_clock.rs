//! INV-020: retained crank/resolve bytes across independent Clock freshness boundaries.
//! Public System/SPL/ATA/wrapper construction only; Clock and Pyth reports are external.
//! A rejected transaction cannot commit a new observation epoch or mark. A committed
//! report, not caller time or settlement progress, determines terminal maturity.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const OPEN_SLOT: u64 = 1;
const DELIVERY_SLOT: u64 = 5;
const STALE_SLOTS: u64 = 5;
const OPEN_TIME: i64 = 100;
const OPEN_PRICE: u64 = 100;
const CAPITAL: u64 = 1_000;

fn prepare(env: &mut V16CuEnv, instructions: &[Instruction]) -> Transaction {
    env.svm.expire_blockhash();
    Transaction::new_signed_with_payer(
        &[&[heap_ix(), cu_ix()][..], instructions].concat(),
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    )
}

fn deliver(
    env: &mut V16CuEnv,
    tx: Transaction,
    tracked: &[Pubkey],
    expected_error: Option<(u8, PercolatorError)>,
    label: &str,
) -> u64 {
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = match expected_error {
        Some((index, error)) => {
            let failure = result.expect_err(label);
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
                "{label}: {failure:?}"
            );
            for (key, account) in keys.iter().zip(before) {
                assert_eq!(env.svm.get_account(key), account, "{label}: rollback {key}");
            }
            failure.meta.compute_units_consumed
        }
        None => {
            result
                .unwrap_or_else(|error| panic!("{label}: {error:?}"))
                .compute_units_consumed
        }
    };
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer,
        "{label}: network fee only"
    );
    assert_cu_within(label, cu, 500_000);
    cu
}

fn submit(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    expected_error: Option<(u8, PercolatorError)>,
    label: &str,
) -> u64 {
    let tx = prepare(env, instructions);
    deliver(env, tx, tracked, expected_error, label)
}

fn crank_ix(env: &V16CuEnv, portfolio: Pubkey, report: Pubkey, caller_slot: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(report, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: caller_slot,
            observations: crank_observations_with_accounts(0, 1),
        }
        .encode(),
    }
}

#[test]
fn v16_program_retained_crank_resolution_uses_only_committed_clock_provenance() {
    let mut peak_cu = 0;
    let mut rejections = 0;
    let mut payouts = 0;
    for caller_slot in [0, u64::MAX] {
        for (case, report_price, publish_time, delivery_time) in [
            ("unchanged at freshness boundary", 100_u64, 100_i64, 160_i64),
            ("new at freshness boundary", 110, 101, 161),
            ("new one second stale", 110, 101, 162),
        ] {
            let label = format!("{case}, caller_slot={caller_slot}");
            // Independent input model: provider age is in seconds; resolution age is in slots.
            let admissible = delivery_time - publish_time <= 60;
            let advanced = admissible && publish_time > OPEN_TIME;
            let good_slot = if advanced { DELIVERY_SLOT } else { OPEN_SLOT };
            let accepted_price = if advanced { report_price } else { OPEN_PRICE };
            let accepted_time = if advanced { publish_time } else { OPEN_TIME };
            let deadline = good_slot + STALE_SLOTS;

            let mut env = inv018_public_spl_market(0);
            env.configure_permissionless_resolve_with_cu(STALE_SLOTS, 1);
            set_test_clock(&mut env, OPEN_SLOT, OPEN_TIME);
            let feed = [0xe2; 32];
            let initial = env.set_pyth_price_with_conf(&feed, OPEN_PRICE as i64, -6, 0, OPEN_TIME);
            configure_single_pyth(&mut env, feed, initial, OPEN_SLOT, OPEN_TIME, 100).unwrap();
            let owners = [Keypair::new(), Keypair::new()];
            let portfolios = owners.each_ref().map(|owner| {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                let portfolio = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &portfolio,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio.pubkey(), false),
                    ],
                    &[owner],
                )
                .unwrap();
                env.portfolios.push(portfolio.pubkey());
                portfolio.pubkey()
            });
            let wallets = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            for actor in 0..2 {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallets[actor],
                        &env.admin.pubkey(),
                        &[],
                        CAPITAL,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolios[actor], u128::from(CAPITAL)),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(wallets[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
            }
            env.trade_asset_with_cu(
                0,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                POS_SCALE as i128,
                OPEN_PRICE,
                0,
            );
            let report =
                env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, publish_time);
            let tracked = [
                env.market,
                portfolios[0],
                portfolios[1],
                env.mint,
                env.vault,
                wallets[0],
                wallets[1],
                owners[0].pubkey(),
                owners[1].pubkey(),
                env.admin.pubkey(),
                initial,
                report,
                solana_sdk::sysvar::clock::ID,
            ];
            let custody_keys = [env.mint, env.vault, wallets[0], wallets[1], initial, report];
            let custody_before = custody_keys.map(|key| env.svm.get_account(&key).unwrap());

            // Retain actual encoded metas/data before time advances. The first bundle is also
            // signed before delivery; later retries replace only transport blockhash/signature.
            let crank = crank_ix(&env, portfolios[0], report, caller_slot);
            let resolve = Instruction {
                program_id: env.program_id,
                accounts: vec![AccountMeta::new(env.market, false)],
                data: ProgInstruction::ResolveStalePermissionless {
                    now_slot: caller_slot,
                }
                .encode(),
            };
            let bundle = prepare(&mut env, &[crank.clone(), resolve.clone()]);
            set_test_clock(&mut env, DELIVERY_SLOT, delivery_time);
            peak_cu = peak_cu.max(deliver(
                &mut env,
                bundle,
                &tracked,
                Some((if admissible { 3 } else { 2 }, PercolatorError::OracleStale)),
                &label,
            ));
            rejections += 1;
            // A valid prefix must have reached the resolve suffix, yet none of its progress,
            // accepted report, mark movement, or certificate updates can survive that failure.
            assert_eq!(env.market_state().0.last_good_oracle_slot, OPEN_SLOT);
            assert_eq!(env.market_state().1.assets[0].slot_last, OPEN_SLOT);
            assert_eq!(env.market_state().1.assets[0].effective_price, OPEN_PRICE);

            if admissible {
                for expected_slot in OPEN_SLOT + 1..=DELIVERY_SLOT {
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        std::slice::from_ref(&crank),
                        &tracked,
                        None,
                        &label,
                    ));
                    let (cfg, group) = env.market_state();
                    assert_eq!(group.current_slot, DELIVERY_SLOT);
                    assert_eq!(
                        group.assets[0].slot_last, expected_slot,
                        "{label}: bounded slot rank"
                    );
                    assert!(
                        (OPEN_PRICE..=accepted_price).contains(&group.assets[0].effective_price)
                    );
                    assert_eq!(cfg.last_good_oracle_slot, good_slot);
                    assert_eq!(cfg.oracle_target_publish_time, accepted_time);
                    assert_eq!(cfg.oracle_leg_prices_e6[0], accepted_price);
                    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
                    assert_eq!(group.assets[0].oi_eff_short_q, POS_SCALE);
                }
                assert_eq!(
                    env.market_state().1.assets[0].effective_price,
                    accepted_price
                );
            } else {
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    std::slice::from_ref(&crank),
                    &tracked,
                    Some((2, PercolatorError::OracleStale)),
                    &label,
                ));
                rejections += 1;
            }
            assert_eq!(
                custody_keys.map(|key| env.svm.get_account(&key).unwrap()),
                custody_before
            );
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(profile.last_good_oracle_slot, good_slot);
            assert_eq!(profile.oracle_target_publish_time, accepted_time);
            assert_eq!(profile.oracle_leg_prices_e6[0], accepted_price);

            // Identical retained resolver bytes reject at the original deadline when a new
            // report committed, and at renewed expiry minus one. Unix time cannot mature slots.
            let mut premature_slots = vec![DELIVERY_SLOT];
            if advanced {
                premature_slots.extend([OPEN_SLOT + STALE_SLOTS, deadline - 1]);
            }
            for slot in premature_slots {
                set_test_clock(
                    &mut env,
                    slot,
                    delivery_time + (slot - DELIVERY_SLOT) as i64,
                );
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    std::slice::from_ref(&resolve),
                    &tracked,
                    Some((2, PercolatorError::OracleStale)),
                    &label,
                ));
                rejections += 1;
            }
            set_test_clock(
                &mut env,
                deadline,
                delivery_time + (deadline - DELIVERY_SLOT) as i64,
            );
            assert!(
                env.market_state().1.current_slot < deadline,
                "cached engine time remains earlier than the authenticated expiry"
            );
            peak_cu = peak_cu.max(submit(
                &mut env,
                std::slice::from_ref(&resolve),
                &tracked,
                None,
                &label,
            ));
            let (cfg, resolved) = env.market_state();
            assert_eq!(resolved.mode, MarketModeV16::Resolved);
            assert_eq!(resolved.resolved_slot, deadline);
            assert_eq!(resolved.assets[0].effective_price, accepted_price);
            assert_eq!(cfg.last_good_oracle_slot, good_slot);
            assert_eq!(
                custody_keys.map(|key| env.svm.get_account(&key).unwrap()),
                custody_before
            );
            peak_cu = peak_cu.max(submit(
                &mut env,
                std::slice::from_ref(&resolve),
                &tracked,
                Some((2, PercolatorError::EngineLockActive)),
                &label,
            ));
            rejections += 1;

            // Close the loser first, then pay both owners into their existing public ATAs.
            // No account restoration, token injection, or direct engine settlement is used.
            let gain = accepted_price - OPEN_PRICE;
            let expected = [CAPITAL + gain, CAPITAL - gain];
            for actor in [1, 0] {
                let close = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(owners[actor].pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(wallets[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                    .encode(),
                };
                if actor == 1 {
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        std::slice::from_ref(&close),
                        &tracked,
                        Some((2, PercolatorError::ExpectedSigner)),
                        &label,
                    ));
                    rejections += 1;
                    // The one-slot owner window is measured from the authenticated resolved
                    // slot. Neither owner signs the identical close at its exact expiry.
                    set_test_clock(
                        &mut env,
                        deadline + 1,
                        delivery_time + (deadline + 1 - DELIVERY_SLOT) as i64,
                    );
                }
                for round in 0..8 {
                    if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                        break;
                    }
                    let account_before = env.svm.get_account(&portfolios[actor]).unwrap();
                    peak_cu = peak_cu.max(submit(
                        &mut env,
                        std::slice::from_ref(&close),
                        &tracked,
                        None,
                        &label,
                    ));
                    assert_ne!(
                        env.svm.get_account(&portfolios[actor]).unwrap(),
                        account_before,
                        "{label}: terminal progress in round {round}"
                    );
                    assert!(env.token_amount(wallets[actor]) <= expected[actor]);
                }
                assert!(
                    resolved_portfolio_is_terminal(&env, portfolios[actor]),
                    "{label}: bounded terminal exit"
                );
                assert_eq!(
                    env.token_amount(wallets[actor]),
                    expected[actor],
                    "{label}: independent mark-to-payout equation"
                );
                payouts += 1;
            }
            let (_, final_group) = env.market_state();
            assert_eq!(final_group.mode, MarketModeV16::Resolved);
            assert_eq!(final_group.resolved_slot, deadline);
            assert_eq!(final_group.assets[0].effective_price, accepted_price);
            assert_eq!(final_group.assets[0].oi_eff_long_q, 0);
            assert_eq!(final_group.assets[0].oi_eff_short_q, 0);
            assert_eq!(final_group.vault, 0);
            assert_eq!(final_group.c_tot, 0);
            assert_eq!(env.token_amount(env.vault), 0);
            assert_eq!(
                env.token_amount(wallets[0]) + env.token_amount(wallets[1]),
                2 * CAPITAL
            );
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), custody_before[0]);
            assert_eq!(env.svm.get_account(&initial).unwrap(), custody_before[4]);
            assert_eq!(env.svm.get_account(&report).unwrap(), custody_before[5]);
        }
    }
    println!("retained resolution clock: 6 worlds, {rejections} exact rejections, {payouts} funded terminal exits, peak {peak_cu} CU");
}
