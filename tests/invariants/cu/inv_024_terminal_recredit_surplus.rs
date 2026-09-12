//! INV-024/005/036/081, row410: raw SPL donations do not enlarge spent-insurance
//! recredit. Last-portfolio deletion, expiry, payout, burn and sweep retain this
//! partition through a failed suffix and retry, with a separate cleanup payer.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use terminal_reserve_destination_recovery::land;

const CAPITAL: [u64; 3] = [1_000, 100, 137];
const GAIN: u64 = 10 * (120 - 100);
const SPENT: u64 = GAIN - CAPITAL[1];
const PAYOUTS: [u64; 3] = [CAPITAL[0] + GAIN, 0, CAPITAL[2]];
const SURPLUS: u64 = 83;
const EXPIRY: u64 = 44;

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

#[test]
fn v16_program_terminal_recredit_excludes_raw_surplus_across_cleanup_rollback() {
    let mut peaks = [0; 4]; // user settlement, rejected bundle, retry, final close
    for backing in [61u64, 137] {
        let recovered = backing.min(SPENT).min(CAPITAL[1]);
        let burned = backing - recovered;
        let supply = CAPITAL.iter().sum::<u64>() + SPENT + backing + SURPLUS;
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
        let admin = env.admin.insecure_clone();
        let insurer = Keypair::new();
        let provider = Keypair::new();
        let operator = Keypair::new();
        let donor = Keypair::new();
        for signer in [&insurer, &provider, &operator, &donor] {
            env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
        }
        for (kind, holder) in [
            (processor::ASSET_AUTH_INSURANCE, &insurer),
            (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
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
        for asset in [0, 1] {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }
        env.configure_permissionless_resolve_with_cu(20, 3);
        let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        // Every economic/control identity, the donor and the cleanup payer are distinct.
        let wallets = [
            insurer.pubkey(),
            provider.pubkey(),
            operator.pubkey(),
            donor.pubkey(),
            admin.pubkey(),
            env.payer.pubkey(),
        ];
        let mut identities = wallets
            .into_iter()
            .chain(owners.each_ref().map(Signer::pubkey))
            .collect::<Vec<_>>();
        identities.sort_unstable();
        identities.dedup();
        assert_eq!(identities.len(), 9);
        let role_tokens =
            wallets.map(|wallet| create_ata_for_test(&mut env.svm, &env.payer, wallet, env.mint));
        for (token, amount) in tokens.into_iter().zip(CAPITAL).chain([
            (role_tokens[0], SPENT),
            (role_tokens[1], backing),
            (role_tokens[3], SURPLUS),
        ]) {
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
        assert_eq!((mint.supply, mint.mint_authority), (supply, COption::None));
        for actor in 0..3 {
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
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
        for (instruction, holder, token) in [
            (
                ProgInstruction::TopUpInsuranceDomain {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    amount: SPENT.into(),
                },
                &insurer,
                role_tokens[0],
            ),
            (
                ProgInstruction::TopUpBackingBucket {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: backing.into(),
                    expiry_slot: EXPIRY,
                },
                &provider,
                role_tokens[1],
            ),
        ] {
            env.send(
                instruction,
                vec![
                    AccountMeta::new(holder.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[holder],
            )
            .unwrap();
        }
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            (10 * POS_SCALE) as i128,
            100,
            0,
        );
        for offset in 0..5 {
            let slot = offset + 2;
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(0, slot, 100 + 5 * (offset + 1).min(4));
            env.crank(
                portfolios[2],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        for actor in [0, 1] {
            env.crank(
                portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 6,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(
            env.portfolio_state(portfolios[0]).pnl.get(),
            i128::from(GAIN)
        );
        assert_eq!(
            env.portfolio_state(portfolios[1]).pnl.get(),
            -i128::from(SPENT)
        );
        env.svm.warp_to_slot(40);
        env.send(
            ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
            vec![AccountMeta::new(env.market, false)],
            &[],
        )
        .unwrap();
        assert_eq!(env.market_state().1.resolved_slot, 40);
        env.svm.warp_to_slot(43);
        for actor in [1, 0, 2] {
            for _ in 0..8 {
                if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    break;
                }
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(owners[actor].pubkey(), false),
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
                peaks[0] = peaks[0].max(cu);
                assert_cu_within("row410 recredit user settlement", cu, 400_000);
            }
            assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
            assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
        }
        let delete = |env: &V16CuEnv, portfolio| {
            wrap(
                env,
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new_readonly(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            )
        };
        let tracked = [env.market, env.vault, env.mint]
            .into_iter()
            .chain(wallets)
            .chain(role_tokens)
            .chain(tokens)
            .chain(portfolios)
            .chain(owners.each_ref().map(Signer::pubkey))
            .collect::<Vec<_>>();
        for actor in [1, 2] {
            let ix = delete(&env, portfolios[actor]);
            let rent_before = env.svm.get_account(&env.market).unwrap().lamports;
            let rent = env.svm.get_account(&portfolios[actor]).unwrap().lamports;
            let allowed = [env.market, portfolios[actor]];
            land(
                &mut env,
                &[ix],
                &[&admin],
                &tracked,
                &allowed,
                0,
                None,
                None,
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                rent_before + rent
            );
        }
        let before_donation = env.svm.get_account(&env.market);
        let donation = spl_token::instruction::transfer(
            &spl_token::ID,
            &role_tokens[3],
            &env.vault,
            &donor.pubkey(),
            &[],
            SURPLUS,
        )
        .unwrap();
        let allowed = [env.vault, role_tokens[3]];
        land(
            &mut env,
            &[donation],
            &[&donor],
            &tracked,
            &allowed,
            0,
            None,
            None,
        );
        assert_eq!(env.svm.get_account(&env.market), before_donation);
        let config = env.market_state().0;
        let terminal = env.market_state().1;
        let profiles = [0, 1].map(|asset| {
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, asset)
                .unwrap()
        });
        let sequences = [0, 1].map(|asset| env.control_sequences(asset));
        let token_frames = tokens.map(|token| env.svm.get_account(&token).unwrap());
        let role_frames = role_tokens.map(|token| env.svm.get_account(&token).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let check = |env: &V16CuEnv, paid: bool| {
            let group = env.market_state().1;
            let amount = if paid { recovered } else { 0 };
            assert_eq!(env.market_state().0, config);
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num
                ),
                (0, 0, 0)
            );
            assert_eq!(group.materialized_portfolio_count, u64::from(!paid));
            assert_eq!(group.vault, u128::from(backing - amount));
            assert_eq!(group.insurance, 0);
            assert_eq!(
                group.insurance_domain_budget,
                vec![u128::from(SPENT - amount), 0, 0, 0]
            );
            assert_eq!(group.insurance_domain_spent, group.insurance_domain_budget);
            assert_eq!(group.insurance_domain_budget_remaining_total, 0);
            assert_eq!(group.backing_provider_earnings_total, 0);
            let fresh = if paid {
                0
            } else {
                u128::from(backing) * BOUND_SCALE
            };
            assert_eq!(
                group.source_backing_buckets[0].fresh_unliened_backing_num,
                fresh
            );
            assert_eq!(group.source_credit[0].fresh_reserved_backing_num, fresh);
            assert_eq!(
                group.source_backing_buckets[0].status,
                if paid {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(
                group.source_credit[0].credit_epoch,
                terminal.source_credit[0].credit_epoch + u64::from(paid)
            );
            assert_eq!(
                group.source_credit[1].provider_receivable_num,
                u128::from(CAPITAL[1]) * BOUND_SCALE
            );
            for domain in 1..4 {
                assert_eq!(group.source_credit[domain], terminal.source_credit[domain]);
                assert_eq!(
                    group.source_backing_buckets[domain],
                    terminal.source_backing_buckets[domain]
                );
            }
            for asset in [0, 1] {
                assert_eq!(env.control_sequences(asset), sequences[asset]);
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        asset
                    )
                    .unwrap(),
                    profiles[asset]
                );
            }
            for (token, frame) in tokens.into_iter().zip(&token_frames) {
                assert_eq!(env.svm.get_account(&token), Some(frame.clone()));
            }
            for ((token, frame), amount) in role_tokens
                .into_iter()
                .zip(&role_frames)
                .zip([amount, 0, 0, 0, 0, 0])
                .chain(std::iter::once((
                    (env.vault, &vault_frame),
                    backing - amount + SURPLUS,
                )))
            {
                let mut expected = frame.clone();
                let mut token_state = TokenAccount::unpack(&expected.data).unwrap();
                token_state.amount = amount;
                TokenAccount::pack(token_state, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(&token), Some(expected));
            }
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(
                PAYOUTS.iter().sum::<u64>() + amount + env.token_amount(env.vault),
                supply
            );
            let accounts = if paid {
                vec![]
            } else {
                vec![env.portfolio_state(portfolios[0])]
            };
            // The shared census requires reconciled custody; the donation is an
            // independently fixed external balance, not an engine stock.
            let custody = u128::from(env.token_amount(env.vault));
            assert_eq!(custody.checked_sub(group.vault), Some(SURPLUS.into()));
            crate::support::fuzz_model::assert_market_stock_census(
                "row410 raw surplus excludes recredit",
                &group,
                &env.svm.get_account(&env.market).unwrap().data,
                &accounts,
                custody.checked_sub(SURPLUS.into()).unwrap(),
            )
            .unwrap();
        };
        check(&env, false);
        let deletion = delete(&env, portfolios[0]);
        let close = wrap(
            &env,
            ProgInstruction::CloseSlab {
                authority_epoch: sequences[0].authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(role_tokens[4], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
        );
        let withdrawal = wrap(
            &env,
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: sequences[0].authority_epoch,
                amount: recovered.into(),
            },
            vec![
                AccountMeta::new_readonly(insurer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(role_tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        assert!(withdrawal.accounts.iter().all(|meta| !meta.is_signer));
        // Reserve holders and users supply no signatures for any cleanup continuation.
        drop((insurer, provider, operator, owners, donor));
        env.svm.warp_to_slot(EXPIRY);
        let prefix = [deletion, close.clone(), withdrawal];
        let rejected = prefix
            .iter()
            .cloned()
            .chain([close.clone(), close.clone()])
            .collect::<Vec<_>>();
        peaks[1] = peaks[1].max(land(
            &mut env,
            &rejected,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((6, PercolatorError::InvalidAccountLen)),
        ));
        check(&env, false);
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let last_rent = env.svm.get_account(&portfolios[0]).unwrap().lamports;
        let allowed = [env.market, portfolios[0], env.vault, role_tokens[0]];
        peaks[2] = peaks[2].max(land(
            &mut env,
            &prefix,
            &[&admin],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        check(&env, true);
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_rent + last_rent
        );
        assert!(env
            .svm
            .get_account(&portfolios[0])
            .map_or(true, |account| account.lamports == 0
                && account.data.is_empty()));
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = market_rent + last_rent + vault_frame.lamports - tombstone_rent;
        let allowed = [env.market, env.vault, env.mint, role_tokens[4]];
        peaks[3] = peaks[3].max(land(
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
            .map_or(true, |account| account.lamports == 0
                && account.data.is_empty()
                && account.owner == solana_sdk::system_program::ID));
        let mut expected_mint = mint_frame;
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        mint.supply = supply - burned;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
        assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
        let mut expected_admin_token = role_frames[4].clone();
        let mut token = TokenAccount::unpack(&expected_admin_token.data).unwrap();
        token.amount = SURPLUS;
        TokenAccount::pack(token, &mut expected_admin_token.data).unwrap();
        assert_eq!(
            env.svm.get_account(&role_tokens[4]),
            Some(expected_admin_token)
        );
        assert_eq!(
            role_tokens.map(|token| env.token_amount(token)),
            [recovered, 0, 0, 0, SURPLUS, 0]
        );
        assert_eq!(
            PAYOUTS.iter().sum::<u64>() + recovered + SURPLUS + burned,
            supply
        );
        eprintln!("row410 recredit/surplus: backing={backing}, spent={SPENT}, recovered={recovered}, burned={burned}, admin_surplus={SURPLUS}, payer_quote=0");
    }
    for peak in peaks {
        assert_cu_within("row410 recredit/surplus", peak, 400_000);
    }
    eprintln!("row410 recredit/surplus: 2 histories, 2 exact final-suffix rollbacks; peak CU [settlement, rejected bundle, retry, final close]={peaks:?}");
}
