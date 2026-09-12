//! INV-005/020/024/027/055/081: funded beneficiary consent across stale resolution.
//! The insurer changes; oracle/operator/backing keys and the authenticated deadline do not.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [307, 503];
const INSURANCE: u64 = 71;
const BACKING: u64 = 113;
const OPERATOR_PAID: u64 = 13;
const PRICE: u64 = 100;
const OBSERVED_SLOT: u64 = 3;
const STALE_SLOTS: u64 = 10;
const DEADLINE: u64 = OBSERVED_SLOT + STALE_SLOTS;
const FORCE_CLOSE_DELAY: u64 = 2;

fn instruction(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn submit(
    env: &mut V16CuEnv,
    protected: &[Pubkey],
    instructions: &[Instruction],
    signers: &[&Keypair],
    changed: &[Pubkey],
    error: Option<(u8, PercolatorError)>,
) {
    env.svm.expire_blockhash();
    let before: Vec<_> = protected
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect();
    let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let mut all_instructions = vec![heap_ix(), cu_ix()];
    all_instructions.extend_from_slice(instructions);
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &all_instructions,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    payer_before.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    if let Some((index, expected)) = error {
        let failure = result.expect_err("public conformance denial must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index + 2,
                InstructionError::Custom(expected as u32)
            ),
            "unexpected denial: {failure:?}",
        );
        for (key, account) in protected.iter().zip(before) {
            assert_eq!(
                env.svm.get_account(key),
                account,
                "complete rollback: {key}"
            );
        }
    } else {
        result.expect("public conformance continuation must succeed");
        for (key, account) in protected.iter().zip(before) {
            if !changed.contains(key) {
                assert_eq!(
                    env.svm.get_account(key),
                    account,
                    "unrelated Account: {key}"
                );
            }
        }
    }
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer_before));
}

fn assert_value(
    env: &V16CuEnv,
    wallets: &[Pubkey; 7],
    expected_wallets: [u64; 7],
    portfolios: &[Pubkey; 2],
    expected_capital: [u128; 2],
    insurance: u128,
    backing: u128,
) {
    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.c_tot, expected_capital.iter().sum::<u128>());
    assert_eq!(group.insurance, insurance);
    assert_eq!(group.insurance_domain_budget[0], insurance);
    assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
    let bucket = &group.source_backing_buckets[1];
    assert_eq!(bucket.fresh_unliened_backing_num, backing * BOUND_SCALE);
    assert_eq!(bucket.valid_liened_backing_num, 0);
    assert_eq!(bucket.consumed_liened_backing_num, 0);
    assert_eq!(bucket.impaired_liened_backing_num, 0);
    assert_eq!(bucket.utilization_fee_earnings, 0);
    assert_eq!(group.vault, group.c_tot + insurance + backing);
    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
    assert_eq!(wallets.map(|key| env.token_amount(key)), expected_wallets);
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(
        mint.supply,
        CAPITAL.iter().sum::<u64>() + INSURANCE + BACKING
    );
    assert_eq!(
        mint.supply,
        env.token_amount(env.vault) + expected_wallets.iter().sum::<u64>()
    );
    let (_, view) = state::market_view_mut(&mut market_data).unwrap();
    view.validate_shape().unwrap();
    for (index, portfolio) in portfolios.iter().enumerate() {
        let Some(stored) = env
            .svm
            .get_account(portfolio)
            .filter(|a| !a.data.is_empty())
        else {
            assert_eq!(expected_capital[index], 0);
            continue;
        };
        let mut data = stored.data;
        let account = env.portfolio_state(*portfolio);
        assert_eq!(account.capital.get(), expected_capital[index]);
        assert_eq!(account.pnl.get(), 0);
        state::portfolio_view_mut_for_market_slots(&mut data, 1)
            .unwrap()
            .validate_with_market(&view.as_view())
            .unwrap();
    }
}

