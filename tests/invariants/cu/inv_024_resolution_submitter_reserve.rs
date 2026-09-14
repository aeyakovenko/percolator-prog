//! Row 410, INV-024/005/036/081: resolution and two reserve payouts roll back
//! together when the submitting live operator requests the insurer's terminal stock.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_resolution_bundle_cannot_preserve_submitter_live_reserve_authority() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    const PRINCIPAL: u64 = 79;
    const FUNDED: u64 = 131;
    const LIVE_PAID: u64 = 7;
    const TERMINAL_PREFIX: u64 = 17;
    const TOTAL: u64 = PRINCIPAL + FUNDED;
    let mut peak = 0;
    for permissionless in [false, true] {
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 1,
                ..V16CuMarketParams::default()
            },
            1,
        );
        let admin = env.admin.insecure_clone();
        let provider = Keypair::new();
        let operator = Keypair::new();
        let insurer = Keypair::new();
        for (kind, holder) in [
            (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
            (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            (processor::ASSET_AUTH_INSURANCE, &insurer),
        ] {
            env.svm.airdrop(&holder.pubkey(), 1_000_000_000).unwrap();
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
        env.configure_permissionless_resolve_with_cu(if permissionless { 5 } else { 100 }, 1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        let wallets = [
            env.payer.pubkey(),
            admin.pubkey(),
            provider.pubkey(),
            operator.pubkey(),
            insurer.pubkey(),
        ];
        assert_eq!(
            wallets
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            5
        );
        let tokens =
            wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
        let ledgers = [
            state::backing_domain_ledger_account_len(),
            state::insurance_ledger_account_len(),
        ]
        .map(|len| {
            let key = Keypair::new();
            system_create_account_for_test(&mut env.svm, &env.payer, &key, len, env.program_id);
            key.pubkey()
        });
        for (actor, amount) in [(2, PRINCIPAL), (4, FUNDED)] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
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
        let epoch = env.control_sequences(0).authority_epoch;
        for (actor, ledger, holder, ix) in [
            (
                2,
                ledgers[0],
                &provider,
                ProgInstruction::TopUpBackingBucket {
                    domain: 1,
                    market_id: env.asset_market_id(0),
                    authority_epoch: epoch,
                    intent_id: 0,
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: PRINCIPAL.into(),
                    expiry_slot: 100,
                },
            ),
            (
                4,
                ledgers[1],
                &insurer,
                ProgInstruction::TopUpInsuranceDomain {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: epoch,
                    intent_id: 0,
                    amount: FUNDED.into(),
                },
            ),
        ] {
            env.send(
                ix,
                vec![
                    AccountMeta::new(holder.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
                &[holder],
            )
            .unwrap();
        }
        let mint_frame = env.svm.get_account(&env.mint).unwrap();
        let mint = Mint::unpack(&mint_frame.data).unwrap();
        assert_eq!((mint.supply, mint.mint_authority), (TOTAL, COption::None));
        let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let ledger_frames = ledgers.map(|key| env.svm.get_account(&key).unwrap());
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let config = env.market_state().0;
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let sequences = env.control_sequences(0);
        let credit_epoch = env.market_state().1.source_credit[1].credit_epoch;
        let tracked = [env.market, env.vault, env.mint]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(ledgers)
            .collect::<Vec<_>>();

        // Inputs, not observed payout deltas, determine every remaining entitlement.
        let check = |env: &V16CuEnv, resolved: bool, paid: [u64; 3]| {
            let [backing_paid, operator_paid, insurer_paid] = paid;
            let backing = PRINCIPAL - backing_paid;
            let insurance = FUNDED - operator_paid - insurer_paid;
            let remaining = backing + insurance;
            let mut image = env.svm.get_account(&env.market).unwrap();
            let (cfg, group) = state::read_market(&image.data).unwrap();
            assert_eq!(cfg, config);
            assert_eq!(image.lamports, market_rent);
            assert_eq!(
                group.mode,
                if resolved {
                    MarketModeV16::Resolved
                } else {
                    MarketModeV16::Live
                }
            );
            assert_eq!(group.resolved_slot, if resolved { 7 } else { 0 });
            assert_eq!(
                (
                    group.materialized_portfolio_count,
                    group.c_tot,
                    group.pnl_pos_tot
                ),
                (0, 0, 0)
            );
            assert_eq!(group.source_claim_bound_total_num, 0);
            assert_eq!(group.vault, remaining.into());
            assert_eq!(group.insurance, insurance.into());
            assert_eq!(
                group.insurance_domain_budget,
                vec![u128::from(insurance), 0]
            );
            assert_eq!(group.insurance_domain_spent, vec![0, 0]);
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                insurance.into()
            );
            assert_eq!(group.backing_provider_earnings_total, 0);
            assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
            let bucket = group.source_backing_buckets[1];
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(backing) * BOUND_SCALE
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(bucket.utilization_fee_earnings, 0);
            assert_eq!(bucket.expiry_slot, if backing == 0 { 0 } else { 100 });
            assert_eq!(
                bucket.status,
                if backing == 0 {
                    BackingBucketStatusV16::Empty
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(
                group.source_credit[1].fresh_reserved_backing_num,
                u128::from(backing) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].credit_epoch,
                credit_epoch + u64::from(backing == 0)
            );
            assert_eq!(
                state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                profile
            );
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch += u64::from(operator_paid != 0);
            assert_eq!(env.control_sequences(0), expected_sequences);
            assert_eq!(
                profile.backing_bucket_authority,
                provider.pubkey().to_bytes()
            );
            assert_eq!(profile.insurance_authority, insurer.pubkey().to_bytes());
            assert_eq!(profile.insurance_operator, operator.pubkey().to_bytes());
            let amounts = [0, 0, backing_paid, operator_paid, insurer_paid];
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
            assert_eq!(remaining + amounts.iter().sum::<u64>(), TOTAL);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let mut expected = ledger_frames[0].clone();
            let mut ledger = state::read_backing_domain_ledger(&expected.data).unwrap();
            assert_eq!(ledger.authority, provider.pubkey().to_bytes());
            assert_eq!(ledger.market_group, env.market.to_bytes());
            assert_eq!(ledger.domain, 1);
            assert_eq!(ledger.total_deposited_atoms, PRINCIPAL.into());
            ledger.total_principal_atoms = backing.into();
            ledger.total_principal_withdrawn_atoms = backing_paid.into();
            state::write_backing_domain_ledger(&mut expected.data, &ledger).unwrap();
            assert_eq!(env.svm.get_account(&ledgers[0]), Some(expected));
            let mut expected = ledger_frames[1].clone();
            let mut ledger = state::read_insurance_ledger(&expected.data).unwrap();
            assert_eq!(ledger.authority, insurer.pubkey().to_bytes());
            assert_eq!(ledger.market_group, env.market.to_bytes());
            assert_eq!(ledger.total_deposited_atoms, FUNDED.into());
            ledger.total_principal_atoms = insurance.into();
            ledger.total_withdrawn_atoms = (operator_paid + insurer_paid).into();
            ledger.last_observed_insurance_atoms = insurance.into();
            state::write_insurance_ledger(&mut expected.data, &ledger).unwrap();
            assert_eq!(env.svm.get_account(&ledgers[1]), Some(expected));
            crate::support::fuzz_model::assert_market_stock_census(
                "resolution submitter reserve",
                &group,
                &image.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            state::market_view_mut(&mut image.data)
                .unwrap()
                .1
                .validate_shape()
                .unwrap();
        };
        check(&env, false, [0; 3]);
        let insurance_payout = |env: &V16CuEnv, actor, amount| {
            let mut ix = reserve_payout(env, wallets, tokens, ledgers[0], 2, amount);
            ix.accounts[0].pubkey = wallets[actor];
            ix.accounts[0].is_signer = true;
            ix.accounts[2].pubkey = tokens[actor];
            ix.accounts.push(AccountMeta::new(ledgers[1], false));
            ix
        };
        let retained_operator = insurance_payout(&env, 3, LIVE_PAID);
        assert!(retained_operator.accounts[0].is_signer);
        env.payer = operator.insecure_clone();
        let allowed = [env.market, env.vault, tokens[3], ledgers[1]];
        peak = peak.max(land(
            &mut env,
            &[retained_operator.clone()],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(&env, false, [0, LIVE_PAID, 0]);

        env.svm.warp_to_slot(7);
        let resolve = Instruction {
            program_id: env.program_id,
            accounts: if permissionless {
                vec![AccountMeta::new(env.market, false)]
            } else {
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ]
            },
            data: if permissionless {
                ProgInstruction::ResolveStalePermissionless { now_slot: 7 }
            } else {
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: env.market_state().1.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
            }
            .encode(),
        };
        let mut backing_payout = reserve_payout(&env, wallets, tokens, ledgers[0], 0, PRINCIPAL);
        backing_payout
            .accounts
            .push(AccountMeta::new(ledgers[0], false));
        let prefix = [
            resolve,
            backing_payout,
            insurance_payout(&env, 4, TERMINAL_PREFIX),
        ];
        assert_eq!(
            prefix[1..]
                .iter()
                .flat_map(|ix| &ix.accounts)
                .filter(|meta| meta.is_signer)
                .count(),
            1
        );
        let signers = if permissionless {
            vec![&insurer]
        } else {
            vec![&admin, &insurer]
        };
        let mut rejected = prefix.to_vec();
        rejected.push(retained_operator);
        let mut rejected_signers = signers.clone();
        rejected_signers.push(&operator);
        // All three prefix instructions complete, including two SPL transfers and
        // both ledger writes. Rejection must also restore the Live market mode.
        peak = peak.max(land(
            &mut env,
            &rejected,
            &rejected_signers,
            &tracked,
            &[],
            0,
            None,
            Some((5, PercolatorError::Unauthorized)),
        ));
        check(&env, false, [0, LIVE_PAID, 0]);
        let resolve = Instruction {
            program_id: env.program_id,
            accounts: if permissionless {
                vec![AccountMeta::new(env.market, false)]
            } else {
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ]
            },
            data: if permissionless {
                ProgInstruction::ResolveStalePermissionless { now_slot: 7 }
            } else {
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: env.market_state().1.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
            }
            .encode(),
        };
        let mut backing_payout = reserve_payout(&env, wallets, tokens, ledgers[0], 0, PRINCIPAL);
        backing_payout
            .accounts
            .push(AccountMeta::new(ledgers[0], false));
        let prefix = [
            resolve,
            backing_payout,
            insurance_payout(&env, 4, TERMINAL_PREFIX),
        ];
        let allowed = [
            env.market, env.vault, tokens[2], tokens[4], ledgers[0], ledgers[1],
        ];
        peak = peak.max(land(
            &mut env, &prefix, &signers, &tracked, &allowed, 0, None, None,
        ));
        check(&env, true, [PRINCIPAL, LIVE_PAID, TERMINAL_PREFIX]);
        let tail = insurance_payout(&env, 4, FUNDED - LIVE_PAID - TERMINAL_PREFIX);
        let allowed = [env.market, env.vault, tokens[4], ledgers[1]];
        peak = peak.max(land(
            &mut env,
            &[tail],
            &[&insurer],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(&env, true, [PRINCIPAL, LIVE_PAID, FUNDED - LIVE_PAID]);

        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[1], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = market_rent + vault_frame.lamports - tombstone_rent;
        let allowed = [env.market, env.vault];
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
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [0, 0, PRINCIPAL, LIVE_PAID, FUNDED - LIVE_PAID]
        );
        eprintln!("row410 resolution bundle: permissionless={permissionless}, provider=79, operator=7 live only, insurer=124; exact rollback and closure");
    }
    eprintln!("row410 resolution submitter: 2 histories, 2 composed rollbacks, peak={peak} CU");
}
