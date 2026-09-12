//! Terminal backing and insurance have separate recipients and finite allowances even when
//! the vault remains liquid. All state and collateral are created through System/SPL/wrapper
//! instructions; retries retain the original wire bytes and use fresh transaction blockhashes.
//! The absent-provider suffix reverses user order and leaves replenished principal attributed
//! while the current insurance beneficiary exits without the provider or insurance operator.

use super::*;

const CAPITAL: u64 = 1_000;
const BACKING: u64 = 401;
const INSURANCE: u64 = 204;
const UNRELATED_INSURANCE: u64 = 1_000;
const FUNDED: u64 = 2 * CAPITAL + BACKING + INSURANCE + UNRELATED_INSURANCE;

#[test]
fn v16_program_terminal_provider_and_insurance_retries_preserve_separate_entitlements() {
    run_terminal_provider_and_insurance(false);
}

#[test]
fn v16_program_absent_provider_preserves_user_order_and_operator_free_insurance_exit() {
    run_terminal_provider_and_insurance(true);
}

fn run_terminal_provider_and_insurance(absent_provider: bool) {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const FIRST: [u64; 2] = [251, 151];
    const REMAINDER: [u64; 2] = [BACKING - FIRST[0], INSURANCE - FIRST[1]];

    let mut peak_cu = 0;
    for insurance_first in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        let owners = [Keypair::new(), Keypair::new()];
        let provider = Keypair::new();
        let insurer = Keypair::new();
        let operator = Keypair::new();
        let admin = env.admin.insecure_clone();
        for owner in [&owners[0], &owners[1], &provider, &insurer, &operator] {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        }
        for (kind, incoming) in [
            (processor::ASSET_AUTH_INSURANCE, &insurer),
            (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
        ] {
            env.send(
                ProgInstruction::UpdateAssetAuthority {
                    asset_index: 1,
                    market_id: env.asset_market_id(1),
                    authority_epoch: env.control_sequences(1).authority_epoch,
                    kind,
                    new_pubkey: incoming.pubkey().to_bytes(),
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new_readonly(incoming.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&admin, incoming],
            )
            .expect("consensual public handoff of the active asset's payout roles");
        }
        env.svm.warp_to_slot(1);
        env.configure_permissionless_resolve_with_cu(100, 5);
        for asset in [0, 1] {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }

        let recipients = [&owners[0], &owners[1], &provider, &insurer, &admin];
        let tokens = recipients
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        for (token, amount) in
            tokens
                .into_iter()
                .zip([CAPITAL, CAPITAL, BACKING, INSURANCE, UNRELATED_INSURANCE])
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

        let portfolios = owners.each_ref().map(|owner| {
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
                env.deposit_ix(portfolios[index], u128::from(CAPITAL)),
                vec![
                    AccountMeta::new(owners[index].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[index], false),
                    AccountMeta::new(tokens[index], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[index]],
            )
            .unwrap();
        }
        for (domain, amount, authority, token) in [
            (0, UNRELATED_INSURANCE, &admin, tokens[4]),
            (2, 101, &insurer, tokens[3]),
            (3, 103, &insurer, tokens[3]),
        ] {
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain,
                    market_id: env.asset_market_id(domain / 2),
                    authority_epoch: 0,
                    intent_id: 0,
                    amount: u128::from(amount),
                },
                vec![
                    AccountMeta::new(authority.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[authority],
            )
            .unwrap();
        }
        env.send(
            ProgInstruction::TopUpBackingBucket {
                domain: 3,
                market_id: env.asset_market_id(1),
                authority_epoch: 0,
                intent_id: 0,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount: u128::from(BACKING),
                expiry_slot: 1_000,
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&provider],
        )
        .unwrap();
        assert_eq!(tokens.map(|token| env.token_amount(token)), [0; 5]);
        assert_eq!(env.token_amount(env.vault), FUNDED);

        env.trade_asset_with_cu(
            1,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            20 * POS_SCALE as i128,
            100,
            0,
        );
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(1, 2, 105);
        for portfolio in portfolios {
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(1),
                },
            );
        }
        assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 100);
        assert!(env.market_state().1.source_credit[3].positive_claim_bound_num > 0);
        env.resolve();

        if absent_provider {
            let absent = [provider.pubkey(), operator.pubkey()];
            drop(provider);
            drop(operator);
            verify_absent_provider_suffix(
                &mut env,
                &owners,
                &insurer,
                absent,
                tokens,
                insurance_first,
            );
            continue;
        }

        let mut frame_keys = vec![env.market, env.vault, env.mint, operator.pubkey()];
        frame_keys.extend(portfolios);
        frame_keys.extend(tokens);
        frame_keys.extend(recipients.map(|owner| owner.pubkey()));
        let frame = |env: &V16CuEnv| {
            frame_keys
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>()
        };
        let custody = |env: &V16CuEnv| {
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply, FUNDED);
            assert_eq!(
                tokens
                    .map(|token| env.token_amount(token))
                    .iter()
                    .sum::<u64>()
                    + env.token_amount(env.vault),
                FUNDED
            );
            assert_eq!(
                env.market_state().1.vault,
                u128::from(env.token_amount(env.vault))
            );
        };
        let land = |env: &mut V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]| {
            env.svm.expire_blockhash();
            let mut batch = vec![heap_ix(), cu_ix()];
            batch.extend_from_slice(instructions);
            let mut all_signers = vec![&env.payer];
            all_signers.extend_from_slice(signers);
            let tx = Transaction::new_signed_with_payer(
                &batch,
                Some(&env.payer.pubkey()),
                &all_signers,
                env.svm.latest_blockhash(),
            );
            env.svm.send_transaction(tx)
        };
        let withdraw = |env: &V16CuEnv, insurance: bool, amount: u64| {
            let role = usize::from(insurance);
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(recipients[role + 2].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[role + 2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: if insurance {
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 1,
                        market_id: env.asset_market_id(1),
                        authority_epoch: env.control_sequences(1).authority_epoch,
                        amount: u128::from(amount),
                    }
                } else {
                    ProgInstruction::WithdrawBackingBucket {
                        domain: 3,
                        market_id: env.asset_market_id(1),
                        authority_epoch: env.control_sequences(1).authority_epoch,
                        amount: u128::from(amount),
                    }
                }
                .encode(),
            }
        };
        let retained = [
            withdraw(&env, false, FIRST[0]),
            withdraw(&env, true, FIRST[1]),
        ];
        let authorities = [&provider, &insurer];
        let reject = |env: &mut V16CuEnv,
                      batch: &[Instruction],
                      signers: &[&Keypair],
                      index,
                      error: PercolatorError| {
            let before = frame(env);
            let failure = land(env, batch, signers).expect_err("terminal allowance must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32),)
            );
            assert_eq!(
                frame(env),
                before,
                "rejection preserves every non-fee-payer account"
            );
            custody(env);
            failure.meta
        };
        for role in 0..2 {
            reject(
                &mut env,
                &[retained[role].clone()],
                &[authorities[role]],
                2,
                PercolatorError::EngineLockActive,
            );
        }

        // User claims are paid permissionlessly after the exit window. Provider funds cannot
        // leave even after payment until both now-empty portfolios are publicly closed.
        env.svm.warp_to_slot(7);
        let payout = |env: &V16CuEnv, index: usize| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owners[index].pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[index], false),
                AccountMeta::new(tokens[index], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
            .encode(),
        };
        for _ in 0..8 {
            for index in [1, 0] {
                if !resolved_portfolio_is_terminal(&env, portfolios[index]) {
                    let instruction = payout(&env, index);
                    let before = frame(&env);
                    let meta = land(&mut env, &[instruction], &[]).expect("user payout progresses");
                    assert_ne!(frame(&env), before);
                    assert_cu_within(
                        "INV-067 permissionless user payout",
                        meta.compute_units_consumed,
                        CUSTODY_CU_LIMIT,
                    );
                    peak_cu = peak_cu.max(meta.compute_units_consumed);
                    custody(&env);
                }
            }
            if portfolios
                .iter()
                .all(|key| resolved_portfolio_is_terminal(&env, *key))
            {
                break;
            }
        }
        assert!(portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&env, *key)));
        assert_eq!(
            tokens.map(|token| env.token_amount(token)),
            [1_100, 900, 0, 0, 0]
        );
        assert_eq!(env.market_state().1.c_tot, 0);
        for role in 0..2 {
            reject(
                &mut env,
                &[retained[role].clone()],
                &[authorities[role]],
                2,
                PercolatorError::EngineLockActive,
            );
        }
        for index in 0..2 {
            env.close_portfolio_with_cu(&owners[index], portfolios[index]);
        }
        let terminal = env.market_state().1;
        assert_eq!(terminal.materialized_portfolio_count, 0);
        assert_eq!(
            terminal.source_backing_buckets[3].fresh_unliened_backing_num,
            u128::from(BACKING) * BOUND_SCALE
        );
        assert!(
            terminal.source_backing_buckets[3].consumed_liened_backing_num > 0,
            "the trading claim must have actually consumed and replenished backing"
        );
        assert_eq!(
            &terminal.insurance_domain_budget[..4],
            &[u128::from(UNRELATED_INSURANCE), 0, 101, 103]
        );
        let unrelated_source = terminal.source_credit[0];
        let unrelated_backing = terminal.source_backing_buckets[0];
        let partition = |env: &V16CuEnv, paid: [u64; 2]| {
            let group = env.market_state().1;
            assert_eq!(
                tokens.map(|token| env.token_amount(token)),
                [1_100, 900, paid[0], paid[1], 0]
            );
            assert_eq!(
                group.vault,
                u128::from(BACKING + INSURANCE + UNRELATED_INSURANCE - paid[0] - paid[1])
            );
            assert_eq!(
                group.insurance,
                u128::from(INSURANCE + UNRELATED_INSURANCE - paid[1])
            );
            assert_eq!(
                group.insurance_domain_budget[0],
                u128::from(UNRELATED_INSURANCE)
            );
            assert_eq!(
                group.insurance_domain_budget[2] + group.insurance_domain_budget[3],
                u128::from(INSURANCE - paid[1])
            );
            assert_eq!(
                group.source_backing_buckets[3].fresh_unliened_backing_num,
                u128::from(BACKING - paid[0]) * BOUND_SCALE
            );
            assert_eq!(group.source_credit[0], unrelated_source);
            assert_eq!(group.source_backing_buckets[0], unrelated_backing);
            custody(env);
        };

        let order = if insurance_first { [1, 0] } else { [0, 1] };
        let mut paid = [0u64; 2];
        for role in order {
            let before = frame(&env);
            let meta = land(&mut env, &[retained[role].clone()], &[authorities[role]])
                .expect("retained withdrawal becomes payable only after user exits");
            assert_cu_within(
                "INV-067 terminal first withdrawal",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            paid[role] += FIRST[role];
            for (index, key) in frame_keys.iter().enumerate() {
                if ![env.market, env.vault, tokens[role + 2]].contains(key) {
                    assert_eq!(
                        env.svm.get_account(key),
                        before[index],
                        "unrelated payout state"
                    );
                }
            }
            partition(&env, paid);
            reject(
                &mut env,
                &[retained[role].clone()],
                &[authorities[role]],
                2,
                PercolatorError::EngineLockActive,
            );
        }

        // The first instruction really transfers, then the other recipient's retained
        // overdraw rejects. The whole prefix must roll back, leaving its remainder payable.
        for role in order {
            let other = 1 - role;
            let remainder = withdraw(&env, role == 1, REMAINDER[role]);
            let rejected = reject(
                &mut env,
                &[remainder.clone(), retained[other].clone()],
                &authorities,
                3,
                PercolatorError::EngineLockActive,
            );
            assert_eq!(
                rejected
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                1
            );
            assert_cu_within(
                "INV-067 terminal atomic withdrawal rollback",
                rejected.compute_units_consumed,
                500_000,
            );
            peak_cu = peak_cu.max(rejected.compute_units_consumed);
            let meta = land(&mut env, &[remainder.clone()], &[authorities[role]])
                .expect("unchanged remainder pays after rollback");
            assert_cu_within(
                "INV-067 terminal remainder retry",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            paid[role] += REMAINDER[role];
            partition(&env, paid);
            for instruction in [retained[role].clone(), remainder] {
                reject(
                    &mut env,
                    &[instruction],
                    &[authorities[role]],
                    2,
                    PercolatorError::EngineLockActive,
                );
            }
        }
        assert_eq!(paid, [BACKING, INSURANCE]);
        let after = env.market_state().1;
        assert_eq!(after.vault, u128::from(UNRELATED_INSURANCE));
        assert_eq!(after.insurance, u128::from(UNRELATED_INSURANCE));
        assert_eq!(
            &after.insurance_domain_budget[..4],
            &[u128::from(UNRELATED_INSURANCE), 0, 0, 0]
        );
        assert_eq!(after.source_credit[0], unrelated_source);
        assert_eq!(after.source_backing_buckets[0], unrelated_backing);
        assert_eq!(
            after.source_backing_buckets[3].fresh_unliened_backing_num,
            0
        );

        let cu = env.withdraw_insurance_domain_to_admin_token_with_cu(
            tokens[4],
            0,
            u128::from(UNRELATED_INSURANCE),
        );
        assert_cu_within("INV-067 unrelated terminal insurance", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        assert_eq!(
            tokens.map(|token| env.token_amount(token)),
            [1_100, 900, BACKING, INSURANCE, UNRELATED_INSURANCE]
        );
        assert_eq!(env.token_amount(env.vault), 0);
        assert_eq!(env.market_state().1.insurance, 0);
        custody(&env);
        env.close_slab_with_cu();
        assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
        println!("INV-067 provider/insurance order insurance_first={insurance_first}: users=2000 backing={BACKING} insurance={INSURANCE} unrelated={UNRELATED_INSURANCE}; zero vault residue");
    }
    if !absent_provider {
        println!("INV-067 provider/insurance terminal suffix peak CU {peak_cu}");
    }
}

fn verify_absent_provider_suffix(
    env: &mut V16CuEnv,
    owners: &[Keypair; 2],
    insurer: &Keypair,
    absent: [Pubkey; 2],
    tokens: [Pubkey; 5],
    winner_first: bool,
) {
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const PAYOUTS: [u64; 2] = [CAPITAL + 20 * (105 - 100), CAPITAL - 20 * (105 - 100)];
    let portfolios = [env.portfolios[0], env.portfolios[1]];
    let admin = env.admin.insecure_clone();
    let mut tracked = vec![
        env.market,
        env.vault,
        env.mint,
        insurer.pubkey(),
        admin.pubkey(),
    ];
    tracked.extend(portfolios);
    tracked.extend(tokens);
    tracked.extend(absent);
    tracked.extend(owners.each_ref().map(|owner| owner.pubkey()));
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    let absent_before = absent.map(|key| env.svm.get_account(&key));
    let check = |env: &V16CuEnv| {
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
        let mint = Mint::unpack(&mint_before.data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply, FUNDED);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                + env.token_amount(env.vault),
            FUNDED
        );
        assert_eq!(env.token_amount(tokens[2]), 0);
        assert_eq!(absent.map(|key| env.svm.get_account(&key)), absent_before);
        for i in 0..2 {
            assert!(env.token_amount(tokens[i]) <= PAYOUTS[i]);
        }
    };
    let land = |env: &mut V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]| {
        env.svm.expire_blockhash();
        let mut batch = vec![heap_ix(), cu_ix()];
        batch.extend_from_slice(instructions);
        let mut all_signers = vec![&env.payer];
        all_signers.extend_from_slice(signers);
        assert!(all_signers
            .iter()
            .all(|signer| !absent.contains(&signer.pubkey())));
        let tx = Transaction::new_signed_with_payer(
            &batch,
            Some(&env.payer.pubkey()),
            &all_signers,
            env.svm.latest_blockhash(),
        );
        // Include every compiled account, even readonly programs and the vault PDA.
        let frame: Vec<_> = tracked
            .iter()
            .chain(&tx.message.account_keys)
            .map(|key| (*key, env.svm.get_account(key)))
            .collect();
        let payer = env.payer.pubkey();
        let fee = u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let result = env.svm.send_transaction(tx);
        for (key, before) in frame {
            if result.is_err() || key == payer {
                let expected = before.map(|mut account| {
                    if key == payer {
                        account.lamports -= fee;
                    }
                    account
                });
                assert_eq!(env.svm.get_account(&key), expected, "terminal frame {key}");
            }
        }
        check(env);
        result
    };
    let unsigned_provider = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawBackingBucket {
            domain: 3,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            amount: BACKING.into(),
        }
        .encode(),
    };
    let reject = |env: &mut V16CuEnv,
                  batch: &[Instruction],
                  signers: &[&Keypair],
                  index,
                  expected: PercolatorError| {
        let failure = land(env, batch, signers).expect_err("provider consent remains required");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(expected as u32))
        );
        assert_cu_within(
            "INV-073 absent-provider rollback",
            failure.meta.compute_units_consumed,
            if batch.len() == 1 {
                CUSTODY_CU_LIMIT
            } else {
                500_000
            },
        );
        failure.meta
    };
    reject(
        env,
        &[unsigned_provider.clone()],
        &[],
        2,
        PercolatorError::ExpectedSigner,
    );

    env.svm.warp_to_slot(7);
    let order = if winner_first { [0, 1] } else { [1, 0] };
    let rank = |env: &V16CuEnv| {
        let legs: u32 = portfolios
            .iter()
            .map(|key| {
                percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(*key)))
            })
            .sum();
        let unpaid = 2 * CAPITAL - env.token_amount(tokens[0]) - env.token_amount(tokens[1]);
        (legs, unpaid)
    };
    let mut calls = 0;
    let mut peak_cu = 0;
    for _ in 0..8 {
        for i in order {
            if resolved_portfolio_is_terminal(env, portfolios[i]) {
                continue;
            }
            let payout = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owners[i].pubkey(), false),
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
            let before_rank = rank(env);
            let before: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
            let meta = land(env, &[payout], &[]).expect("keeper progresses without either role");
            assert!(
                rank(env) < before_rank,
                "each keeper call reduces (legs, unpaid value)"
            );
            for (key, account) in tracked.iter().zip(before) {
                if ![env.market, portfolios[i], env.vault, tokens[i]].contains(key) {
                    assert_eq!(
                        env.svm.get_account(key),
                        account,
                        "unrelated user-exit frame {key}"
                    );
                }
            }
            assert_cu_within(
                "INV-073 absent-provider user exit",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            calls += 1;
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(env, *key))
        {
            break;
        }
    }
    assert!(portfolios
        .iter()
        .all(|key| resolved_portfolio_is_terminal(env, *key)));
    assert_eq!(rank(env), (0, 0));
    assert_eq!(
        tokens.map(|key| env.token_amount(key)),
        [PAYOUTS[0], PAYOUTS[1], 0, 0, 0]
    );
    let paid = env.market_state().1;
    assert_eq!(
        (
            paid.c_tot,
            paid.pnl_pos_tot,
            paid.materialized_portfolio_count
        ),
        (0, 0, 2)
    );
    assert_eq!(
        (paid.assets[1].oi_eff_long_q, paid.assets[1].oi_eff_short_q),
        (0, 0)
    );
    assert_eq!(paid.insurance, u128::from(INSURANCE + UNRELATED_INSURANCE));
    assert_eq!(
        paid.source_backing_buckets[3].fresh_unliened_backing_num,
        u128::from(BACKING) * BOUND_SCALE
    );
    assert!(paid.source_backing_buckets[3].consumed_liened_backing_num > 0);
    for i in order {
        let cu = env.close_portfolio_with_cu(&owners[i], portfolios[i]);
        assert_cu_within("INV-021 owner portfolio deletion", cu, CUSTODY_CU_LIMIT);
        check(env);
    }
    let terminal = env.market_state().1;
    assert_eq!(terminal.materialized_portfolio_count, 0);
    let insurance_payout = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(insurer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[3], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 1,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            amount: INSURANCE.into(),
        }
        .encode(),
    };
    let mut wrong_provider = unsigned_provider.clone();
    wrong_provider.accounts[0] = AccountMeta::new(admin.pubkey(), true);
    wrong_provider.accounts[2] = AccountMeta::new(tokens[4], false);
    for (suffix, signers, error) in [
        (
            unsigned_provider,
            vec![insurer],
            PercolatorError::ExpectedSigner,
        ),
        (
            wrong_provider,
            vec![insurer, &admin],
            PercolatorError::Unauthorized,
        ),
    ] {
        let meta = reject(env, &[insurance_payout.clone(), suffix], &signers, 3, error);
        for program in [env.program_id, spl_token::ID] {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                1,
                "insurance payment must execute before signer rejection"
            );
        }
        peak_cu = peak_cu.max(meta.compute_units_consumed);
    }
    let meta = land(env, &[insurance_payout], &[insurer])
        .expect("beneficiary exit needs no operator or provider");
    assert_cu_within(
        "INV-064 terminal beneficiary exit",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    peak_cu = peak_cu.max(meta.compute_units_consumed);
    let after_insurance = env.market_state().1;
    assert_eq!(
        after_insurance.source_backing_buckets,
        terminal.source_backing_buckets
    );
    assert_eq!(after_insurance.source_credit[0], terminal.source_credit[0]);
    assert_eq!(
        &after_insurance.insurance_domain_budget[..4],
        &[u128::from(UNRELATED_INSURANCE), 0, 0, 0]
    );
    let cu = env.withdraw_insurance_domain_to_admin_token_with_cu(
        tokens[4],
        0,
        UNRELATED_INSURANCE.into(),
    );
    assert_cu_within("INV-064 unrelated insurance exit", cu, CUSTODY_CU_LIMIT);
    check(env);
    let final_state = env.market_state().1;
    assert_eq!(
        tokens.map(|key| env.token_amount(key)),
        [PAYOUTS[0], PAYOUTS[1], 0, INSURANCE, UNRELATED_INSURANCE]
    );
    assert_eq!(
        (
            final_state.vault,
            final_state.c_tot,
            final_state.pnl_pos_tot,
            final_state.insurance
        ),
        (BACKING.into(), 0, 0, 0)
    );
    assert_eq!(
        final_state.source_backing_buckets,
        terminal.source_backing_buckets
    );
    assert_eq!(final_state.backing_provider_earnings_total, 0);
    assert_eq!(final_state.materialized_portfolio_count, 0);
    println!("INV-073 absent provider/operator: winner_first={winner_first}, keeper_calls={calls}/16, exact_rollbacks=3, retained_principal={BACKING}, peak_cu={peak_cu}; administrative retirement remains open");
}
