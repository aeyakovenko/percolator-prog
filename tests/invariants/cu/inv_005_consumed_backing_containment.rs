//! INV-005/020/024/027/055, row416: consumed backing keeps its incumbent role.
//! Unlike cold_admin_earned_reserve, all principal has been repaid and no valid
//! liens remain before management begins. Unlike backing_role_refunding, public
//! PnL conversion leaves a consumed receivable, including after all fees are paid.
//! Both side domains cross expiry and Active/DrainOnly. Authenticated
//! stale maturity rolls back a valid cold-admin rotation; a fresh oracle report
//! restores payout without renewing the expired backing or transferring the role.
//! This is bounded privileged containment, with public System/SPL/wrapper setup.

use super::cold_admin_earned_reserve::{land, wrap};
use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const CAPITAL: [u128; 2] = [52_502, 2_000_000];
const PRINCIPAL: u128 = 100_000;
const RATE: u16 = 3_333;
const PRICE: u64 = 100;
const EXPIRY: u64 = 100;
const DEADLINE: u64 = 112;
const PROFIT: u128 = 5_000;

#[test]
fn v16_program_consumed_backing_role_survives_expiry_stale_clock_and_drain_only() {
    let mut peak = [0; 3]; // rejection, management/payout, owner exit
    for domain in [0u16, 1] {
        let (mark, final_size) = if domain == 1 {
            (105, 1_050)
        } else {
            (95, 1_150)
        };
        let lien = final_size as u128 * mark as u128 / 2 - CAPITAL[0];
        let earned = (lien * RATE as u128).div_ceil(10_000);
        let claims = [CAPITAL[0] + PROFIT - earned, CAPITAL[1] - PROFIT];
        for drain_only in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let cold = Keypair::new();
            let provider = Keypair::new();
            let owners = [Keypair::new(), Keypair::new()];
            let actors = [&owners[0], &owners[1], &provider, &admin, &cold];
            for actor in actors {
                env.ensure_signer_account(actor.pubkey());
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
            env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
            env.configure_permissionless_resolve_with_cu(DEADLINE - 2, 2);
            env.update_backing_fee_policy_with_cu(domain, RATE, 0);
            let wallets = actors.map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            for (destination, amount) in
                wallets.into_iter().zip([CAPITAL[0], CAPITAL[1], PRINCIPAL])
            {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &destination,
                        &admin.pubkey(),
                        &[],
                        amount as u64,
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
            for i in 0..2 {
                env.send(
                    env.deposit_ix(portfolios[i], CAPITAL[i]),
                    vec![
                        AccountMeta::new(owners[i].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(wallets[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[i]],
                )
                .unwrap();
            }
            let ledger_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger_key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger_key.pubkey();
            let sequences = env.control_sequences(0);
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain,
                    market_id: env.asset_market_id(0),
                    authority_epoch: sequences.authority_epoch,
                    intent_id: next_control_sequence(sequences.backing_top_up),
                    amount: PRINCIPAL,
                    expiry_slot: EXPIRY,
                    backing_fee_bps: RATE,
                    insurance_share_bps: 0,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(wallets[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
                &[&provider],
            )
            .unwrap();
            let sign = if domain == 1 { 1 } else { -1 };
            env.trade_asset_with_cu(
                0,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                sign * 1_000 * POS_SCALE as i128,
                PRICE,
                0,
            );
            env.svm.warp_to_slot(2);
            env.push_auth_mark_for_asset_as_admin(0, 2, mark);
            for i in [1, 0] {
                env.crank(
                    portfolios[i],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                );
            }
            for size in [final_size - 1_000, -final_size] {
                env.try_trade_asset_with_backing_fee_cap_with_cu(
                    0,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    sign * size * POS_SCALE as i128,
                    mark,
                    0,
                    RATE,
                )
                .unwrap_or_else(|error| panic!("domain={domain} size={size}: {error}"));
            }
            // Flat positions still need the public crank to release source liens.
            env.crank(
                portfolios[0],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(0),
                },
            );
            env.convert_released_pnl_with_cu(&owners[0], portfolios[0], PROFIT);
            let principal = wrap(
                &env,
                ProgInstruction::WithdrawBackingBucket {
                    domain,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    amount: PRINCIPAL,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(wallets[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
            );
            let market = env.market;
            let vault = env.vault;
            let mut tracked = vec![market, vault, env.mint, env.vault_authority, ledger];
            tracked.extend(wallets);
            tracked.extend(portfolios);
            tracked.extend(actors.map(Signer::pubkey));
            let payout_changed = [market, vault, ledger, wallets[2]];
            land(
                &mut env,
                &[principal],
                &[&provider],
                &tracked,
                &payout_changed,
                None,
            );
            if drain_only {
                env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_DRAIN_ONLY,
                    0,
                    0,
                    0,
                );
            }

            let initial = env.market_state();
            let original_profile =
                state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                    .unwrap();
            let original_sequences = env.control_sequences(0);
            let epoch = original_sequences.authority_epoch;
            let portfolio_frames = portfolios.map(|key| env.svm.get_account(&key));
            let mint_frame = env.svm.get_account(&env.mint);
            let ledger_initial =
                state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data)
                    .unwrap();
            assert_eq!(ledger_initial.total_principal_atoms, 0);
            assert_eq!(ledger_initial.total_deposited_atoms, PRINCIPAL);
            assert_eq!(ledger_initial.total_principal_withdrawn_atoms, PRINCIPAL);
            assert_eq!(ledger_initial.total_earnings_atoms, earned);
            assert_eq!(ledger_initial.total_earnings_withdrawn_atoms, 0);
            assert_eq!(initial.0.last_good_oracle_slot, 2);
            let check_value = |env: &V16CuEnv, paid: u128| {
                let (_, group) = env.market_state();
                let mut expected = initial.1.clone();
                expected.vault -= paid;
                expected.backing_provider_earnings_total -= paid;
                expected.source_backing_buckets[domain as usize].utilization_fee_earnings -= paid;
                assert_eq!(
                    group, expected,
                    "complete economics after management and payout"
                );
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[0].lifecycle,
                    if drain_only {
                        AssetLifecycleV16::DrainOnly
                    } else {
                        AssetLifecycleV16::Active
                    }
                );
                assert_eq!(
                    (
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q
                    ),
                    (0, 0)
                );
                for (index, bucket) in group.source_backing_buckets.iter().enumerate() {
                    assert_eq!(bucket.fresh_unliened_backing_num, 0);
                    assert_eq!(bucket.valid_liened_backing_num, 0);
                    assert_eq!(
                        bucket.consumed_liened_backing_num,
                        if index == domain as usize {
                            PROFIT * BOUND_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(bucket.impaired_liened_backing_num, 0);
                    assert_eq!(
                        bucket.utilization_fee_earnings,
                        if index == domain as usize {
                            earned - paid
                        } else {
                            0
                        }
                    );
                }
                assert_eq!(
                    group.source_backing_buckets[domain as usize].expiry_slot,
                    EXPIRY
                );
                assert_eq!(group.insurance, 0);
                assert_eq!(group.c_tot, claims.iter().sum::<u128>());
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    portfolio_frames
                );
                assert_eq!(env.portfolio_state(portfolios[0]).capital.get(), claims[0]);
                assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), claims[1]);
                for portfolio in portfolios {
                    assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
                }
                assert_eq!(
                    wallets.map(|key| env.token_amount(key) as u128),
                    [0, 0, PRINCIPAL + paid, 0, 0]
                );
                assert_eq!(env.token_amount(vault) as u128, group.vault);
                assert_eq!(
                    group.vault + PRINCIPAL + paid,
                    CAPITAL.iter().sum::<u128>() + PRINCIPAL
                );
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
                assert_eq!(
                    mint.supply as u128,
                    CAPITAL.iter().sum::<u128>() + PRINCIPAL
                );
                assert_eq!(mint.mint_authority, COption::None);
                let mut expected_ledger = ledger_initial;
                expected_ledger.total_earnings_withdrawn_atoms += paid;
                expected_ledger.last_observed_bucket_earnings_atoms -= paid;
                assert_eq!(
                    state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data,)
                        .unwrap(),
                    expected_ledger
                );
                assert_eq!(expected_ledger.authority, provider.pubkey().to_bytes());
            };
            check_value(&env, 0);
            let rotation = |kind, from: Pubkey, to: Pubkey, epoch| {
                wrap(
                    &env,
                    ProgInstruction::UpdateAssetAuthority {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: epoch,
                        kind,
                        new_pubkey: to.to_bytes(),
                    },
                    vec![
                        AccountMeta::new(from, true),
                        AccountMeta::new_readonly(to, true),
                        AccountMeta::new(market, false),
                    ],
                )
            };
            let rotate_admin = rotation(
                processor::ASSET_AUTH_ADMIN,
                admin.pubkey(),
                cold.pubkey(),
                epoch,
            );
            let replace_funded = rotation(
                processor::ASSET_AUTH_BACKING_BUCKET,
                cold.pubkey(),
                cold.pubkey(),
                epoch + 1,
            );
            let consent = rotation(
                processor::ASSET_AUTH_BACKING_BUCKET,
                provider.pubkey(),
                cold.pubkey(),
                epoch + 1,
            );
            let earnings = |amount| {
                wrap(
                    &env,
                    ProgInstruction::WithdrawBackingBucketEarnings {
                        domain,
                        market_id: env.asset_market_id(0),
                        authority_epoch: epoch + 1,
                        amount,
                    },
                    vec![
                        AccountMeta::new(provider.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(ledger, false),
                        AccountMeta::new(wallets[2], false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let partial = earnings(earned - 1);
            let last = earnings(1);
            let refresh = wrap(
                &env,
                ProgInstruction::PushAuthMark {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    now_slot: u64::MAX,
                    mark_e6: mark,
                    observation_sequence: next_control_sequence(
                        original_sequences.oracle_observation,
                    ),
                    authority_epoch: epoch + 1,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
            );

            // Expiry does not erase earnings. Even after a real SPL prefix pays
            // all but one atom, the current cold admin cannot replace the holder.
            env.svm.warp_to_slot(DEADLINE - 1);
            peak[0] = peak[0].max(land(
                &mut env,
                &[
                    rotate_admin.clone(),
                    partial.clone(),
                    replace_funded.clone(),
                ],
                &[&admin, &cold, &provider],
                &tracked,
                &[],
                Some((4, PercolatorError::EngineLockActive, 1)),
            ));
            check_value(&env, 0);
            // Clock maturity independently denies payout and rolls back the
            // authorized rotation, including its authority epoch and profile.
            env.svm.warp_to_slot(DEADLINE);
            peak[0] = peak[0].max(land(
                &mut env,
                &[rotate_admin.clone(), partial.clone()],
                &[&admin, &cold, &provider],
                &tracked,
                &[],
                Some((3, PercolatorError::OracleStale, 0)),
            ));
            check_value(&env, 0);
            peak[1] = peak[1].max(land(
                &mut env,
                &[rotate_admin, refresh, partial],
                &[&admin, &cold, &provider],
                &tracked,
                &payout_changed,
                None,
            ));
            check_value(&env, earned - 1);
            let mut expected_cfg = initial.0;
            expected_cfg.last_good_oracle_slot = DEADLINE;
            assert_eq!(env.market_state().0, expected_cfg);
            let mut expected_sequences = original_sequences;
            expected_sequences.authority_epoch += 1;
            expected_sequences.oracle_observation += 1;
            assert_eq!(env.control_sequences(0), expected_sequences);
            let profile =
                state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                    .unwrap();
            let mut expected_profile = original_profile;
            expected_profile.asset_admin = cold.pubkey().to_bytes();
            expected_profile.last_good_oracle_slot = DEADLINE;
            assert_eq!(profile, expected_profile);
            peak[0] = peak[0].max(land(
                &mut env,
                &[replace_funded.clone()],
                &[&cold],
                &tracked,
                &[],
                Some((2, PercolatorError::EngineLockActive, 0)),
            ));
            check_value(&env, earned - 1);

            // Paying even the last earned atom cannot erase the consumed
            // receivable. The completed SPL payout rolls back with management.
            peak[0] = peak[0].max(land(
                &mut env,
                &[last.clone(), replace_funded.clone()],
                &[&provider, &cold],
                &tracked,
                &[],
                Some((3, PercolatorError::EngineLockActive, 1)),
            ));
            check_value(&env, earned - 1);
            peak[1] = peak[1].max(land(
                &mut env,
                &[last],
                &[&provider],
                &tracked,
                &payout_changed,
                None,
            ));
            check_value(&env, earned);
            assert_eq!(env.control_sequences(0).authority_epoch, epoch + 1);

            // Incumbent consent remains sufficient for the consumed-only role;
            // the historical ledger and all paid value retain their attribution.
            peak[1] = peak[1].max(land(
                &mut env,
                &[consent],
                &[&provider, &cold],
                &tracked,
                &[market],
                None,
            ));
            check_value(&env, earned);
            expected_profile.backing_bucket_authority = cold.pubkey().to_bytes();
            expected_sequences.authority_epoch += 1;
            assert_eq!(env.control_sequences(0), expected_sequences);
            assert_eq!(
                state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0,)
                    .unwrap(),
                expected_profile
            );
            assert_eq!(env.market_state().0, expected_cfg);

            for i in 0..2 {
                let amount = claims[i];
                let ix = wrap(
                    &env,
                    env.withdraw_ix(portfolios[i], amount),
                    vec![
                        AccountMeta::new(owners[i].pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(wallets[i], false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                peak[2] = peak[2].max(land(
                    &mut env,
                    &[ix],
                    &[&owners[i]],
                    &tracked,
                    &[market, vault, portfolios[i], wallets[i]],
                    None,
                ));
                assert_eq!(env.token_amount(wallets[i]) as u128, amount);
                assert_eq!(env.portfolio_state(portfolios[i]).capital.get(), 0);
            }
            assert_eq!(
                wallets.map(|key| env.token_amount(key) as u128),
                [claims[0], claims[1], PRINCIPAL + earned, 0, 0]
            );
            assert_eq!(env.market_state().1.c_tot, 0);
            assert_eq!(env.market_state().1.vault, 0);
            assert_eq!(env.token_amount(vault), 0);
        }
    }
    eprintln!("INV-005 consumed backing containment: worlds=4, exact_rollbacks=16, SPL-prefix rollbacks=8, last-atom payouts=4, owner exits=8; peak CU={peak:?}");
}
