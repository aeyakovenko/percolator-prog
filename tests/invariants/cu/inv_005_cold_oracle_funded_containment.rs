//! Row 416: cold-admin oracle replacement without the funded coholder's signature.
//! An accepted observation transfers no backing rights; a funded-role suffix must
//! restore the executed oracle update, observation, and unrelated SPL payout.

use super::*;

const USER_PREFIX: u128 = 7;

#[test]
fn v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix() {
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for pushed_mark in [100u64, 103] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let incumbent = Keypair::new();
            let cold = Keypair::new();
            let incoming = Keypair::new();
            let user = Keypair::new();
            let actors = [&incumbent, &cold, &incoming, &user, &admin];
            for actor in actors {
                env.ensure_signer_account(actor.pubkey());
            }
            for (kind, holder) in [
                (processor::ASSET_AUTH_BACKING_BUCKET, &incumbent),
                (processor::ASSET_AUTH_ORACLE, &incumbent),
                (processor::ASSET_AUTH_ADMIN, &cold),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    asset,
                    kind,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            env.svm.warp_to_slot(1);
            for index in [0u16, 1] {
                env.configure_auth_mark_for_asset_with_authority(
                    index,
                    if index == asset { &incumbent } else { &admin },
                    1,
                    100,
                );
            }
            let wallets = actors.map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            for (wallet, amount) in [
                (wallets[0], PRINCIPAL.iter().sum::<u128>()),
                (wallets[3], CAPITAL),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallet,
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
            for (side, amount) in PRINCIPAL.into_iter().enumerate() {
                let seq = env.control_sequences(asset as usize);
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain: asset * 2 + side as u16,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.backing_top_up),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount,
                        expiry_slot: 10_000,
                    },
                    vec![
                        AccountMeta::new(incumbent.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallets[0], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&incumbent],
                )
                .unwrap();
            }
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
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&user],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio, CAPITAL),
                vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&user],
            )
            .unwrap();

            let market = env.market;
            let mut tracked = vec![market, env.mint, env.vault, env.vault_authority, portfolio];
            tracked.extend(wallets);
            tracked.extend(actors.map(Signer::pubkey));
            let peer = (1 - asset) as usize;
            let peer_profile = profile(&env, peer);
            let peer_sequences = env.control_sequences(peer);
            let peer_asset = env.market_state().1.assets[peer];
            let assert_stock = |env: &V16CuEnv, remaining: [u128; 2], user_paid: u128| {
                let (cfg, group) = env.market_state();
                let backing = remaining.iter().sum::<u128>();
                let paid = PRINCIPAL.iter().sum::<u128>() - backing;
                assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[asset as usize].lifecycle,
                    AssetLifecycleV16::Active
                );
                assert_eq!(group.assets[peer], peer_asset);
                assert_eq!(profile(env, peer), peer_profile);
                assert_eq!(env.control_sequences(peer), peer_sequences);
                assert_eq!(group.c_tot, CAPITAL - user_paid);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert_eq!(group.insurance, 0);
                assert!(group
                    .insurance_domain_budget
                    .iter()
                    .all(|value| *value == 0));
                for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                    let amount = if domain / 2 == asset as usize {
                        remaining[domain % 2]
                    } else {
                        0
                    };
                    assert_eq!(bucket.fresh_unliened_backing_num, amount * BOUND_SCALE);
                    assert_eq!(bucket.valid_liened_backing_num, 0);
                    assert_eq!(bucket.consumed_liened_backing_num, 0);
                    assert_eq!(bucket.impaired_liened_backing_num, 0);
                    assert_eq!(bucket.utilization_fee_earnings, 0);
                }
                let owner = env.portfolio_state(portfolio);
                assert_eq!(owner.owner, user.pubkey().to_bytes());
                assert_eq!(owner.capital.get(), CAPITAL - user_paid);
                assert_eq!(owner.pnl.get(), 0);
                let expected_wallets = [paid, 0, 0, user_paid, 0];
                assert_eq!(
                    wallets.map(|key| env.token_amount(key) as u128),
                    expected_wallets
                );
                let vault = backing + CAPITAL - user_paid;
                assert_eq!(group.vault, vault);
                assert_eq!(env.token_amount(env.vault) as u128, vault);
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(mint.mint_authority, COption::None);
                assert_eq!(
                    mint.supply as u128,
                    PRINCIPAL.iter().sum::<u128>() + CAPITAL
                );
                assert_eq!(
                    mint.supply as u128,
                    vault + expected_wallets.iter().sum::<u128>()
                );
            };
            assert_stock(&env, PRINCIPAL, 0);
            let mut expected_profile = profile(&env, asset as usize);
            let mut expected_sequences = env.control_sequences(asset as usize);
            assert_eq!(expected_profile.asset_admin, cold.pubkey().to_bytes());
            assert_eq!(
                expected_profile.oracle_authority,
                incumbent.pubkey().to_bytes()
            );
            assert_eq!(
                expected_profile.backing_bucket_authority,
                incumbent.pubkey().to_bytes()
            );
            let handoff = |kind, epoch| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(cold.pubkey(), true),
                    AccountMeta::new(incoming.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch,
                    kind,
                    new_pubkey: incoming.pubkey().to_bytes(),
                }
                .encode(),
            };
            let user_exit = |env: &V16CuEnv, amount| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(portfolio, amount).encode(),
            };
            let epoch = expected_sequences.authority_epoch;
            let oracle = handoff(processor::ASSET_AUTH_ORACLE, epoch);
            let funded = handoff(processor::ASSET_AUTH_BACKING_BUCKET, epoch + 1);
            let observation = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(incoming.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::PushAuthMark {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch + 1,
                    observation_sequence: next_control_sequence(
                        expected_sequences.oracle_observation,
                    ),
                    now_slot: u64::MAX,
                    mark_e6: pushed_mark,
                }
                .encode(),
            };
            let prefix = [user_exit(&env, USER_PREFIX), oracle, observation];
            let bundle = [prefix.to_vec(), vec![funded]].concat();
            env.svm.warp_to_slot(2);

            // No incumbent signature: the cold admin and incoming oracle are valid
            // management signers, but neither owns the two positive backing buckets.
            let meta = land(
                &mut env,
                &bundle,
                &[&user, &cold, &incoming],
                &tracked,
                &[],
                Some((5, PercolatorError::EngineLockActive)),
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            for (program, count) in [(env.program_id, 3), (spl_token::ID, 1)] {
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count
                );
            }
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_stock(&env, PRINCIPAL, 0);

            // Reuse the exact successful instruction prefix after rollback. The
            // oracle changes without incumbent consent while its funded role stays.
            let custody_changes = [market, portfolio, env.vault, wallets[3]];
            let meta = land(
                &mut env,
                &prefix,
                &[&user, &cold, &incoming],
                &tracked,
                &custody_changes,
                None,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            expected_profile.oracle_authority = incoming.pubkey().to_bytes();
            expected_profile.oracle_target_price_e6 = pushed_mark;
            if pushed_mark != 100 {
                expected_profile.mark_ewma_e6 = pushed_mark;
                expected_profile.mark_ewma_last_slot = 2;
                expected_profile.funding_mark_pending_e6 = pushed_mark;
                expected_profile.funding_mark_pending_slot = 2;
            }
            expected_profile.last_good_oracle_slot = 2;
            expected_sequences.authority_epoch += 1;
            expected_sequences.oracle_observation =
                next_control_sequence(expected_sequences.oracle_observation);
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_stock(&env, PRINCIPAL, USER_PREFIX);

            let mut remaining = PRINCIPAL;
            for side in 0..2 {
                let ix = withdrawal(
                    &env,
                    incumbent.pubkey(),
                    wallets[0],
                    asset * 2 + side as u16,
                    remaining[side],
                    epoch + 1,
                );
                let changed = [market, env.vault, wallets[0]];
                let meta = land(&mut env, &[ix], &[&incumbent], &tracked, &changed, None);
                assert!(meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)));
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                remaining[side] = 0;
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), expected_sequences);
                assert_stock(&env, remaining, USER_PREFIX);
            }
            let ix = user_exit(&env, CAPITAL - USER_PREFIX);
            let meta = land(&mut env, &[ix], &[&user], &tracked, &custody_changes, None);
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            assert_stock(&env, [0, 0], CAPITAL);
        }
    }
    eprintln!("INV-005 cold oracle funded containment: 4 live worlds, 4 exact SPL/oracle/observation rollbacks, 4 cold-signed oracle replacements, 8 incumbent backing payouts, peak {peak_cu} CU");
}
