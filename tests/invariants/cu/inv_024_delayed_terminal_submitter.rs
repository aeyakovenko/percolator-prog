//! INV-024/036/081, row410: expiry during delayed user settlement or mechanical
//! cleanup cannot turn a settled user or terminal operator into a fee beneficiary.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_delayed_terminal_cleanup_preserves_earned_fees_across_submitter_changes() {
    const PREFIX: u64 = 17;
    let mut peak = 0;
    let mut worlds = 0;
    for expiry_slot in [100, 101] {
        for (delay_settlement, fees_before_normalization) in
            [(false, false), (false, true), (true, false)]
        {
            let (
                TerminalEarningsWorld {
                    mut env,
                    admin,
                    incumbent: provider,
                    successor: operator,
                    mut wallets,
                    mut tokens,
                    portfolios,
                    mint_frame,
                },
                users,
            ) = terminal_earnings_world_with_user_signers(false, None);
            let keeper = env.payer.insecure_clone();
            let insurer = Keypair::new();
            env.svm.airdrop(&insurer.pubkey(), 1_000_000_000).unwrap();
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&insurer),
                0,
                processor::ASSET_AUTH_INSURANCE,
                insurer.pubkey().to_bytes(),
            )
            .unwrap();
            let admin_token = tokens[4];
            wallets[4] = insurer.pubkey();
            tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
            let keeper_token =
                create_ata_for_test(&mut env.svm, &env.payer, keeper.pubkey(), env.mint);
            let distinct = wallets
                .into_iter()
                .chain([admin.pubkey(), keeper.pubkey()])
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(distinct.len(), 7);
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger.pubkey();
            let empty_ledger = env.svm.get_account(&ledger).unwrap();
            let tracked = [env.market, env.vault, env.mint, ledger]
                .into_iter()
                .chain(wallets)
                .chain(tokens)
                .chain(portfolios)
                .chain([admin.pubkey(), keeper.pubkey(), admin_token, keeper_token])
                .collect::<Vec<_>>();
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(
                profile.backing_bucket_authority,
                provider.pubkey().to_bytes()
            );
            assert_eq!(profile.insurance_authority, insurer.pubkey().to_bytes());
            assert_eq!(profile.insurance_operator, operator.pubkey().to_bytes());
            let sequences = env.control_sequences(0);
            // Construct public beneficiary-bound payouts before resolution and retain
            // their bytes through both delay boundaries and every payer change.
            let fee_prefix = reserve_payout(&env, wallets, tokens, ledger, 1, PREFIX);
            let fee_tail = reserve_payout(&env, wallets, tokens, ledger, 1, EARNINGS - PREFIX);
            let insurance = reserve_payout(&env, wallets, tokens, ledger, 2, INSURANCE);
            for ix in [&fee_prefix, &fee_tail, &insurance] {
                assert!(ix.accounts.iter().all(|account| !account.is_signer));
            }
            env.resolve();
            assert_eq!(env.market_state().1.resolved_slot, 2);
            env.svm
                .warp_to_slot(if delay_settlement { expiry_slot } else { 7 });
            let settlement_slot = if delay_settlement { expiry_slot } else { 7 };
            let mut calls = 0;
            for _ in 0..8 {
                // Settle the loss before the gain, alternating unrelated public payers.
                for actor in [1, 0] {
                    if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                        continue;
                    }
                    env.payer = if actor == 1 { &operator } else { &keeper }.insecure_clone();
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(wallets[actor], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                        .encode(),
                    };
                    let progress_keys = [env.market, portfolios[actor]];
                    let before = progress_keys.map(|key| env.svm.get_account(&key));
                    let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
                    peak = peak.max(land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    calls += 1;
                    assert_ne!(progress_keys.map(|key| env.svm.get_account(&key)), before);
                    assert_eq!(
                        env.token_amount(env.vault)
                            + tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
                        SUPPLY
                    );
                    assert_eq!(
                        env.market_state().1.backing_provider_earnings_total,
                        EARNINGS.into()
                    );
                    assert_eq!(env.token_amount(tokens[2]), 0);
                    assert_eq!(env.token_amount(tokens[3]), 0);
                    assert_eq!(env.token_amount(tokens[4]), 0);
                }
                if portfolios
                    .iter()
                    .all(|key| resolved_portfolio_is_terminal(&env, *key))
                {
                    break;
                }
            }
            for actor in 0..2 {
                assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
                assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
            }
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let check = |env: &V16CuEnv, count: u64, expired: bool, paid: [u64; 2]| {
                let amounts = [PAYOUTS[0], PAYOUTS[1], paid[0], 0, paid[1]];
                let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
                for ((key, frame), amount) in tokens.into_iter().zip(&token_frames).zip(amounts) {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                let mut expected_vault = vault_frame.clone();
                let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut expected_vault.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                assert_eq!(env.token_amount(admin_token), 0);
                assert_eq!(env.token_amount(keeper_token), 0);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                let mut image = env.svm.get_account(&env.market).unwrap();
                let (cfg, group) = env.market_state();
                assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, count)
                );
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.vault, remaining.into());
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - paid[0])
                );
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[1]));
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert!(group
                    .insurance_domain_spent
                    .iter()
                    .all(|amount| *amount == 0));
                let bucket = group.source_backing_buckets[1];
                let source = group.source_credit[1];
                let principal = if expired {
                    0
                } else {
                    u128::from(BACKING) * BOUND_SCALE
                };
                assert_eq!(bucket.expiry_slot, 100);
                assert_eq!(
                    bucket.status,
                    if expired {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(bucket.fresh_unliened_backing_num, principal);
                assert_eq!(source.fresh_reserved_backing_num, principal);
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - paid[0])
                );
                // Late settlement expires backing before conversion. The losing
                // user's capital still funds the same gain, without provider credit.
                let provider_credit = if delay_settlement {
                    0
                } else {
                    u128::from(PROFIT) * BOUND_SCALE
                };
                assert_eq!(source.provider_receivable_num, provider_credit);
                assert_eq!(source.valid_liened_backing_num, 0);
                assert_eq!(source.spent_backing_num, provider_credit);
                assert_eq!(bucket.consumed_liened_backing_num, provider_credit);
                assert_eq!(
                    state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                    profile
                );
                assert_eq!(env.control_sequences(0), sequences);
                assert_domain_budget_remaining_total_consistent(
                    &group,
                    "delayed terminal submitter",
                );
                let materialized = portfolios[..count as usize]
                    .iter()
                    .map(|key| env.portfolio_state(*key))
                    .collect::<Vec<_>>();
                crate::support::fuzz_model::assert_market_stock_census(
                    "delayed terminal submitter",
                    &group,
                    &image.data,
                    &materialized,
                    remaining.into(),
                )
                .unwrap();
                state::market_view_mut(&mut image.data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
                let account = env.svm.get_account(&ledger).unwrap();
                if paid[0] == 0 {
                    assert_eq!(account, empty_ledger);
                } else {
                    let record = state::read_backing_domain_ledger(&account.data).unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.domain, 1);
                    assert_eq!(record.authority, provider.pubkey().to_bytes());
                    assert_eq!(record.total_principal_atoms, 0);
                    assert_eq!(record.total_earnings_withdrawn_atoms, paid[0].into());
                    assert_eq!(record.total_earnings_atoms, 0);
                    assert_eq!(
                        record.last_observed_bucket_earnings_atoms,
                        u128::from(EARNINGS - paid[0])
                    );
                    assert_eq!(account.lamports, empty_ledger.lamports);
                }
            };
            check(&env, 2, delay_settlement, [0; 2]);
            let cleanup = portfolios.map(|portfolio| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                data: env.close_portfolio_ix(portfolio).encode(),
            });
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
            let portfolio_rent = portfolios.map(|key| env.svm.get_account(&key).unwrap().lamports);
            env.payer = users[1].insecure_clone();
            let allowed = [env.market, portfolios[1]];
            peak = peak.max(land(
                &mut env,
                &[cleanup[1].clone()],
                &[&admin],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            check(&env, 1, delay_settlement, [0; 2]);
            env.svm.warp_to_slot(expiry_slot);
            env.payer = users[0].insecure_clone();
            // Crossing expiry alone grants neither cleanup progress nor reserve access.
            for (ix, signers) in [(close.clone(), vec![&admin]), (fee_prefix.clone(), vec![])] {
                peak = peak.max(land(
                    &mut env,
                    &[ix],
                    &signers,
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((2, PercolatorError::EngineLockActive)),
                ));
                check(&env, 1, delay_settlement, [0; 2]);
            }
            let allowed = [env.market, portfolios[0]];
            peak = peak.max(land(
                &mut env,
                &[cleanup[0].clone()],
                &[&admin],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            check(&env, 0, delay_settlement, [0; 2]);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_rent + portfolio_rent.iter().sum::<u64>()
            );
            for portfolio in portfolios {
                assert!(env
                    .svm
                    .get_account(&portfolio)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            }

            let mut paid = [0; 2];
            let mut expired = delay_settlement;
            let actions: &[bool] = if delay_settlement {
                &[true]
            } else if fees_before_normalization {
                &[true, false]
            } else {
                &[false, true]
            };
            for &pay_fees in actions {
                if pay_fees {
                    env.payer = users[1].insecure_clone();
                    let allowed = [env.market, env.vault, ledger, tokens[2]];
                    peak = peak.max(land(
                        &mut env,
                        &[fee_prefix.clone()],
                        &[],
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    paid[0] = PREFIX;
                } else {
                    env.payer = users[0].insecure_clone();
                    let allowed = [env.market];
                    peak = peak.max(land(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    expired = true;
                }
                check(&env, 0, expired, paid);
            }
            // The former losing user paid the prefix; the insurance operator pays
            // the fee tail and the settled winner pays the insurer's separate stock.
            env.payer = operator.insecure_clone();
            let allowed = [env.market, env.vault, ledger, tokens[2]];
            peak = peak.max(land(
                &mut env,
                &[fee_tail],
                &[],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            paid[0] = EARNINGS;
            check(&env, 0, true, paid);
            env.payer = users[0].insecure_clone();
            let allowed = [env.market, env.vault, tokens[4]];
            peak = peak.max(land(
                &mut env,
                &[insurance],
                &[],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            paid[1] = INSURANCE;
            check(&env, 0, true, paid);

            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports
                - tombstone_rent;
            let final_tokens = tokens.map(|key| env.svm.get_account(&key));
            let paid_ledger = env.svm.get_account(&ledger);
            env.payer = keeper.insecure_clone();
            let allowed = [env.market, env.vault, env.mint];
            peak = peak.max(land(
                &mut env,
                &[close],
                &[&admin],
                &tracked,
                &allowed,
                0,
                Some((admin.pubkey(), refund)),
                None,
            ));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(tokens.map(|key| env.svm.get_account(&key)), final_tokens);
            assert_eq!(env.svm.get_account(&ledger), paid_ledger);
            let mut expected_mint = mint_frame;
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply = SUPPLY - BACKING;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)),
                [PAYOUTS[0], PAYOUTS[1], EARNINGS, 0, INSURANCE]
            );
            assert_eq!(env.token_amount(admin_token), 0);
            assert_eq!(env.token_amount(keeper_token), 0);
            worlds += 1;
            eprintln!("row410 delayed cleanup: expiry={expiry_slot}, settlement={settlement_slot}, fees_before_normalization={fees_before_normalization}, user_calls={calls}/16");
        }
    }
    assert_eq!(worlds, 6);
    eprintln!("row410 delayed terminal submitter: {worlds} closures, 12 admission rollbacks, peak={peak} CU");
}
