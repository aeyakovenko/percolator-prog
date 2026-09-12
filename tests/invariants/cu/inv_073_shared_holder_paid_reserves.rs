//! INV-073/024/080: a reserve holder's paid-and-spent stock is distinct from
//! its outstanding portfolio entitlement and its unpaid reserve claims.
//! Only portfolio disposition is permissionless in this bounded witness.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [52_502, 2_000_000];
const BACKING: u64 = 100_000;
const INSURANCE: u64 = 31;
const RATE: u16 = 3_333;
const PROFIT: u64 = 1_000 * (105 - 100);
const EARNINGS: u64 = ((1_050 * 105 / 2 - CAPITAL[0]) * RATE as u64).div_ceil(10_000);
const PAYOUTS: [u64; 2] = [CAPITAL[0] + PROFIT - EARNINGS, CAPITAL[1] - PROFIT];
const PAID: [u64; 3] = [17, 19, 7];
const SUPPLY: u64 = CAPITAL[0] + CAPITAL[1] + BACKING + INSURANCE;
const CU_LIMIT: u32 = 600_000;

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rejection: Option<(u8, &[PercolatorError])>,
) -> (u64, usize) {
    env.svm.expire_blockhash();
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let signatures = [&env.payer]
        .into_iter()
        .chain(signers.iter().copied())
        .collect::<Vec<_>>();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + signers.len()
    );
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, errors)) = rejection {
        let failure = result.expect_err("the named reserve or portfolio signer remains required");
        let accepted = match failure.err {
            TransactionError::InstructionError(actual, InstructionError::Custom(code))
                if actual == index =>
            {
                errors.iter().any(|error| code == error.clone() as u32)
            }
            _ => false,
        };
        assert!(accepted, "{failure:?}");
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            usize::from(index - 2),
            "all preceding wrapper instructions must complete"
        );
        failure.meta
    } else {
        result.expect("publicly constructed authorized continuation")
    };
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !allowed.contains(&key) {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame {key}"
            );
        }
    }
    assert_cu_within(
        "shared holder paid reserves",
        meta.compute_units_consumed,
        CU_LIMIT.into(),
    );
    let transfers = meta
        .logs
        .iter()
        .filter(|line| **line == format!("Program {} success", spl_token::ID))
        .count();
    (meta.compute_units_consumed, transfers)
}

