//! INV-024 / row 410: merging market authority with a funded role does not merge
//! terminal entitlements. A handoff and both reserve payouts share one transaction;
//! payer identity changes signature privileges, not the recipient of either stock.
//! This is resolved fresh principal, not the live shutdown-beneficiary fallback.
//! The close-order variant distinguishes both reserve claims from raw SPL surplus
//! and rent, including when a reserve payout and final sweep share a destination.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_terminal_role_handoff_preserves_reserve_beneficiaries_with_aliased_payer() {
    terminal_role_handoff(false);
}

#[test]
fn v16_program_terminal_role_handoff_close_order_preserves_reserves_and_surplus() {
    terminal_role_handoff(true);
}

fn terminal_role_handoff(close_slab: bool) {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const BACKING: u64 = 41;
    const INSURANCE: u64 = 59;
    const PEER_INSURANCE: u64 = 23;
    const CAPITAL: u64 = 31;
    let surplus = if close_slab { 13 } else { 0 };
    let supply = BACKING + INSURANCE + PEER_INSURANCE + CAPITAL + surplus;
    let mut peak_cu = [0; 2]; // rejected batch, successful batch
    let mut attempts = [0; 2];
    let mut worlds = 0;
    for asset in [0u16, 1] {
        for incoming_role in 0..3 {
            for aliased_payer in [false, true] {
                let label =
                    format!("asset={asset}, incoming={incoming_role}, payer_alias={aliased_payer}, close_slab={close_slab}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let setup_payer = env.payer.insecure_clone();
                let roles = [Keypair::new(), Keypair::new(), Keypair::new()];
                let owner = Keypair::new();
                for signer in roles.iter().chain([&owner]) {
                    env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
                }
                // Both assets have independent funded roles before the market-key handoff.
                for configured_asset in [0, 1] {
                    for (role, kind) in [
                        processor::ASSET_AUTH_BACKING_BUCKET,
                        processor::ASSET_AUTH_INSURANCE,
                        processor::ASSET_AUTH_INSURANCE_OPERATOR,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        env.send(
                            ProgInstruction::UpdateAssetAuthority {
                                asset_index: configured_asset,
                                market_id: env.asset_market_id(configured_asset),
                                authority_epoch: env
                                    .control_sequences(configured_asset as usize)
                                    .authority_epoch,
                                kind,
                                new_pubkey: roles[role].pubkey().to_bytes(),
                            },
                            vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new_readonly(roles[role].pubkey(), true),
                                AccountMeta::new(env.market, false),
                            ],
                            &[&admin, &roles[role]],
                        )
                        .unwrap();
                    }
                }
                let wallets = [
                    roles[0].pubkey(),
                    roles[1].pubkey(),
                    roles[2].pubkey(),
                    owner.pubkey(),
                    admin.pubkey(),
                    setup_payer.pubkey(),
                ];
                let tokens = wallets.map(|wallet| {
                    create_ata_for_test(&mut env.svm, &setup_payer, wallet, env.mint)
                });
                for (token, amount) in tokens.into_iter().zip([
                    BACKING,
                    INSURANCE + PEER_INSURANCE,
                    0,
                    CAPITAL,
                    surplus,
                    0,
                ]) {
                    if amount != 0 {
                        send_raw_tx(
                            &mut env.svm,
                            &setup_payer,
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
                }
                send_raw_tx(
                    &mut env.svm,
                    &setup_payer,
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
                let mint_before = env.svm.get_account(&env.mint).unwrap();
                let mint = Mint::unpack(&mint_before.data).unwrap();
                assert_eq!(mint.supply, supply);
                assert_eq!(mint.mint_authority, COption::None);

                let portfolio_key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &setup_payer,
                    &portfolio_key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                let portfolio = portfolio_key.pubkey();
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[&owner],
                )
                .unwrap();
                env.portfolios.push(portfolio);
                env.send(
                    env.deposit_ix(portfolio, CAPITAL.into()),
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(tokens[3], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owner],
                )
                .unwrap();
                env.svm.warp_to_slot(1);
                let domain = 2 * asset + 1;
                let peer_domain = 2 * (1 - asset);
                let funding_accounts = |role: usize| {
                    vec![
                        AccountMeta::new(roles[role].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[role], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ]
                };
                let backing_accounts = funding_accounts(0);
                let insurance_accounts = funding_accounts(1);
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                        intent_id: 0,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: BACKING.into(),
                        expiry_slot: 100,
                    },
                    backing_accounts,
                    &[&roles[0]],
                )
                .unwrap();
                for (funded_domain, amount) in
                    [(2 * asset, INSURANCE), (peer_domain, PEER_INSURANCE)]
                {
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: funded_domain,
                            market_id: env.asset_market_id(funded_domain / 2),
                            authority_epoch: env
                                .control_sequences((funded_domain / 2) as usize)
                                .authority_epoch,
                            intent_id: 0,
                            amount: amount.into(),
                        },
                        insurance_accounts.clone(),
                        &[&roles[1]],
                    )
                    .unwrap();
                }
                if surplus != 0 {
                    send_raw_tx(
                        &mut env.svm,
                        &setup_payer,
                        spl_token::instruction::transfer(
                            &spl_token::ID,
                            &tokens[4],
                            &env.vault,
                            &admin.pubkey(),
                            &[],
                            surplus,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                }
                assert_eq!(tokens.map(|token| env.token_amount(token)), [0; 6]);
                assert_eq!(env.token_amount(env.vault), supply);
                env.resolve();
                env.send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(owner.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(tokens[3], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                )
                .unwrap();
                assert!(resolved_portfolio_is_terminal(&env, portfolio));
                env.close_portfolio_with_cu(&owner, portfolio);

                let profiles = [0, 1].map(|index| {
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        index,
                    )
                    .unwrap()
                });
                let epochs = [0, 1].map(|index| env.control_sequences(index).authority_epoch);
                let terminal_before = env.market_state().1;
                let stock = |env: &V16CuEnv, paid: bool| {
                    let closed = paid && close_slab;
                    let backing = if paid { 0 } else { BACKING };
                    let insurance = if paid { 0 } else { INSURANCE };
                    let mut expected = [BACKING - backing, INSURANCE - insurance, 0, CAPITAL, 0, 0];
                    if closed {
                        expected[1] += PEER_INSURANCE;
                        expected[incoming_role] += surplus;
                    }
                    for ((key, wallet), amount) in tokens.into_iter().zip(wallets).zip(expected) {
                        let account = env.svm.get_account(&key).unwrap();
                        let token = TokenAccount::unpack(&account.data).unwrap();
                        assert_eq!(account.owner, spl_token::ID);
                        assert_eq!(
                            (token.owner, token.mint, token.amount),
                            (wallet, env.mint, amount),
                            "{label}: per-wallet attribution"
                        );
                    }
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                    if closed {
                        assert_eq!(expected.iter().sum::<u64>(), supply);
                        let tombstone = env.svm.get_account(&env.market).unwrap();
                        assert_closed_market_tombstone(&tombstone);
                        assert_eq!(
                            tombstone.lamports,
                            env.svm.minimum_balance_for_rent_exemption(
                                percolator_prog::constants::HEADER_LEN
                            )
                        );
                        assert!(env.svm.get_account(&env.vault).is_none_or(|account| {
                            account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                        }));
                        return;
                    }
                    let group = env.market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(
                        (
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(
                        group.vault,
                        u128::from(backing + insurance + PEER_INSURANCE)
                    );
                    assert_eq!(group.insurance, u128::from(insurance + PEER_INSURANCE));
                    assert_eq!(
                        env.token_amount(env.vault),
                        backing + insurance + PEER_INSURANCE + surplus
                    );
                    assert_eq!(
                        expected.iter().sum::<u64>() + env.token_amount(env.vault),
                        supply
                    );
                    for d in 0..4 {
                        let expected_insurance = if d == 2 * asset as usize {
                            insurance
                        } else if d == peer_domain as usize {
                            PEER_INSURANCE
                        } else {
                            0
                        };
                        assert_eq!(
                            group.insurance_domain_budget[d],
                            u128::from(expected_insurance)
                        );
                        assert_eq!(group.insurance_domain_spent[d], 0);
                        let expected_backing = if d == domain as usize { backing } else { 0 };
                        assert_eq!(
                            group.source_backing_buckets[d].fresh_unliened_backing_num,
                            u128::from(expected_backing) * BOUND_SCALE
                        );
                        assert_eq!(
                            group.source_credit[d].fresh_reserved_backing_num,
                            u128::from(expected_backing) * BOUND_SCALE
                        );
                        if d != domain as usize {
                            assert_eq!(group.source_credit[d], terminal_before.source_credit[d]);
                            assert_eq!(
                                group.source_backing_buckets[d],
                                terminal_before.source_backing_buckets[d]
                            );
                        }
                    }
                    assert_domain_budget_remaining_total_consistent(&group, &label);
                };
                stock(&env, false);

                let incoming = &roles[incoming_role];
                let payer = if aliased_payer {
                    incoming
                } else {
                    &setup_payer
                };
                let wrap = |ix: ProgInstruction, accounts| Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: ix.encode(),
                };
                let handoff = wrap(
                    ProgInstruction::UpdateAuthority {
                        authority_epoch: epochs[0],
                        new_pubkey: incoming.pubkey().to_bytes(),
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new_readonly(incoming.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                );
                // Raw batches use the post-handoff epoch, without helper rebinding.
                let epoch = epochs[asset as usize] + u64::from(asset == 0);
                let withdrawal = |role: usize| {
                    wrap(
                        if role == 0 {
                            ProgInstruction::WithdrawBackingBucket {
                                domain,
                                market_id: env.asset_market_id(asset),
                                authority_epoch: epoch,
                                amount: BACKING.into(),
                            }
                        } else {
                            ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: asset,
                                market_id: env.asset_market_id(asset),
                                authority_epoch: epoch,
                                amount: INSURANCE.into(),
                            }
                        },
                        vec![
                            AccountMeta::new(roles[role].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(tokens[role], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    )
                };
                let first = usize::from(incoming_role == 1);
                let close = wrap(
                    ProgInstruction::CloseSlab {
                        authority_epoch: epochs[0] + 1,
                    },
                    vec![
                        AccountMeta::new(incoming.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(tokens[incoming_role], false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                );
                let mut valid = vec![handoff, withdrawal(first), withdrawal(1 - first)];
                if close_slab {
                    let peer_asset = peer_domain / 2;
                    valid.push(wrap(
                        ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: peer_asset,
                            market_id: env.asset_market_id(peer_asset),
                            authority_epoch: epochs[peer_asset as usize]
                                + u64::from(peer_asset == 0),
                            amount: PEER_INSURANCE.into(),
                        },
                        vec![
                            AccountMeta::new(roles[1].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(tokens[1], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    ));
                    valid.push(close.clone());
                }
                let mut wrong_recipient = valid.clone();
                let (wrong_ix, destination) = if close_slab { (4, 4) } else { (2, 2) };
                wrong_recipient[wrong_ix].accounts[destination].pubkey = if close_slab {
                    tokens[(incoming_role + 1) % 3]
                } else {
                    tokens[incoming_role]
                };
                assert_ne!(
                    wrong_recipient[wrong_ix].accounts[destination].pubkey,
                    valid[wrong_ix].accounts[destination].pubkey
                );
                let mut frame_keys = vec![env.market, env.vault, env.mint, portfolio];
                frame_keys.extend(tokens);
                frame_keys.extend(wallets);
                let frame = |env: &V16CuEnv| {
                    frame_keys
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let sign = |env: &V16CuEnv, instructions: &[Instruction]| {
                    let mut batch = vec![
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(500_000),
                    ];
                    batch.extend_from_slice(instructions);
                    let mut signers = vec![payer, &admin, &roles[0], &roles[1], incoming];
                    signers.retain(|signer| {
                        signer.pubkey() == payer.pubkey()
                            || instructions.iter().any(|ix| {
                                ix.accounts
                                    .iter()
                                    .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                            })
                    });
                    signers.sort_by_key(|key| key.pubkey());
                    signers.dedup_by_key(|key| key.pubkey());
                    Transaction::new_signed_with_payer(
                        &batch,
                        Some(&payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    )
                };
                let before_simulation = frame(&env);
                env.svm
                    .simulate_transaction(sign(&env, &valid).into())
                    .expect("owner-attributed terminal batch is jointly payable");
                assert_eq!(frame(&env), before_simulation);
                let mut batches = Vec::new();
                if close_slab {
                    // A valid reserve payout cannot authorize final disposal of
                    // the other holder's remaining claim. Allow bounded scan
                    // progress across two assets before requiring the stock gate.
                    batches.push((
                        Some((4..=6, PercolatorError::EngineLockActive, 1)),
                        vec![
                            valid[0].clone(),
                            valid[1].clone(),
                            close.clone(),
                            close.clone(),
                            close,
                        ],
                    ));
                }
                batches.push((
                    Some((
                        (wrong_ix + 2) as u8..=(wrong_ix + 2) as u8,
                        PercolatorError::InvalidTokenAccount,
                        if close_slab { 3 } else { 1 },
                    )),
                    wrong_recipient,
                ));
                batches.push((None, valid));
                let mut changed = vec![env.market, env.vault, tokens[0], tokens[1]];
                if close_slab {
                    changed.push(tokens[incoming_role]);
                }
                for (rejection, instructions) in batches {
                    let failed = rejection.is_some();
                    env.svm.expire_blockhash();
                    let tx = sign(&env, &instructions);
                    let fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let before = frame(&env);
                    let rent_refund = env.svm.get_account(&env.market).unwrap().lamports
                        + env.svm.get_account(&env.vault).unwrap().lamports
                        - env.svm.minimum_balance_for_rent_exemption(
                            percolator_prog::constants::HEADER_LEN,
                        );
                    let result = env.svm.send_transaction(tx);
                    let (meta, token_calls) = if let Some((indices, error, transfers)) = rejection {
                        let failure = result.expect_err(
                            "terminal roles must preserve stock ownership and close ordering",
                        );
                        let TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(code),
                        ) = failure.err
                        else {
                            panic!("{label}: unexpected failure {:?}", failure.err);
                        };
                        assert_eq!(code, error as u32, "{label}");
                        assert!(indices.contains(&index), "{label}: failure index {index}");
                        assert_eq!(
                            failure
                                .meta
                                .logs
                                .iter()
                                .filter(|line| {
                                    **line == format!("Program {} success", env.program_id)
                                })
                                .count(),
                            usize::from(index - 2),
                            "{label}: successful wrapper prefix before stock/destination gate"
                        );
                        (failure.meta, transfers)
                    } else {
                        (
                            result.expect("unchanged owner-attributed terminal batch remains payable after rollback"),
                            if close_slab { 5 } else { 2 },
                        )
                    };
                    assert_eq!(
                        meta.logs
                            .iter()
                            .filter(|line| **line == format!("Program {} success", spl_token::ID))
                            .count(),
                        token_calls,
                        "{label}: nonvacuous payout prefix"
                    );
                    for (key, mut account) in frame_keys.iter().zip(before) {
                        if *key == payer.pubkey() {
                            account.as_mut().unwrap().lamports -= fee;
                        }
                        if !failed && close_slab && *key == incoming.pubkey() {
                            account.as_mut().unwrap().lamports += rent_refund;
                        }
                        if failed || !changed.contains(key) {
                            assert_eq!(
                                env.svm.get_account(key),
                                account,
                                "{label}: exact frame {key}"
                            );
                        }
                    }
                    stock(&env, !failed);
                    if failed || !close_slab {
                        assert_eq!(
                            env.market_state().0.marketauth,
                            if failed {
                                admin.pubkey().to_bytes()
                            } else {
                                incoming.pubkey().to_bytes()
                            }
                        );
                        for index in 0..2 {
                            let mut expected = profiles[index];
                            if !failed && index == 0 {
                                expected.asset_admin = incoming.pubkey().to_bytes();
                                expected.oracle_authority = incoming.pubkey().to_bytes();
                            }
                            assert_eq!(
                                state::read_asset_oracle_profile(
                                    &env.svm.get_account(&env.market).unwrap().data,
                                    index
                                )
                                .unwrap(),
                                expected,
                                "{label}: reserve role identity survives handoff"
                            );
                            assert_eq!(
                                env.control_sequences(index).authority_epoch,
                                epochs[index] + u64::from(!failed && index == 0)
                            );
                        }
                    }
                    let metric = usize::from(!failed);
                    attempts[metric] += 1;
                    peak_cu[metric] = peak_cu[metric].max(meta.compute_units_consumed);
                    assert_cu_within(
                        "INV-024 terminal handoff batch",
                        meta.compute_units_consumed,
                        500_000,
                    );
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 12);
    assert_eq!(attempts, [if close_slab { 24 } else { 12 }, 12]);
    eprintln!("INV-024 terminal role handoff close_slab={close_slab}: {worlds} worlds, 12 payable simulations, attempts [rejection, success]={attempts:?}; peak CU={peak_cu:?}");
}