#[test]
fn v16_program_funded_insurer_handoff_preserves_stale_deadline_and_permissionless_user_exit() {
    for handoff_at_deadline in [false, true] {
        for reverse_users in [false, true] {
            let mut env = inv018_public_spl_market_with_params(6, V16CuMarketParams::default());
            let admin = env.admin.insecure_clone();
            let incumbent = Keypair::new();
            let successor = Keypair::new();
            let operator = Keypair::new();
            let provider = Keypair::new();
            let users = [Keypair::new(), Keypair::new()];
            let actors = [
                &incumbent, &successor, &operator, &provider, &users[0], &users[1], &admin,
            ];
            for actor in actors {
                env.svm.airdrop(&actor.pubkey(), 1_000_000).unwrap();
            }
            for (kind, holder) in [
                (processor::ASSET_AUTH_INSURANCE, &incumbent),
                (processor::ASSET_AUTH_ORACLE, &incumbent),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
                (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    0,
                    kind,
                    holder.pubkey().to_bytes(),
                )
                .expect("configure empty roles by public consent");
            }
            env.svm.warp_to_slot(OBSERVED_SLOT);
            env.configure_auth_mark_for_asset_with_authority(0, &incumbent, OBSERVED_SLOT, PRICE);
            env.configure_permissionless_resolve_with_cu(STALE_SLOTS, FORCE_CLOSE_DELAY);
            let wallets = actors.map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            for (index, amount) in [
                (0, INSURANCE),
                (3, BACKING),
                (4, CAPITAL[0]),
                (5, CAPITAL[1]),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallets[index],
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
            let sequences = env.control_sequences(0);
            for (owner, source, ix) in [
                (
                    &incumbent,
                    wallets[0],
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch,
                        intent_id: next_control_sequence(sequences.insurance_top_up),
                        amount: INSURANCE as u128,
                    },
                ),
                (
                    &provider,
                    wallets[3],
                    ProgInstruction::TopUpBackingBucket {
                        domain: 1,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch,
                        intent_id: next_control_sequence(sequences.backing_top_up),
                        amount: BACKING as u128,
                        expiry_slot: 1_000,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                    },
                ),
            ] {
                env.send(
                    ix,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[owner],
                )
                .unwrap();
            }
            let portfolios = users.each_ref().map(|owner| {
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
            for index in 0..2 {
                env.send(
                    env.deposit_ix(portfolios[index], CAPITAL[index] as u128),
                    vec![
                        AccountMeta::new(users[index].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[index], false),
                        AccountMeta::new(wallets[4 + index], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&users[index]],
                )
                .unwrap();
            }
            env.trade_asset_with_cu(
                0,
                &users[0],
                portfolios[0],
                &users[1],
                portfolios[1],
                POS_SCALE as i128,
                PRICE,
                0,
            );
            let (_, live) = env.market_state();
            assert_eq!(live.assets[0].oi_eff_long_q, POS_SCALE);
            assert_eq!(live.assets[0].oi_eff_short_q, POS_SCALE);
            let mut protected = vec![env.market, env.mint, env.vault, env.vault_authority];
            protected.extend(wallets);
            protected.extend(portfolios);
            protected.extend(actors.map(Signer::pubkey));
            let market = env.market;
            let vault = env.vault;
            let payout = |env: &V16CuEnv,
                          owner: &Keypair,
                          destination: Pubkey,
                          amount: u128,
                          insurance: bool| {
                let ix = if insurance {
                    env.withdraw_insurance_asset_instruction(owner.pubkey(), 0, amount)
                } else {
                    ProgInstruction::WithdrawBackingBucket {
                        domain: 1,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        amount,
                    }
                };
                instruction(
                    env,
                    ix,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let handoff = |env: &V16CuEnv, current: &Keypair, incoming_signed: bool| {
                instruction(
                    env,
                    ProgInstruction::UpdateAssetAuthority {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        kind: processor::ASSET_AUTH_INSURANCE,
                        new_pubkey: successor.pubkey().to_bytes(),
                    },
                    vec![
                        AccountMeta::new(current.pubkey(), true),
                        AccountMeta::new(successor.pubkey(), incoming_signed),
                        AccountMeta::new(market, false),
                    ],
                )
            };
            let resolve = |env: &V16CuEnv, caller_slot| {
                instruction(
                    env,
                    ProgInstruction::ResolveStalePermissionless {
                        now_slot: caller_slot,
                    },
                    vec![AccountMeta::new(market, false)],
                )
            };
            let close = |env: &V16CuEnv, index: usize, destination: Pubkey| {
                instruction(
                    env,
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(users[index].pubkey(), false),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolios[index], false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let mut expected_wallets = [0; 7];
            let mut expected_capital = CAPITAL.map(u128::from);
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                INSURANCE as u128,
                BACKING as u128,
            );
            let ix = payout(&env, &operator, wallets[2], OPERATOR_PAID as u128, true);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&operator],
                &[market, vault, wallets[2]],
                None,
            );
            expected_wallets[2] = OPERATOR_PAID;
            let remaining_insurance = (INSURANCE - OPERATOR_PAID) as u128;
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                remaining_insurance,
                BACKING as u128,
            );

            env.svm.warp_to_slot(DEADLINE - 1);
            // An authenticated immature deadline rolls back a fully signed funded handoff.
            let bundle = [handoff(&env, &incumbent, true), resolve(&env, u64::MAX)];
            submit(
                &mut env,
                &protected,
                &bundle,
                &[&incumbent, &successor],
                &[],
                Some((1, PercolatorError::OracleStale)),
            );
            if handoff_at_deadline {
                env.svm.warp_to_slot(DEADLINE);
            }
            for (current, incoming_signed, expected) in [
                (&admin, true, PercolatorError::EngineLockActive),
                (&operator, true, PercolatorError::Unauthorized),
                (&incumbent, false, PercolatorError::ExpectedSigner),
            ] {
                let ix = handoff(&env, current, incoming_signed);
                let signers = if incoming_signed {
                    vec![current, &successor]
                } else {
                    vec![current]
                };
                submit(
                    &mut env,
                    &protected,
                    &[ix],
                    &signers,
                    &[],
                    Some((0, expected)),
                );
            }
            let old_sequences = env.control_sequences(0);
            let old_profile =
                state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                    .unwrap();
            let old_cfg = env.market_state().0;
            let ix = handoff(&env, &incumbent, true);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&incumbent, &successor],
                &[market],
                None,
            );
            let new_sequences = env.control_sequences(0);
            assert_eq!(
                new_sequences.authority_epoch,
                old_sequences.authority_epoch + 1
            );
            assert_eq!(
                new_sequences.oracle_observation,
                old_sequences.oracle_observation
            );
            let mut expected_profile = old_profile;
            expected_profile.insurance_authority = successor.pubkey().to_bytes();
            assert_eq!(
                state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                    .unwrap(),
                expected_profile
            );
            assert_eq!(env.market_state().0, old_cfg);
            assert_eq!(old_cfg.last_good_oracle_slot, OBSERVED_SLOT);
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                remaining_insurance,
                BACKING as u128,
            );

            for (signer, epoch, expected) in [
                (
                    &successor,
                    new_sequences.authority_epoch,
                    PercolatorError::Unauthorized,
                ),
                (
                    &incumbent,
                    old_sequences.authority_epoch,
                    PercolatorError::EngineStale,
                ),
            ] {
                let ix = instruction(
                    &env,
                    ProgInstruction::PushAuthMark {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        now_slot: u64::MAX,
                        mark_e6: PRICE,
                        observation_sequence: next_control_sequence(
                            old_sequences.oracle_observation,
                        ),
                        authority_epoch: epoch,
                    },
                    vec![
                        AccountMeta::new(signer.pubkey(), true),
                        AccountMeta::new(market, false),
                    ],
                );
                submit(
                    &mut env,
                    &protected,
                    &[ix],
                    &[signer],
                    &[],
                    Some((0, expected)),
                );
            }
            env.svm.warp_to_slot(DEADLINE);
            // Resolution is public, but a beneficiary still cannot precede materialized users.
            let bundle = [
                resolve(&env, 0),
                payout(&env, &successor, wallets[1], remaining_insurance, true),
            ];
            submit(
                &mut env,
                &protected,
                &bundle,
                &[&successor],
                &[],
                Some((1, PercolatorError::EngineLockActive)),
            );
            assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
            let ix = resolve(&env, 0);
            submit(&mut env, &protected, &[ix], &[], &[market], None);
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(env.market_state().1.resolved_slot, DEADLINE);
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                remaining_insurance,
                BACKING as u128,
            );

            let trade = instruction(
                &env,
                env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, POS_SCALE as i128, PRICE, 0),
                vec![
                    AccountMeta::new(users[0].pubkey(), true),
                    AccountMeta::new(users[1].pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ],
            );
            submit(
                &mut env,
                &protected,
                &[trade],
                &[&users[0], &users[1]],
                &[],
                Some((0, PercolatorError::EngineLockActive)),
            );
            let order = if reverse_users { [1, 0] } else { [0, 1] };
            env.svm.warp_to_slot(DEADLINE + FORCE_CLOSE_DELAY - 1);
            let ix = close(&env, order[0], wallets[4 + order[0]]);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[],
                &[],
                Some((0, PercolatorError::ExpectedSigner)),
            );
            env.svm.warp_to_slot(DEADLINE + FORCE_CLOSE_DELAY);
            let bundle = [
                close(&env, order[0], wallets[4 + order[0]]),
                payout(&env, &successor, wallets[1], remaining_insurance, true),
            ];
            submit(
                &mut env,
                &protected,
                &bundle,
                &[&successor],
                &[],
                Some((1, PercolatorError::EngineLockActive)),
            );
            for index in order {
                let ix = close(&env, index, wallets[4 + index]);
                submit(
                    &mut env,
                    &protected,
                    &[ix],
                    &[],
                    &[market, vault, portfolios[index], wallets[4 + index]],
                    None,
                );
                expected_wallets[4 + index] = CAPITAL[index];
                expected_capital[index] = 0;
                assert_eq!(
                    env.portfolio_state(portfolios[index]).owner,
                    users[index].pubkey().to_bytes()
                );
                assert_value(
                    &env,
                    &wallets,
                    expected_wallets,
                    &portfolios,
                    expected_capital,
                    remaining_insurance,
                    BACKING as u128,
                );
            }
            assert_eq!(env.market_state().1.materialized_portfolio_count, 2);
            let deregister = |env: &V16CuEnv, signer: &Keypair, index: usize| {
                instruction(
                    env,
                    env.close_portfolio_ix(portfolios[index]),
                    vec![
                        AccountMeta::new(signer.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolios[index], false),
                    ],
                )
            };
            let ix = deregister(&env, &successor, order[0]);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&successor],
                &[],
                Some((0, PercolatorError::Unauthorized)),
            );
            let bundle = [
                deregister(&env, &admin, order[0]),
                payout(&env, &successor, wallets[1], remaining_insurance, true),
            ];
            submit(
                &mut env,
                &protected,
                &bundle,
                &[&admin, &successor],
                &[],
                Some((1, PercolatorError::EngineLockActive)),
            );
            for (closed_count, index) in order.into_iter().enumerate() {
                let slab_lamports = env.svm.get_account(&market).unwrap().lamports;
                let portfolio_lamports = env.svm.get_account(&portfolios[index]).unwrap().lamports;
                let ix = deregister(&env, &admin, index);
                submit(
                    &mut env,
                    &protected,
                    &[ix],
                    &[&admin],
                    &[market, portfolios[index]],
                    None,
                );
                assert!(env
                    .svm
                    .get_account(&portfolios[index])
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                assert_eq!(
                    env.svm.get_account(&market).unwrap().lamports,
                    slab_lamports + portfolio_lamports
                );
                assert_eq!(
                    env.market_state().1.materialized_portfolio_count,
                    1 - closed_count as u64
                );
                assert_value(
                    &env,
                    &wallets,
                    expected_wallets,
                    &portfolios,
                    expected_capital,
                    remaining_insurance,
                    BACKING as u128,
                );
            }
            for (signer, destination) in [
                (&incumbent, wallets[0]),
                (&operator, wallets[2]),
                (&admin, wallets[6]),
                (&provider, wallets[3]),
            ] {
                let ix = payout(&env, signer, destination, remaining_insurance, true);
                submit(
                    &mut env,
                    &protected,
                    &[ix],
                    &[signer],
                    &[],
                    Some((0, PercolatorError::Unauthorized)),
                );
            }
            let mut unsigned = payout(&env, &successor, wallets[1], remaining_insurance, true);
            unsigned.accounts[0].is_signer = false;
            submit(
                &mut env,
                &protected,
                &[unsigned],
                &[],
                &[],
                Some((0, PercolatorError::ExpectedSigner)),
            );
            let ix = payout(&env, &successor, wallets[1], remaining_insurance, true);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&successor],
                &[market, vault, wallets[1]],
                None,
            );
            expected_wallets[1] = remaining_insurance as u64;
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                0,
                BACKING as u128,
            );
            let ix = payout(&env, &successor, wallets[1], BACKING as u128, false);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&successor],
                &[],
                Some((0, PercolatorError::Unauthorized)),
            );
            let ix = payout(&env, &provider, wallets[3], BACKING as u128, false);
            submit(
                &mut env,
                &protected,
                &[ix],
                &[&provider],
                &[market, vault, wallets[3]],
                None,
            );
            expected_wallets[3] = BACKING;
            assert_value(
                &env,
                &wallets,
                expected_wallets,
                &portfolios,
                expected_capital,
                0,
                0,
            );
            assert_eq!(expected_wallets, [0, 58, 13, 113, 307, 503, 0]);
        }
    }
}
