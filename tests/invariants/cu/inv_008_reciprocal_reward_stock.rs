//! INV-008 / row415: consumed withdrawal consent survives reciprocal fee stock.
//! The emptied recipient is replenished, then pays its own fee back to that
//! source. Both fee legs and a fresh payout must roll back on a retained suffix.
//! Finite public-SBF conformance; insurance withdrawal binding remains open.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const INITIAL: [u64; 3] = [503, 37, 103];
const BIRTHS: [u64; 3] = [0, 3, 3];
const NOW: u64 = 4;
const FEE_RATE: u64 = 41;

#[derive(Clone, Copy)]
enum Action {
    Original,
    ForwardFee,
    ReturnFee,
    Fresh,
    SourceExit,
    Fail,
}

#[derive(Default)]
struct Book {
    charged: [bool; 2],
    original_paid: bool,
    fresh_paid: bool,
    source_paid: bool,
}

#[test]
fn v16_consumed_withdrawal_survives_reciprocal_fee_stock_and_atomic_retry() {
    let mut evidence = Evidence::default();
    let mut rolled_back_payouts = 0;
    for share in [5_000u16, 10_000] {
        for split in [false, true] {
            let label = format!("reciprocal stock share={share}, split={split}");
            let gross = [0, 1].map(|i| (NOW - BIRTHS[i]) * FEE_RATE);
            let reward = gross.map(|fee| fee * u64::from(share) / 10_000);
            let fresh_amount = reward[0] - gross[1];
            let source_amount = INITIAL[0] - gross[0] + reward[1];
            assert!(fresh_amount >= INITIAL[1], "stale amount remains funded");
            let supply = INITIAL.iter().sum::<u64>();
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    maintenance_fee_per_slot: FEE_RATE.into(),
                    ..V16CuMarketParams::default()
                },
            );
            env.update_maintenance_fee_policy_with_cu(share);
            let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
            let mut portfolios = [Pubkey::default(); 3];
            let mut tokens = [Pubkey::default(); 3];
            for actor in 0..3 {
                env.svm.warp_to_slot(BIRTHS[actor]);
                env.svm
                    .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                    .unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                portfolios[actor] = key.pubkey();
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
                tokens[actor] =
                    create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &tokens[actor],
                        &env.admin.pubkey(),
                        &[],
                        INITIAL[actor],
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolios[actor], INITIAL[actor].into()),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &env.admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            let ids = portfolios.map(|key| env.portfolio_id(key));
            let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
            let controls = env.control_sequences(0);
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!(mint.supply, supply);
            assert_eq!(mint.mint_authority, COption::None);
            let frame: Vec<_> = portfolios
                .into_iter()
                .chain(tokens)
                .chain(owners.iter().map(Signer::pubkey))
                .chain([
                    env.market,
                    env.vault,
                    env.mint,
                    env.vault_authority,
                    env.admin.pubkey(),
                ])
                .collect();
            let untouched: Vec<_> = owners
                .iter()
                .map(Signer::pubkey)
                .chain([portfolios[2], tokens[2], env.admin.pubkey()])
                .map(|key| (key, env.svm.get_account(&key)))
                .collect();
            let check = |env: &V16CuEnv, book: &Book| {
                let fees = [0, 1].map(|i| u64::from(book.charged[i]) * gross[i]);
                let rewards = [0, 1].map(|i| u64::from(book.charged[i]) * reward[i]);
                let paid = [
                    u64::from(book.source_paid) * source_amount,
                    u64::from(book.original_paid) * INITIAL[1]
                        + u64::from(book.fresh_paid) * fresh_amount,
                    0,
                ];
                let capital = [
                    INITIAL[0] - fees[0] + rewards[1] - paid[0],
                    INITIAL[1] + rewards[0] - fees[1] - paid[1],
                    INITIAL[2],
                ];
                let accounts = portfolios.map(|key| env.portfolio_state(key));
                for actor in 0..3 {
                    let p = &accounts[actor];
                    assert_eq!(p.capital.get(), u128::from(capital[actor]), "{label}");
                    assert_eq!(p.pnl.get(), 0);
                    assert_eq!(p.reserved_pnl.get(), 0);
                    assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                    assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                    assert_eq!(
                        env.portfolio_position_epoch(portfolios[actor]),
                        epochs[actor]
                    );
                    assert_eq!(
                        env.portfolio_matcher_sequence(portfolios[actor]),
                        1 + match actor {
                            0 => u64::from(book.source_paid),
                            1 => u64::from(book.original_paid) + u64::from(book.fresh_paid),
                            _ => 0,
                        }
                    );
                    assert_eq!(
                        p.last_fee_slot.get(),
                        if actor < 2 && book.charged[actor] {
                            NOW
                        } else {
                            BIRTHS[actor]
                        }
                    );
                    assert_eq!(env.token_amount(tokens[actor]), paid[actor]);
                    assert_eq!(
                        p.capital.get() + u128::from(paid[actor]),
                        u128::from(INITIAL[actor])
                            + if actor < 2 {
                                u128::from(rewards[1 - actor])
                            } else {
                                0
                            }
                            - if actor < 2 {
                                u128::from(fees[actor])
                            } else {
                                0
                            },
                        "each owner's entitlement, including the returned reward"
                    );
                }
                let retained = [0, 1].map(|i| fees[i] - rewards[i]);
                let custody = supply - paid.iter().sum::<u64>();
                let group = env.market_state().1;
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(group.materialized_portfolio_count, 3);
                assert_eq!(group.c_tot, u128::from(capital.iter().sum::<u64>()));
                assert_eq!(group.insurance, u128::from(retained.iter().sum::<u64>()));
                assert_eq!(group.vault, u128::from(custody));
                assert_eq!(group.vault, group.c_tot + group.insurance);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
                    let expected = retained
                        .iter()
                        .map(|fee| match domain {
                            0 => fee / 2,
                            1 => fee - fee / 2,
                            _ => 0,
                        })
                        .sum::<u64>();
                    assert_eq!(*budget, u128::from(expected));
                }
                assert_eq!(env.token_amount(env.vault), custody);
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                assert_eq!(env.control_sequences(0), controls);
                assert_domain_budget_remaining_total_consistent(&group, &label);
                assert_market_stock_census(
                    &label,
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    u128::from(custody),
                )
                .unwrap();
                assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                for (key, account) in &untouched {
                    assert_eq!(env.svm.get_account(key), *account, "untouched {key}");
                }
            };
            let withdrawal = |actor: usize, sequence, amount: u64| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::Withdraw {
                    portfolio_id: ids[actor],
                    expected_sequence: sequence,
                    amount: amount.into(),
                }
                .encode(),
            };
            let original = withdrawal(1, 1, INITIAL[1]);
            let fresh = withdrawal(1, 2, fresh_amount);
            let source_exit = withdrawal(0, 1, source_amount);
            let fees = [0, 1].map(|actor| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(portfolios[1 - actor], false),
                ],
                data: ProgInstruction::SyncMaintenanceFee { now_slot: NOW }.encode(),
            });
            let fail = spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[1],
                &env.vault,
                &owners[1].pubkey(),
                &[],
                u64::MAX,
            )
            .unwrap();
            use Action::*;
            // Every delivery is signed at slot 3, before the original pays. The
            // only subsequent Clock change is to slot 4; no request is rebound.
            let mut steps = vec![
                (vec![Original, Fail], Some(1)),
                (vec![Original], None),
                (vec![ForwardFee, ReturnFee, Original], Some(2)),
                (vec![ForwardFee, ReturnFee, Fresh, Original], Some(3)),
                (vec![ForwardFee, ReturnFee, Fresh, Fail], Some(3)),
            ];
            if split {
                steps.extend([
                    (vec![ForwardFee], None),
                    (vec![ReturnFee, Original], Some(1)),
                    (vec![ReturnFee], None),
                ]);
            } else {
                steps.push((vec![ForwardFee, ReturnFee], None));
            }
            steps.extend([
                (vec![Original], Some(0)),
                (vec![Fresh, Original], Some(1)),
                (vec![Fresh], None),
                (vec![ForwardFee, ReturnFee], None),
                (vec![Original], Some(0)),
                (vec![Fresh], Some(0)),
                (vec![SourceExit], None),
            ]);
            let deliveries: Vec<_> = steps
                .iter()
                .enumerate()
                .map(|(nonce, (actions, _))| {
                    let ixs: Vec<_> = actions
                        .iter()
                        .map(|action| match action {
                            Original => original.clone(),
                            ForwardFee => fees[0].clone(),
                            ReturnFee => fees[1].clone(),
                            Fresh => fresh.clone(),
                            SourceExit => source_exit.clone(),
                            Fail => fail.clone(),
                        })
                        .collect();
                    let owner = if actions.iter().any(|action| matches!(action, SourceExit)) {
                        &owners[0]
                    } else {
                        &owners[1]
                    };
                    signed(&env, owner, &ixs, 1 + nonce as u32)
                })
                .collect();
            assert_eq!(
                deliveries
                    .iter()
                    .map(|tx| tx.signatures[0])
                    .collect::<BTreeSet<_>>()
                    .len(),
                steps.len()
            );
            let serialized: Vec<_> = deliveries
                .iter()
                .map(|tx| bincode::serialize(tx).unwrap())
                .collect();
            let mut book = Book::default();
            check(&env, &book);
            for (index, ((actions, failing), tx)) in steps.iter().zip(&deliveries).enumerate() {
                if index == 2 {
                    env.svm.warp_to_slot(NOW);
                    check(&env, &book);
                }
                assert_eq!(bincode::serialize(tx).unwrap(), serialized[index]);
                let custody: Vec<_> = tokens
                    .into_iter()
                    .chain([env.vault, env.mint])
                    .map(|key| (key, env.svm.get_account(&key)))
                    .collect();
                let expected = failing.map(|prefix| {
                    let late = matches!(actions[prefix], Fail);
                    let payouts = actions[..prefix]
                        .iter()
                        .filter(|action| matches!(action, Original | Fresh | SourceExit))
                        .count();
                    rolled_back_payouts += payouts;
                    if late {
                        evidence.late_failures += 1;
                    } else {
                        evidence.stale += 1;
                    }
                    (
                        prefix,
                        InstructionError::Custom(if late {
                            spl_token::error::TokenError::InsufficientFunds as u32
                        } else {
                            PercolatorError::EngineStale as u32
                        }),
                        [prefix, payouts],
                    )
                });
                checked_send(&mut env, tx.clone(), &frame, expected, &mut evidence);
                if failing.is_none() {
                    for action in actions {
                        match action {
                            Original => {
                                assert!(!book.original_paid);
                                book.original_paid = true;
                            }
                            ForwardFee => book.charged[0] = true,
                            ReturnFee => {
                                assert!(book.charged[0]);
                                book.charged[1] = true;
                            }
                            Fresh => {
                                assert!(book.original_paid && book.charged == [true; 2]);
                                assert!(!book.fresh_paid);
                                book.fresh_paid = true;
                            }
                            SourceExit => {
                                assert!(book.charged == [true; 2] && !book.source_paid);
                                book.source_paid = true;
                            }
                            Fail => unreachable!(),
                        }
                    }
                    if actions
                        .iter()
                        .all(|action| matches!(action, ForwardFee | ReturnFee))
                    {
                        assert_eq!(tx.message.header.num_required_signatures, 1);
                        for (key, account) in custody {
                            assert_eq!(env.svm.get_account(&key), account, "fee-only custody");
                        }
                    }
                }
                check(&env, &book);
            }
            assert!(book.original_paid && book.fresh_paid && book.source_paid);
            assert_eq!(env.portfolio_state(portfolios[0]).capital.get(), 0);
            assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), 0);
        }
    }
    assert_eq!(evidence.transactions, 56);
    assert_eq!(evidence.stale, 26);
    assert_eq!(evidence.late_failures, 8);
    assert_eq!(rolled_back_payouts, 16);
    eprintln!(
        "INV-008 reciprocal fee stock: 4 worlds, {} transactions, {} exact rollbacks, \
         {rolled_back_payouts} rolled-back SPL payouts, 12 committed payouts; peak {} CU",
        evidence.transactions,
        evidence.stale + evidence.late_failures,
        evidence.max_cu
    );
}