#[test]
fn v16_program_absent_shared_holders_keep_paid_reserves_separate_from_public_user_exit() {
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut value_prefixes = 0;
    let mut calls = 0;
    for winner_first in [false, true] {
        for crank_alias in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 1,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let holders = [Keypair::new(), Keypair::new()];
            let operator = Keypair::new();
            for signer in [&holders[0], &holders[1], &operator] {
                env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
            }
            for (kind, holder) in [
                (processor::ASSET_AUTH_BACKING_BUCKET, &holders[0]),
                (processor::ASSET_AUTH_INSURANCE, &holders[1]),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    0,
                    kind,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            env.svm.warp_to_slot(1);
            env.configure_permissionless_resolve_with_cu(4, 5);
            env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
            let policy = env.control_sequences(0);
            env.send(
                ProgInstruction::UpdateBackingFeePolicy {
                    market_id: env.asset_market_id(0),
                    domain: 1,
                    fee_bps: RATE,
                    insurance_share_bps: 0,
                    policy_sequence: next_control_sequence(
                        policy.backing_fee.max(policy.authority_epoch),
                    ),
                    authority_epoch: policy.authority_epoch,
                },
                vec![
                    AccountMeta::new(holders[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&holders[1]],
            )
            .unwrap();
            let wallets = [
                holders[0].pubkey(),
                holders[1].pubkey(),
                operator.pubkey(),
                admin.pubkey(),
            ];
            let tokens =
                wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
            for (token, amount) in tokens
                .into_iter()
                .zip([CAPITAL[0] + BACKING, CAPITAL[1] + INSURANCE])
            {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap(),
                    &[&admin],
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
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
            let portfolios = holders.each_ref().map(|owner| {
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[owner],
                )
                .unwrap();
                env.portfolios.push(key.pubkey());
                key.pubkey()
            });
            for i in 0..2 {
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
                    &[&holders[i]],
                )
                .unwrap();
            }
            for (i, ix) in [
                (
                    0,
                    ProgInstruction::TopUpBackingBucket {
                        domain: 1,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        intent_id: 0,
                        backing_fee_bps: RATE,
                        insurance_share_bps: 0,
                        amount: BACKING.into(),
                        expiry_slot: 100,
                    },
                ),
                (
                    1,
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        intent_id: 0,
                        amount: INSURANCE.into(),
                    },
                ),
            ] {
                env.send(
                    ix,
                    vec![
                        AccountMeta::new(wallets[i], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&holders[i]],
                )
                .unwrap();
            }
            env.trade_asset_with_cu(
                0,
                &holders[0],
                portfolios[0],
                &holders[1],
                portfolios[1],
                1_000 * POS_SCALE as i128,
                100,
                0,
            );
            env.svm.warp_to_slot(2);
            env.push_auth_mark_for_asset_as_admin(0, 2, 105);
            for i in [1, 0] {
                env.crank(
                    portfolios[i],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                );
            }
            env.try_trade_asset_with_backing_fee_cap_with_cu(
                0,
                &holders[0],
                portfolios[0],
                &holders[1],
                portfolios[1],
                50 * POS_SCALE as i128,
                105,
                0,
                RATE,
            )
            .unwrap();
            assert_eq!(
                env.market_state().1.backing_provider_earnings_total,
                EARNINGS.into()
            );

            let ledger_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger_key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger_key.pubkey();
            let market = env.market;
            let vault = env.vault;
            let tracked = [env.market, env.vault, env.mint, env.vault_authority, ledger]
                .into_iter()
                .chain(wallets)
                .chain(tokens)
                .chain(portfolios)
                .collect::<Vec<_>>();
            let wrap = |env: &V16CuEnv, data: ProgInstruction, accounts| Instruction {
                program_id: env.program_id,
                accounts,
                data: data.encode(),
            };
            let reserve = |env: &V16CuEnv, kind: usize, actor: usize, amount: u64, signed: bool| {
                let mut accounts = vec![
                    AccountMeta::new(wallets[actor], signed),
                    AccountMeta::new(env.market, false),
                ];
                if kind == 1 {
                    accounts.push(AccountMeta::new(ledger, false));
                }
                accounts.extend([
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]);
                let market_id = env.asset_market_id(0);
                let authority_epoch = env.control_sequences(0).authority_epoch;
                let amount = amount.into();
                let data = match kind {
                    0 => ProgInstruction::WithdrawBackingBucket {
                        domain: 1,
                        market_id,
                        authority_epoch,
                        amount,
                    },
                    1 => ProgInstruction::WithdrawBackingBucketEarnings {
                        domain: 1,
                        market_id,
                        authority_epoch,
                        amount,
                    },
                    2 => ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id,
                        authority_epoch,
                        amount,
                    },
                    _ => unreachable!(),
                };
                wrap(env, data, accounts)
            };
            // The live operator receives its authorized prefix; the terminal beneficiary
            // keeps the remaining insurance claim without inheriting that paid value.
            let mut paid = [0; 3];
            for kind in 0..3 {
                let (actor, signer) = if kind == 2 {
                    (2, &operator)
                } else {
                    (0, &holders[0])
                };
                let ix = reserve(&env, kind, actor, PAID[kind], true);
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[ix],
                        &[signer],
                        &tracked,
                        &[market, vault, tokens[actor], ledger],
                        None,
                    )
                    .0,
                );
                paid[kind] = PAID[kind];
                assert_eq!(
                    tokens.map(|key| env.token_amount(key)),
                    [paid[0] + paid[1], 0, paid[2], 0]
                );
                let group = env.market_state().1;
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - paid[1])
                );
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
                assert_eq!(group.vault, u128::from(SUPPLY - paid.iter().sum::<u64>()));
                assert_eq!(env.token_amount(vault), SUPPLY - paid.iter().sum::<u64>());
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            }
            let spend = spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[0],
                &tokens[2],
                &wallets[0],
                &[],
                PAID[0] + PAID[1],
            )
            .unwrap();
            peak_cu = peak_cu.max(
                land(
                    &mut env,
                    &[spend],
                    &[&holders[0]],
                    &tracked,
                    &[tokens[0], tokens[2]],
                    None,
                )
                .0,
            );
            assert_eq!(
                tokens.map(|key| env.token_amount(key)),
                [0, 0, PAID.iter().sum(), 0]
            );
            let ledger_frame = env.svm.get_account(&ledger).unwrap();
            let record = state::read_backing_domain_ledger(&ledger_frame.data).unwrap();
            assert_eq!(record.authority, wallets[0].to_bytes());
            assert_eq!(record.total_earnings_withdrawn_atoms, PAID[1].into());
            let profiles = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let sequences = env.control_sequences(0);
            let operator_tokens = env.svm.get_account(&tokens[2]);
            let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
            drop(holders);
            drop(operator);
            drop(admin);
            assert!(!wallets.contains(&env.payer.pubkey()));

            let check = |env: &V16CuEnv| {
                let group = env.market_state().1;
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - PAID[1])
                );
                assert_eq!(
                    group.source_backing_buckets[1].utilization_fee_earnings,
                    u128::from(EARNINGS - PAID[1])
                );
                assert_eq!(group.insurance, u128::from(INSURANCE - PAID[2]));
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert!(group
                    .insurance_domain_spent
                    .iter()
                    .all(|amount| *amount == 0));
                assert_domain_budget_remaining_total_consistent(
                    &group,
                    "shared holder paid reserves",
                );
                assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                assert_eq!(
                    env.token_amount(env.vault)
                        + tokens.iter().map(|key| env.token_amount(*key)).sum::<u64>(),
                    SUPPLY
                );
                for i in 0..2 {
                    assert!(env.token_amount(tokens[i]) <= PAYOUTS[i]);
                }
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                assert_eq!(env.svm.get_account(&ledger), Some(ledger_frame.clone()));
                assert_eq!(env.svm.get_account(&tokens[2]), operator_tokens);
                assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profiles
                );
            };
            check(&env);
            env.svm.warp_to_slot(6);
            let resolve = wrap(
                &env,
                ProgInstruction::ResolveStalePermissionless { now_slot: 6 },
                vec![AccountMeta::new(env.market, false)],
            );
            peak_cu = peak_cu.max(land(&mut env, &[resolve], &[], &tracked, &[market], None).0);
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(env.market_state().1.resolved_slot, 6);
            check(&env);
            let payout = |env: &V16CuEnv, i| {
                wrap(
                    env,
                    if crank_alias {
                        ProgInstruction::PermissionlessCrank {
                            now_slot: env.svm.get_sysvar::<Clock>().slot,
                            observations: vec![],
                        }
                    } else {
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                    },
                    vec![
                        AccountMeta::new_readonly(wallets[i], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let unsigned = [
                reserve(&env, 0, 0, 1, false),
                reserve(&env, 1, 0, 1, false),
                reserve(&env, 2, 1, 1, false),
            ];
            env.svm.warp_to_slot(10);
            for i in 0..2 {
                let ix = payout(&env, i);
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &[],
                        Some((
                            2,
                            &[
                                PercolatorError::ExpectedSigner,
                                PercolatorError::EngineLockActive,
                            ],
                        )),
                    )
                    .0,
                );
                rollbacks += 1;
                check(&env);
            }
            env.svm.warp_to_slot(11);
            let rank = |env: &V16CuEnv| {
                let legs: u32 = portfolios
                    .iter()
                    .map(|key| {
                        percolator::active_bitmap_count_ones(active_bitmap(
                            &env.portfolio_state(*key),
                        ))
                    })
                    .sum();
                let unpaid = PAYOUTS.iter().sum::<u64>()
                    - env.token_amount(tokens[0])
                    - env.token_amount(tokens[1]);
                (legs, unpaid)
            };
            let order = if winner_first { [0, 1] } else { [1, 0] };
            let mut world_calls = 0;
            for _ in 0..8 {
                for i in order {
                    if resolved_portfolio_is_terminal(&env, portfolios[i]) {
                        continue;
                    }
                    let ix = payout(&env, i);
                    let before_rank = rank(&env);
                    let before_tokens = env.token_amount(tokens[i]);
                    let mut prefix_transfers = Vec::new();
                    for suffix in &unsigned {
                        let (cu, transfers) = land(
                            &mut env,
                            &[ix.clone(), suffix.clone()],
                            &[],
                            &tracked,
                            &[],
                            Some((
                                3,
                                &[
                                    PercolatorError::ExpectedSigner,
                                    PercolatorError::EngineLockActive,
                                ],
                            )),
                        );
                        peak_cu = peak_cu.max(cu);
                        prefix_transfers.push(transfers);
                        rollbacks += 1;
                        check(&env);
                    }
                    let (cu, transfers) = land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &[market, vault, portfolios[i], tokens[i]],
                        None,
                    );
                    peak_cu = peak_cu.max(cu);
                    assert!(
                        rank(&env) < before_rank,
                        "each committed public call must decrease (legs, unpaid user value)"
                    );
                    assert!(prefix_transfers.iter().all(|count| *count == transfers));
                    if env.token_amount(tokens[i]) > before_tokens {
                        assert_eq!(transfers, 1);
                        value_prefixes += prefix_transfers.len();
                    }
                    world_calls += 1;
                    check(&env);
                }
                if rank(&env) == (0, 0) {
                    break;
                }
            }
            assert!(world_calls <= 16);
            assert_eq!(rank(&env), (0, 0));
            calls += world_calls;
            assert_eq!(
                tokens.map(|key| env.token_amount(key)),
                [PAYOUTS[0], PAYOUTS[1], PAID.iter().sum(), 0]
            );
            let terminal = env.market_state().1;
            assert_eq!(
                (
                    terminal.c_tot,
                    terminal.pnl_pos_tot,
                    terminal.materialized_portfolio_count
                ),
                (0, 0, 2)
            );
            assert_eq!(
                (
                    terminal.assets[0].oi_eff_long_q,
                    terminal.assets[0].oi_eff_short_q
                ),
                (0, 0)
            );
            assert_eq!(
                terminal.source_backing_buckets[1].fresh_unliened_backing_num,
                u128::from(BACKING - PAID[0]) * BOUND_SCALE
            );
            assert_eq!(
                terminal.vault,
                u128::from(BACKING + EARNINGS + INSURANCE - PAID.iter().sum::<u64>())
            );
            for i in 0..2 {
                assert!(resolved_portfolio_is_terminal(&env, portfolios[i]));
                let ix = payout(&env, i);
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &[],
                        Some((
                            2,
                            &[
                                PercolatorError::EngineNonProgress,
                                PercolatorError::EngineLockActive,
                            ],
                        )),
                    )
                    .0,
                );
                rollbacks += 1;
                let close = wrap(
                    &env,
                    env.close_portfolio_ix(portfolios[i]),
                    vec![
                        AccountMeta::new(wallets[i], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                    ],
                );
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[close],
                        &[],
                        &tracked,
                        &[],
                        Some((
                            2,
                            &[
                                PercolatorError::ExpectedSigner,
                                PercolatorError::EngineLockActive,
                            ],
                        )),
                    )
                    .0,
                );
                rollbacks += 1;
            }
            for suffix in &unsigned {
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[suffix.clone()],
                        &[],
                        &tracked,
                        &[],
                        Some((
                            2,
                            &[
                                PercolatorError::ExpectedSigner,
                                PercolatorError::EngineLockActive,
                            ],
                        )),
                    )
                    .0,
                );
                rollbacks += 1;
            }
            check(&env);
            eprintln!("shared holder paid reserves: winner_first={winner_first}, crank_alias={crank_alias}, public_calls={world_calls}, user_payouts={PAYOUTS:?}, surviving_reserves={}", terminal.vault);
        }
    }
    assert_eq!(value_prefixes, 24);
    eprintln!("shared holder paid reserves: worlds=4, public_calls={calls}, exact_rollbacks={rollbacks}, value_moving_rollback_prefixes={value_prefixes}, peak_cu={peak_cu}");
}
