//! INV-024/005/036/081, row410: deleting the last settled portfolio unlocks
//! reserve payments, but gives its authority signer and fee payer no entitlement.
//! A rejected redirect after deletion and an actual fee payout restores both.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[track_caller]
fn assert_deleted(env: &V16CuEnv, key: Pubkey, owner: Pubkey) {
    if let Some(account) = env.svm.get_account(&key) {
        assert_eq!(account.lamports, 0);
        assert!(account.data.is_empty());
        assert_eq!(account.owner, owner);
        assert!(!account.executable);
    }
}

#[test]
fn v16_program_last_portfolio_cleanup_cannot_confer_reserve_entitlement_on_submitter() {
    const FEE_PREFIX: u64 = 17;
    let mut peaks = [0; 4]; // blocked payout, rejected bundle, successful bundle, slab close
    for payer_role in 0..4 {
        let TerminalEarningsWorld {
            mut env,
            admin,
            incumbent,
            successor: operator,
            mut wallets,
            mut tokens,
            portfolios,
            mint_frame,
        } = terminal_earnings_world_with_exit(false);
        let keeper = env.payer.insecure_clone();
        let insurer = Keypair::new();
        let asset_admin = Keypair::new();
        for signer in [&insurer, &asset_admin] {
            env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
        }
        // Separate all control roles from the funded holders before resolution.
        // The fixture's original insurer consents to this live insurance transfer.
        for (kind, holder) in [
            (processor::ASSET_AUTH_INSURANCE, &insurer),
            (processor::ASSET_AUTH_ADMIN, &asset_admin),
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
        let admin_token = tokens[4];
        wallets[4] = insurer.pubkey();
        tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
        let submitters = [&keeper, &admin, &operator, &asset_admin];
        let submitter_wallets = submitters.map(|signer| signer.pubkey());
        let submitter_tokens = [
            create_ata_for_test(&mut env.svm, &env.payer, keeper.pubkey(), env.mint),
            admin_token,
            tokens[3],
            create_ata_for_test(&mut env.svm, &env.payer, asset_admin.pubkey(), env.mint),
        ];
        for submitter in submitter_wallets {
            assert_ne!(submitter, wallets[2]);
            assert_ne!(submitter, wallets[4]);
        }
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
        env.resolve();
        env.svm.warp_to_slot(7);
        for _ in 0..8 {
            for actor in [1, 0] {
                if !resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    env.svm.expire_blockhash();
                    env.send(
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(wallets[actor], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[],
                    )
                    .unwrap();
                }
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
        let cleanup = |env: &V16CuEnv, portfolio| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: env.close_portfolio_ix(portfolio).encode(),
        };
        let tracked = [env.market, env.vault, env.mint, ledger]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(submitter_wallets)
            .chain(submitter_tokens)
            .chain(portfolios)
            .collect::<Vec<_>>();
        let first_close = cleanup(&env, portfolios[1]);
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let first_rent = env.svm.get_account(&portfolios[1]).unwrap().lamports;
        let allowed = [env.market, portfolios[1]];
        land(
            &mut env,
            &[first_close],
            &[&admin],
            &tracked,
            &allowed,
            0,
            None,
            None,
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_rent + first_rent
        );
        assert_deleted(&env, portfolios[1], env.program_id);

        env.payer = submitters[payer_role].insecure_clone();
        let close_signers = if payer_role == 1 {
            vec![]
        } else {
            vec![&admin]
        };
        let terminal = env.market_state().1;
        let config = env.market_state().0;
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let sequences = env.control_sequences(0);
        assert_eq!(
            profile.backing_bucket_authority,
            incumbent.pubkey().to_bytes()
        );
        assert_eq!(profile.insurance_authority, insurer.pubkey().to_bytes());
        assert_eq!(profile.insurance_operator, operator.pubkey().to_bytes());
        assert_eq!(profile.asset_admin, asset_admin.pubkey().to_bytes());
        assert_eq!(config.marketauth, admin.pubkey().to_bytes());
        let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let last_rent = env.svm.get_account(&portfolios[0]).unwrap().lamports;
        let check = |env: &V16CuEnv, paid: [u64; 3], deleted: bool| {
            let image = env.svm.get_account(&env.market).unwrap();
            let (cfg, group) = env.market_state();
            let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
            assert_eq!(cfg, config);
            assert_eq!(env.control_sequences(0), sequences);
            assert_eq!(
                state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                profile
            );
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(group.materialized_portfolio_count, u64::from(!deleted));
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num
                ),
                (0, 0, 0)
            );
            assert_eq!(group.vault, remaining.into());
            assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
            assert_eq!(group.insurance_domain_budget, vec![group.insurance, 0]);
            assert_eq!(
                group.insurance_domain_spent,
                terminal.insurance_domain_spent
            );
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                group.insurance
            );
            assert_eq!(
                group.backing_provider_earnings_total,
                u128::from(EARNINGS - paid[1])
            );
            let bucket = group.source_backing_buckets[1];
            let principal_drained = paid[0] == BACKING;
            assert_eq!(
                bucket.status,
                if principal_drained {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(
                group.source_credit[1].credit_epoch,
                terminal.source_credit[1].credit_epoch + u64::from(principal_drained)
            );
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(BACKING - paid[0]) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            assert_eq!(
                bucket.utilization_fee_earnings,
                u128::from(EARNINGS - paid[1])
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(
                bucket.consumed_liened_backing_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].provider_receivable_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
            assert_eq!(
                group.source_backing_buckets[0],
                terminal.source_backing_buckets[0]
            );
            assert_eq!(group.source_credit[0], terminal.source_credit[0]);
            let amounts = [PAYOUTS[0], PAYOUTS[1], paid[0] + paid[1], 0, paid[2]];
            for ((key, frame), amount) in tokens
                .into_iter()
                .zip(&token_frames)
                .zip(amounts)
                .chain(std::iter::once(((env.vault, &vault_frame), remaining)))
            {
                let mut expected = frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amount;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(&key), Some(expected));
            }
            assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
            assert_eq!(submitter_tokens.map(|key| env.token_amount(key)), [0; 4]);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(
                image.lamports,
                market_rent + if deleted { last_rent } else { 0 }
            );
            if deleted {
                assert_deleted(env, portfolios[0], env.program_id);
            } else {
                assert!(resolved_portfolio_is_terminal(env, portfolios[0]));
            }
            let record = env.svm.get_account(&ledger).unwrap();
            if paid[1] == 0 {
                assert_eq!(record, empty_ledger);
            } else {
                let record = state::read_backing_domain_ledger(&record.data).unwrap();
                assert_eq!(record.market_group, env.market.to_bytes());
                assert_eq!(record.authority, wallets[2].to_bytes());
                // This ledger starts after funding, so it has no deposit history.
                assert_eq!(record.total_principal_atoms, 0);
                assert_eq!(record.total_deposited_atoms, 0);
                assert_eq!(record.total_principal_withdrawn_atoms, 0);
                assert_eq!(record.domain, 1);
                assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
                assert_eq!(
                    record.last_observed_bucket_earnings_atoms,
                    u128::from(EARNINGS - paid[1])
                );
            }
            let census_portfolios = if deleted {
                vec![]
            } else {
                vec![env.portfolio_state(portfolios[0])]
            };
            crate::support::fuzz_model::assert_market_stock_census(
                "terminal cleanup submitter",
                &group,
                &image.data,
                &census_portfolios,
                remaining.into(),
            )
            .unwrap();
            let mut data = image.data;
            state::market_view_mut(&mut data)
                .unwrap()
                .1
                .validate_shape()
                .unwrap();
        };
        check(&env, [0; 3], false);
        let close = cleanup(&env, portfolios[0]);
        let prefix = reserve_payout(&env, wallets, tokens, ledger, 1, FEE_PREFIX);
        peaks[0] = peaks[0].max(land(
            &mut env,
            &[prefix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        ));
        check(&env, [0; 3], false);

        // Retain the same valid close and fee payout across all failed suffixes.
        // Both finish in the transaction before the redirect is rejected.
        for kind in 0..3 {
            for replace_authority in [false, true] {
                let mut redirect = reserve_payout(&env, wallets, tokens, ledger, kind, 1);
                redirect.accounts[if kind == 1 { 3 } else { 2 }].pubkey =
                    submitter_tokens[payer_role];
                if replace_authority {
                    // Signer privilege comes from the transaction payer, including
                    // the market authority that authorized the preceding deletion.
                    redirect.accounts[0].pubkey = submitter_wallets[payer_role];
                    assert!(!redirect.accounts[0].is_signer);
                }
                let error = if replace_authority {
                    PercolatorError::Unauthorized
                } else {
                    PercolatorError::InvalidTokenAccount
                };
                peaks[1] = peaks[1].max(land(
                    &mut env,
                    &[close.clone(), prefix.clone(), redirect],
                    &close_signers,
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((4, error)),
                ));
                check(&env, [0; 3], false);
            }
        }
        let allowed = [env.market, portfolios[0], env.vault, ledger, tokens[2]];
        peaks[2] = peaks[2].max(land(
            &mut env,
            &[close, prefix],
            &close_signers,
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(&env, [0, FEE_PREFIX, 0], true);
        let tail = [
            reserve_payout(&env, wallets, tokens, ledger, 0, BACKING),
            reserve_payout(&env, wallets, tokens, ledger, 1, EARNINGS - FEE_PREFIX),
            reserve_payout(&env, wallets, tokens, ledger, 2, INSURANCE),
        ];
        assert!(tail
            .iter()
            .flat_map(|ix| &ix.accounts)
            .all(|meta| !meta.is_signer));
        let allowed = [env.market, env.vault, ledger, tokens[2], tokens[4]];
        peaks[2] = peaks[2].max(land(
            &mut env,
            &tail,
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(&env, [BACKING, EARNINGS, INSURANCE], true);

        let close_slab = Instruction {
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
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = env.svm.get_account(&env.market).unwrap().lamports
            + env.svm.get_account(&env.vault).unwrap().lamports
            - tombstone_rent;
        let allowed = [env.market, env.vault];
        peaks[3] = peaks[3].max(land(
            &mut env,
            &[close_slab],
            &close_signers,
            &tracked,
            &allowed,
            0,
            Some((admin.pubkey(), refund)),
            None,
        ));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert_deleted(&env, env.vault, solana_sdk::system_program::ID);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [PAYOUTS[0], PAYOUTS[1], BACKING + EARNINGS, 0, INSURANCE]
        );
        assert_eq!(submitter_tokens.map(|key| env.token_amount(key)), [0; 4]);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    }
    eprintln!("row410 cleanup submitter: 4 payer roles, 24 exact suffix rollbacks; peak CU [blocked, rejected bundle, payout bundle, slab close]={peaks:?}");
}
