//! INV-073 / row420: independent absent providers share terminal custody without
//! coupling their signatures or erasing the other domain's unpaid earnings.
//! Two public asset cohorts earn unequal fees; both payout orders include exact
//! rollback of a successful payment prefix followed by premature slab closure.
//! One SPL rail, fresh backing, two domains and available cleanup owners only.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_distinct_provider_disposition() {
    const RATES: [u16; 2] = [3_333, 6_666];
    const FEES: [u64; 2] = [875, 1_749];
    const TOTAL_SUPPLY: u64 = 2 * (CAPITAL[0] + CAPITAL[1] + BACKING);
    const LIMIT: u64 = 600_000;
    let mut peak = 0;
    let mut user_calls = 0;
    for order in [[0usize, 1], [1, 0]] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_margin_bps: 5_000,
                maintenance_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let providers = [Keypair::new(), Keypair::new()];
        let users = std::array::from_fn::<_, 4, _>(|_| Keypair::new());
        let provider_keys = providers.each_ref().map(Signer::pubkey);
        let user_keys = users.each_ref().map(Signer::pubkey);
        for key in provider_keys.into_iter().chain(user_keys) {
            env.svm.airdrop(&key, 1_000_000_000).unwrap();
        }
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 1, 100);
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&providers[asset]),
                asset as u16,
                processor::ASSET_AUTH_BACKING_BUCKET,
                provider_keys[asset].to_bytes(),
            )
            .unwrap();
            env.update_backing_fee_policy_with_cu((2 * asset + 1) as u16, RATES[asset], 0);
            assert_eq!(
                FEES[asset],
                ((1_050 * 105 / 2 - CAPITAL[0]) * u64::from(RATES[asset])).div_ceil(10_000)
            );
        }
        env.configure_permissionless_resolve_with_cu(100, 5);
        let user_tokens =
            user_keys.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
        let provider_tokens =
            provider_keys.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        for (token, amount) in user_tokens
            .into_iter()
            .enumerate()
            .map(|(i, key)| (key, CAPITAL[i % 2]))
            .chain(provider_tokens.map(|key| (key, BACKING)))
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
        assert_eq!(Mint::unpack(&mint_frame.data).unwrap().supply, TOTAL_SUPPLY);
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
        for actor in 0..4 {
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL[actor % 2].into()),
                vec![
                    AccountMeta::new(user_keys[actor], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(user_tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&users[actor]],
            )
            .unwrap();
        }
        for asset in 0..2 {
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain: (2 * asset + 1) as u16,
                    market_id: env.asset_market_id(asset as u16),
                    authority_epoch: env.control_sequences(asset).authority_epoch,
                    intent_id: 0,
                    backing_fee_bps: RATES[asset],
                    insurance_share_bps: 0,
                    amount: BACKING.into(),
                    expiry_slot: 100,
                },
                vec![
                    AccountMeta::new(provider_keys[asset], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(provider_tokens[asset], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&providers[asset]],
            )
            .unwrap();
        }
        // No provider key survives funding, including during fee generation and user exit.
        assert!(!provider_keys.contains(&admin.pubkey()));
        assert!(!provider_keys.contains(&env.payer.pubkey()));
        assert_ne!(provider_keys[0], provider_keys[1]);
        drop(providers);
        for asset in 0..2 {
            let a = 2 * asset;
            env.trade_asset_with_cu(
                asset as u16,
                &users[a],
                portfolios[a],
                &users[a + 1],
                portfolios[a + 1],
                1_000 * POS_SCALE as i128,
                100,
                0,
            );
        }
        env.svm.warp_to_slot(2);
        for asset in 0..2 {
            env.push_auth_mark_for_asset_as_admin(asset as u16, 2, 105);
            for actor in [2 * asset + 1, 2 * asset] {
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(asset as u16),
                    },
                );
            }
            let a = 2 * asset;
            env.try_trade_asset_with_backing_fee_cap_with_cu(
                asset as u16,
                &users[a],
                portfolios[a],
                &users[a + 1],
                portfolios[a + 1],
                50 * POS_SCALE as i128,
                105,
                0,
                RATES[asset],
            )
            .unwrap();
            assert_eq!(
                env.market_state().1.source_backing_buckets[2 * asset + 1].utilization_fee_earnings,
                FEES[asset].into()
            );
        }
        env.resolve();
        env.svm.warp_to_slot(7);
        let ledgers = [0, 1].map(|_| {
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            key.pubkey()
        });
        let tracked = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            admin_token,
        ]
        .into_iter()
        .chain(user_keys)
        .chain(provider_keys)
        .chain(user_tokens)
        .chain(provider_tokens)
        .chain(portfolios)
        .chain(ledgers)
        .collect::<Vec<_>>();
        for _ in 0..8 {
            for actor in [1, 0, 3, 2] {
                if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    continue;
                }
                let before = [env.market, portfolios[actor]].map(|key| env.svm.get_account(&key));
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(user_keys[actor], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(user_tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                    .encode(),
                };
                let allowed = [env.market, env.vault, portfolios[actor], user_tokens[actor]];
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
                assert_ne!(
                    [env.market, portfolios[actor]].map(|key| env.svm.get_account(&key)),
                    before
                );
                assert_eq!(
                    user_tokens
                        .into_iter()
                        .chain(provider_tokens)
                        .chain([env.vault])
                        .map(|key| env.token_amount(key))
                        .sum::<u64>(),
                    TOTAL_SUPPLY
                );
                user_calls += 1;
            }
            if portfolios
                .iter()
                .all(|key| resolved_portfolio_is_terminal(&env, *key))
            {
                break;
            }
        }
        for actor in 0..4 {
            assert!(
                resolved_portfolio_is_terminal(&env, portfolios[actor]),
                "bounded unsigned user disposition"
            );
            let expected = if actor % 2 == 0 {
                CAPITAL[0] + PROFIT - FEES[actor / 2]
            } else {
                CAPITAL[1] - PROFIT
            };
            assert_eq!(env.token_amount(user_tokens[actor]), expected);
            env.close_portfolio_with_cu(&users[actor], portfolios[actor]);
        }
        drop(users);
        let profiles = [0, 1].map(|asset| {
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, asset)
                .unwrap()
        });
        let sequences = [0, 1].map(|asset| env.control_sequences(asset));
        let empty_ledgers = ledgers.map(|key| env.svm.get_account(&key).unwrap());
        let custody_keys = user_tokens
            .into_iter()
            .chain(provider_tokens)
            .chain([env.vault, admin_token])
            .collect::<Vec<_>>();
        let custody_frames = custody_keys
            .iter()
            .map(|key| env.svm.get_account(key).unwrap())
            .collect::<Vec<_>>();
        let payout = |env: &V16CuEnv, asset: usize, earnings: bool, amount: u64| {
            let mut accounts = vec![
                AccountMeta::new_readonly(provider_keys[asset], false),
                AccountMeta::new(env.market, false),
            ];
            if earnings {
                accounts.push(AccountMeta::new(ledgers[asset], false));
            }
            accounts.extend([
                AccountMeta::new(provider_tokens[asset], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ]);
            let domain = (2 * asset + 1) as u16;
            let market_id = env.asset_market_id(asset as u16);
            let authority_epoch = sequences[asset].authority_epoch;
            let data = if earnings {
                ProgInstruction::WithdrawBackingBucketEarnings {
                    domain,
                    market_id,
                    authority_epoch,
                    amount: amount.into(),
                }
            } else {
                ProgInstruction::WithdrawBackingBucket {
                    domain,
                    market_id,
                    authority_epoch,
                    amount: amount.into(),
                }
            };
            Instruction {
                program_id: env.program_id,
                accounts,
                data: data.encode(),
            }
        };
        let check = |env: &V16CuEnv, principal_paid: [u64; 2], fees_paid: [u64; 2]| {
            let group = env.market_state().1;
            let remaining = 2 * BACKING + FEES.iter().sum::<u64>()
                - principal_paid.iter().sum::<u64>()
                - fees_paid.iter().sum::<u64>();
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(group.source_claim_bound_total_num, 0);
            assert_eq!(group.insurance, 0);
            assert_eq!(group.vault, remaining.into());
            assert_eq!(
                group.backing_provider_earnings_total,
                u128::from(FEES.iter().sum::<u64>() - fees_paid.iter().sum::<u64>())
            );
            let amounts = [
                CAPITAL[0] + PROFIT - FEES[0],
                CAPITAL[1] - PROFIT,
                CAPITAL[0] + PROFIT - FEES[1],
                CAPITAL[1] - PROFIT,
                principal_paid[0] + fees_paid[0],
                principal_paid[1] + fees_paid[1],
                remaining,
                0,
            ];
            assert_eq!(amounts.iter().sum::<u64>(), TOTAL_SUPPLY);
            for ((key, frame), amount) in custody_keys.iter().zip(&custody_frames).zip(amounts) {
                let mut expected = frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amount;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(key), Some(expected));
            }
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let mut image = env.svm.get_account(&env.market).unwrap();
            for asset in 0..2 {
                assert_eq!(
                    state::read_asset_oracle_profile(&image.data, asset).unwrap(),
                    profiles[asset]
                );
                assert_eq!(
                    profiles[asset].backing_bucket_authority,
                    provider_keys[asset].to_bytes()
                );
                assert_eq!(env.control_sequences(asset), sequences[asset]);
                let domain = 2 * asset + 1;
                let bucket = group.source_backing_buckets[domain];
                assert_eq!(
                    bucket.status,
                    if principal_paid[asset] == BACKING {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    },
                    "asset={asset}, principal_paid={principal_paid:?}"
                );
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    u128::from(BACKING - principal_paid[asset]) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[domain].fresh_reserved_backing_num,
                    bucket.fresh_unliened_backing_num
                );
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(FEES[asset] - fees_paid[asset])
                );
                if fees_paid[asset] == 0 {
                    assert_eq!(
                        env.svm.get_account(&ledgers[asset]),
                        Some(empty_ledgers[asset].clone())
                    );
                } else {
                    let record = state::read_backing_domain_ledger(
                        &env.svm.get_account(&ledgers[asset]).unwrap().data,
                    )
                    .unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.domain, domain as u16);
                    assert_eq!(record.authority, provider_keys[asset].to_bytes());
                    assert_eq!(
                        record.total_earnings_withdrawn_atoms,
                        fees_paid[asset].into()
                    );
                    assert_eq!(
                        record.last_observed_bucket_earnings_atoms,
                        u128::from(FEES[asset] - fees_paid[asset])
                    );
                }
            }
            state::market_view_mut(&mut image.data)
                .unwrap()
                .1
                .validate_shape()
                .unwrap();
            assert_market_stock_census(
                "distinct absent providers",
                &group,
                &image.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            assert_reservation_encumbrance_census("distinct absent providers", &group, &[])
                .unwrap();
        };
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
                authority_epoch: sequences[0].authority_epoch,
            }
            .encode(),
        };
        let mut principal_paid = [0; 2];
        let mut fees_paid = [0; 2];
        check(&env, principal_paid, fees_paid);
        for asset in order {
            let ix = payout(&env, asset, false, BACKING);
            let allowed = [env.market, env.vault, provider_tokens[asset]];
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
            principal_paid[asset] = BACKING;
            check(&env, principal_paid, fees_paid);
        }
        let first = order[0];
        let second = order[1];
        let pay_first = payout(&env, first, true, FEES[first]);
        let allowed = [
            env.market,
            env.vault,
            provider_tokens[first],
            ledgers[first],
        ];
        peak = peak.max(land(
            &mut env,
            &[pay_first],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        fees_paid[first] = FEES[first];
        check(&env, principal_paid, fees_paid);
        // One fully paid domain cannot release the other provider's last fee atom.
        let partial = payout(&env, second, true, FEES[second] - 1);
        peak = peak.max(land(
            &mut env,
            &[partial.clone(), close.clone()],
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((3, PercolatorError::EngineLockActive)),
        ));
        check(&env, principal_paid, fees_paid);
        let allowed = [
            env.market,
            env.vault,
            provider_tokens[second],
            ledgers[second],
        ];
        peak = peak.max(land(
            &mut env,
            &[partial],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        fees_paid[second] = FEES[second] - 1;
        check(&env, principal_paid, fees_paid);
        let last = payout(&env, second, true, 1);
        peak = peak.max(land(
            &mut env,
            &[last],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        fees_paid[second] += 1;
        check(&env, principal_paid, fees_paid);
        let ledger_frames = ledgers.map(|key| env.svm.get_account(&key));
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = env.svm.get_account(&env.market).unwrap().lamports
            + env.svm.get_account(&env.vault).unwrap().lamports
            - rent;
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
        assert_eq!(tombstone.lamports, rent);
        assert!(env
            .svm
            .get_account(&env.vault)
            .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
        assert_eq!(ledgers.map(|key| env.svm.get_account(&key)), ledger_frames);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        assert_eq!(
            provider_tokens.map(|key| env.token_amount(key)),
            FEES.map(|fees| BACKING + fees)
        );
    }
    assert_cu_within("distinct absent providers", peak, LIMIT);
    println!("row420 distinct absent providers: worlds=2, user_calls={user_calls}, reserve_payments=10, exact_rollbacks=2, slab_closes=2, peak_CU={peak}, limit={LIMIT}");
}
