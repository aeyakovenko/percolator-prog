//! INV-073: an absent provider's entire expiry-authorized principal reaches retirement.
//! Unlike the cooperative mixed-maturity control, no backing withdrawal occurs after funding.
//! User payout is permissionless; empty-portfolio deletion and slab retirement are signer-gated.
//! Fresh principal, earnings, and absent insurance beneficiaries remain open in rows 420/421.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_absent_provider_staggered_expiry_reaches_funded_terminal_retirement() {
    const CAPITAL: u64 = 1_009;
    const BACKING: [u64; 2] = [401, 307];
    const RESERVE: u64 = BACKING[0] + BACKING[1];
    const SUPPLY: u64 = CAPITAL + RESERVE;
    const CU_LIMIT: u32 = 150_000;

    for expiries in [[9, 13], [13, 9]] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 1,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let provider = Keypair::new();
        let owner = Keypair::new();
        for role in [&provider, &owner] {
            env.svm.airdrop(&role.pubkey(), 1_000_000_000).unwrap();
        }
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(&provider),
            0,
            processor::ASSET_AUTH_BACKING_BUCKET,
            provider.pubkey().to_bytes(),
        )
        .unwrap();
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        env.configure_permissionless_resolve_with_cu(2, 1);
        let tokens = [&owner, &provider, &admin]
            .map(|role| create_ata_for_test(&mut env.svm, &env.payer, role.pubkey(), env.mint));
        for (token, amount) in tokens.into_iter().zip([CAPITAL, RESERVE]) {
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
        let portfolio_key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
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
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();
        for domain in 0..2 {
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain: domain as u16,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: BACKING[domain].into(),
                    expiry_slot: expiries[domain],
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[1], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&provider],
            )
            .unwrap();
        }
        let absent_provider = provider.pubkey();
        drop(provider);
        let provider_frame = env.svm.get_account(&absent_provider);
        let provider_token_frame = env.svm.get_account(&tokens[1]);
        let mint_frame = env.svm.get_account(&env.mint).unwrap();
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let sequences = env.control_sequences(0);
        let market_key = env.market;
        let wrap = |data: ProgInstruction, accounts| Instruction {
            program_id: percolator_prog::id(),
            accounts,
            data: data.encode(),
        };
        let close = wrap(
            ProgInstruction::CloseSlab {
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
        );
        let tracked = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            portfolio,
            tokens[0],
            tokens[1],
            tokens[2],
            owner.pubkey(),
            absent_provider,
            admin.pubkey(),
        ];
        let land = |env: &mut V16CuEnv,
                    instructions: &[Instruction],
                    signers: &[&Keypair],
                    rejection: Option<(u8, usize, PercolatorError)>| {
            env.svm.expire_blockhash();
            let mut batch = vec![
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT),
            ];
            batch.extend_from_slice(instructions);
            let mut signing = vec![&env.payer];
            signing.extend_from_slice(signers);
            let tx = Transaction::new_signed_with_payer(
                &batch,
                Some(&env.payer.pubkey()),
                &signing,
                env.svm.latest_blockhash(),
            );
            let required = usize::from(tx.message.header.num_required_signatures);
            assert_eq!(required, 1 + signers.len());
            assert!(!tx.message.account_keys[..required].contains(&absent_provider));
            tx.verify().unwrap();
            assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
            let mut keys = tx.message.account_keys.clone();
            keys.extend(tracked);
            keys.sort_unstable();
            keys.dedup();
            let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            payer.lamports -= required as u64 * FeeStructure::default().lamports_per_signature;
            let result = env.svm.send_transaction(tx);
            let meta = if let Some((index, successes, error)) = rejection {
                let failure = result.expect_err("unexpired principal must retain its claim");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        index,
                        InstructionError::Custom(error as u32),
                    )
                );
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| { **line == format!("Program {} success", env.program_id) })
                        .count(),
                    successes,
                );
                for (key, account) in keys.into_iter().zip(before) {
                    if key != env.payer.pubkey() {
                        assert_eq!(env.svm.get_account(&key), account, "rollback for {key}");
                    }
                }
                failure.meta
            } else {
                result.expect("bounded terminal continuation with absent provider")
            };
            assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
            assert_eq!(env.svm.get_account(&absent_provider), provider_frame);
            assert_eq!(env.svm.get_account(&tokens[1]), provider_token_frame);
            assert_cu_within(
                "INV-073 absent-provider expiry retirement",
                meta.compute_units_consumed,
                CU_LIMIT.into(),
            );
            meta.compute_units_consumed
        };
        let stock = |env: &V16CuEnv, paid: bool, materialized: bool, expired: [bool; 2]| {
            let group = env.market_state().1;
            let capital = if paid { 0 } else { CAPITAL };
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(group.c_tot, capital.into());
            assert_eq!(group.materialized_portfolio_count, u64::from(materialized));
            assert_eq!(group.vault, u128::from(capital + RESERVE));
            assert_eq!(group.insurance, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
            for domain in 0..2 {
                let bucket = group.source_backing_buckets[domain];
                let source = group.source_credit[domain];
                let fresh = if expired[domain] {
                    0
                } else {
                    u128::from(BACKING[domain]) * BOUND_SCALE
                };
                assert_eq!(bucket.expiry_slot, expiries[domain]);
                assert_eq!(
                    bucket.status,
                    if expired[domain] {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(bucket.fresh_unliened_backing_num, fresh);
                assert_eq!(bucket.utilization_fee_earnings, 0);
                assert_eq!(source.fresh_reserved_backing_num, fresh);
                assert_eq!(
                    (
                        source.provider_receivable_num,
                        source.valid_liened_backing_num,
                        source.spent_backing_num
                    ),
                    (0, 0, 0)
                );
            }
            assert_eq!(
                tokens.map(|key| env.token_amount(key)),
                [CAPITAL - capital, 0, 0]
            );
            assert_eq!(env.token_amount(env.vault), capital + RESERVE);
            assert_eq!(
                env.token_amount(env.vault) + env.token_amount(tokens[0]),
                SUPPLY
            );
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&market_key).unwrap().data,
                    0
                )
                .unwrap(),
                profile
            );
            assert_eq!(env.control_sequences(0), sequences);
            if materialized {
                assert_eq!(env.portfolio_state(portfolio).capital.get(), capital.into());
            }
        };

        env.svm.warp_to_slot(5);
        let mut peak = land(
            &mut env,
            &[wrap(
                ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
                vec![AccountMeta::new(market_key, false)],
            )],
            &[],
            None,
        );
        stock(&env, false, true, [false; 2]);
        env.svm.warp_to_slot(6);
        let payout = wrap(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        peak = peak.max(land(&mut env, &[payout], &[], None));
        assert!(resolved_portfolio_is_terminal(&env, portfolio));
        stock(&env, true, true, [false; 2]);
        let delete = wrap(
            env.close_portfolio_ix(portfolio),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
        );
        peak = peak.max(land(&mut env, &[delete], &[&owner], None));
        stock(&env, true, false, [false; 2]);
        let custody_keys = [
            env.vault,
            env.mint,
            tokens[0],
            tokens[1],
            tokens[2],
            admin.pubkey(),
        ];
        let custody_before = custody_keys.map(|key| env.svm.get_account(&key));
        env.svm.warp_to_slot(8);
        peak = peak.max(land(
            &mut env,
            &[close.clone()],
            &[&admin],
            Some((2, 0, PercolatorError::EngineLockActive)),
        ));
        stock(&env, true, false, [false; 2]);

        // The first expiry is real progress, but a premature second close rolls it back.
        env.svm.warp_to_slot(9);
        peak = peak.max(land(
            &mut env,
            &[close.clone(), close.clone()],
            &[&admin],
            Some((3, 1, PercolatorError::EngineLockActive)),
        ));
        stock(&env, true, false, [false; 2]);
        peak = peak.max(land(&mut env, &[close.clone()], &[&admin], None));
        let expired = expiries.map(|slot| slot == 9);
        stock(&env, true, false, expired);
        assert_eq!(
            custody_keys.map(|key| env.svm.get_account(&key)),
            custody_before
        );

        env.svm.warp_to_slot(13);
        peak = peak.max(land(&mut env, &[close.clone()], &[&admin], None));
        stock(&env, true, false, [true; 2]);
        assert_eq!(
            custody_keys.map(|key| env.svm.get_account(&key)),
            custody_before
        );
        let market_before = env.svm.get_account(&env.market).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let mut admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
        peak = peak.max(land(&mut env, &[close], &[&admin], None));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(
            tombstone.lamports,
            env.svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
        );
        admin_before.lamports +=
            market_before.lamports + vault_before.lamports - tombstone.lamports;
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(admin_before));
        assert!(env.svm.get_account(&env.vault).is_none_or(
            |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
        ));
        let mut expected_mint = mint_frame.clone();
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        assert_eq!(mint.supply, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        mint.supply -= RESERVE;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
        assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
        assert_eq!(tokens.map(|key| env.token_amount(key)), [CAPITAL, 0, 0]);
        for (index, key) in tokens.into_iter().enumerate() {
            assert_eq!(env.svm.get_account(&key), custody_before[index + 2]);
        }
        assert_cu_within(
            "INV-073 complete absent-provider history",
            peak,
            CU_LIMIT.into(),
        );
        println!("INV-073 absent provider expiries={expiries:?}: payout={CAPITAL}, retired={RESERVE}, successful slab calls=3, exact rollbacks=2, peak={peak} CU");
    }
}
