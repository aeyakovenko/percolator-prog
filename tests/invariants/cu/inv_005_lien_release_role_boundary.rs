//! INV-005, row416: the last impaired lien gates cold takeover even after close.
//! Public normalization opens the empty role; restored coholders and re-funding
//! close it again without an authority-epoch change. Positive claims survive.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const REFILL: u128 = 13;
const SUPPLY: u128 = WINNER_DEPOSIT + COUNTERPARTY_DEPOSIT + BYSTANDER_DEPOSIT + BACKING;

fn profile(env: &V16CuEnv, asset: u16) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(
        &env.svm.get_account(&env.market).unwrap().data,
        asset as usize,
    )
    .unwrap()
}

fn handoff(
    env: &V16CuEnv,
    asset: u16,
    from: Pubkey,
    to: Pubkey,
    kind: u8,
    epoch: u64,
) -> Instruction {
    wrap(
        env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind,
            new_pubkey: to.to_bytes(),
        },
        vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to, true),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn report(env: &V16CuEnv, asset: u16, owner: Pubkey, epoch: u64, mark: u64) -> Instruction {
    wrap(
        env,
        ProgInstruction::PushAuthMark {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            observation_sequence: next_control_sequence(
                env.control_sequences(asset as usize).oracle_observation,
            ),
            now_slot: EXPIRY,
            mark_e6: mark,
        },
        vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn exit(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    wallet: Pubkey,
    amount: u128,
) -> Instruction {
    wrap(
        env,
        env.withdraw_ix(portfolio, amount),
        vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

fn backing(
    env: &V16CuEnv,
    domain: u16,
    owner: Pubkey,
    wallet: Pubkey,
    amount: u128,
    withdraw: bool,
) -> Instruction {
    let seq = env.control_sequences((domain / 2) as usize);
    let ix = if withdraw {
        ProgInstruction::WithdrawBackingBucket {
            domain,
            market_id: env.asset_market_id(domain / 2),
            authority_epoch: seq.authority_epoch,
            amount,
        }
    } else {
        ProgInstruction::TopUpBackingBucket {
            domain,
            market_id: env.asset_market_id(domain / 2),
            authority_epoch: seq.authority_epoch,
            intent_id: next_control_sequence(seq.backing_top_up),
            amount,
            expiry_slot: 100,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
        }
    };
    let mut accounts = vec![
        AccountMeta::new(owner, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(wallet, false),
        AccountMeta::new(env.vault, false),
    ];
    if withdraw {
        accounts.push(AccountMeta::new_readonly(env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    wrap(env, ix, accounts)
}

fn assert_role_stock(env: &V16CuEnv, funded_domain: usize, fresh: u128) {
    let group = env.market_state().1;
    for domain in [funded_domain & !1, funded_domain | 1] {
        let bucket = group.source_backing_buckets[domain];
        assert_eq!(
            bucket.fresh_unliened_backing_num,
            if domain == funded_domain {
                fresh * BOUND_SCALE
            } else {
                0
            }
        );
        assert_eq!(bucket.valid_liened_backing_num, 0);
        assert_eq!(bucket.impaired_liened_backing_num, 0);
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert_eq!(bucket.utilization_fee_earnings, 0);
    }
}

#[test]
fn v16_program_lien_retirement_reopens_cold_role_takeover_only_until_refunding() {
    let mut peak = [0; 3]; // public setup/progress, rejected bundles, committed bundles
    let mut rollbacks = 0;
    for asset in [0u16, 1] {
        for winner_long in [false, true] {
            let LienedWorld {
                mut env,
                provider,
                cold,
                owners,
                wallets,
                portfolios,
            } = liened_world(winner_long, asset, &mut peak);
            let admin = env.admin.insecure_clone();
            let direction = if winner_long { 1 } else { -1 };
            let mark = if winner_long { 106 } else { 94 };
            let domain = (2 * asset + u16::from(winner_long)) as usize;
            let market = env.market;
            let vault = env.vault;
            for (kind, holder) in [
                (processor::ASSET_AUTH_ORACLE, &provider),
                (processor::ASSET_AUTH_ADMIN, &cold),
            ] {
                peak[0] = peak[0].max(
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(holder),
                        asset,
                        kind,
                        holder.pubkey().to_bytes(),
                    )
                    .unwrap(),
                );
            }
            peak[0] = peak[0].max(env.push_auth_mark_for_asset_with_authority(
                asset,
                &provider,
                2,
                if winner_long { 105 } else { 95 },
            ));
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let token_frames = wallets
                .iter()
                .chain(std::iter::once(&vault))
                .map(|key| (*key, env.svm.get_account(key).unwrap()))
                .collect::<Vec<_>>();
            let program_frames = std::iter::once(market)
                .chain(portfolios)
                .map(|key| (key, env.svm.get_account(&key).unwrap()))
                .collect::<Vec<_>>();
            let mut tracked = vec![market, vault, env.mint, env.vault_authority];
            tracked.extend(wallets);
            tracked.extend(portfolios);
            tracked.extend([provider.pubkey(), cold.pubkey(), admin.pubkey()]);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let book =
                |env: &V16CuEnv, amounts: [u128; 6], user_debit: u128, profit: u128, flat: bool| {
                    let group = env.market_state().1;
                    let accounts = portfolios.map(|key| env.portfolio_state(key));
                    assert_eq!(group.mode, MarketModeV16::Live);
                    for index in [0, 1] {
                        assert_eq!(group.assets[index].lifecycle, AssetLifecycleV16::Active);
                    }
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                    let mint = Mint::unpack(&mint_frame.data).unwrap();
                    assert_eq!(mint.supply as u128, SUPPLY);
                    assert_eq!(mint.mint_authority, COption::None);
                    let custody = SUPPLY - amounts.iter().sum::<u128>();
                    assert_eq!(group.vault, custody);
                    for ((key, frame), amount) in token_frames
                        .iter()
                        .zip(amounts.into_iter().chain([custody]))
                    {
                        let mut expected = frame.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = amount as u64;
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(
                            env.svm.get_account(key).unwrap(),
                            expected,
                            "exact SPL custody image"
                        );
                    }
                    for (key, frame) in &program_frames {
                        let after = env.svm.get_account(key).unwrap();
                        let mut expected = frame.clone();
                        expected.data.clone_from(&after.data);
                        assert_eq!(after.data.len(), frame.data.len());
                        assert_eq!(after, expected, "program Account metadata");
                    }
                    for index in 0..3 {
                        assert_eq!(accounts[index].owner, owners[index].pubkey().to_bytes());
                    }
                    for (index, capital) in [WINNER_DEPOSIT, COUNTERPARTY_DEPOSIT]
                        .into_iter()
                        .enumerate()
                    {
                        let sign = if index == 0 { 1 } else { -1 };
                        assert_eq!(
                            accounts[index].capital.get() as i128 + accounts[index].pnl.get(),
                            capital as i128 + sign * (profit as i128 - 50)
                        );
                        for (leg, size) in [
                            (asset, WINNING_SIZE_Q),
                            (1 - asset, ADVERSE_SIZE_Q + RISK_INCREASE_Q),
                        ] {
                            if flat {
                                assert!(!has_active_leg_for_asset(&accounts[index], leg as usize));
                            } else {
                                assert_eq!(
                                    active_leg_for_asset(&accounts[index], leg as usize)
                                        .basis_pos_q,
                                    sign * direction * size
                                );
                            }
                        }
                    }
                    assert_eq!(accounts[2].capital.get(), BYSTANDER_DEPOSIT - user_debit);
                    let local_lien = accounts
                        .iter()
                        .flat_map(|a| &a.source_domains)
                        .filter(|s| s.is_occupied() && s.domain.get() as usize == domain)
                        .map(|s| s.source_lien_counterparty_backing_num.get())
                        .sum::<u128>();
                    let bucket = group.source_backing_buckets[domain];
                    assert_eq!(
                        bucket.valid_liened_backing_num + bucket.impaired_liened_backing_num,
                        local_lien
                    );
                    assert_market_stock_census(
                        "lien retirement / role boundary",
                        &group,
                        &env.svm.get_account(&market).unwrap().data,
                        &accounts,
                        env.token_amount(vault) as u128,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(
                        "lien retirement / local attribution",
                        &group,
                        &accounts,
                    )
                    .unwrap();
                };
            let mut amounts = [0; 6];
            book(&env, amounts, 0, 100, false);
            let initial_mark = if winner_long { 105 } else { 95 };
            let adverse_mark = if winner_long { 95 } else { 105 };
            let loss = (ADVERSE_SIZE_Q / POS_SCALE as i128) as u128 * 5;
            let initial_requirement = ((WINNING_SIZE_Q / POS_SCALE as i128) as u128 * initial_mark
                + ((ADVERSE_SIZE_Q + RISK_INCREASE_Q) / POS_SCALE as i128) as u128 * adverse_mark)
                / 10;
            let lien = (initial_requirement - (WINNER_DEPOSIT - loss)) * BOUND_SCALE;
            assert_eq!(
                env.market_state().1.source_backing_buckets[domain].valid_liened_backing_num,
                lien
            );
            let epoch = env.control_sequences(asset as usize).authority_epoch;
            let takeover = |env: &V16CuEnv, epoch| {
                vec![
                    exit(env, owners[2].pubkey(), portfolios[2], wallets[2], 1),
                    handoff(
                        env,
                        asset,
                        cold.pubkey(),
                        cold.pubkey(),
                        processor::ASSET_AUTH_ORACLE,
                        epoch,
                    ),
                    handoff(
                        env,
                        asset,
                        cold.pubkey(),
                        cold.pubkey(),
                        processor::ASSET_AUTH_BACKING_BUCKET,
                        epoch + 1,
                    ),
                ]
            };
            let request = takeover(&env, epoch);
            peak[1] = peak[1].max(land(
                &mut env,
                &request,
                &[&owners[2], &cold],
                &tracked,
                &[],
                Some((4, PercolatorError::EngineLockActive, 1)),
            ));
            rollbacks += 1;
            expire_lien(
                &mut env,
                portfolios,
                winner_long,
                asset,
                &provider,
                &mut peak,
            );
            let impaired = env.market_state().1.source_backing_buckets[domain];
            assert_eq!(impaired.impaired_liened_backing_num, lien);
            assert!(lien > 0);
            assert_eq!(impaired.fresh_unliened_backing_num, 0);
            assert_eq!(impaired.valid_liened_backing_num, 0);
            assert_eq!(impaired.consumed_liened_backing_num, 0);
            assert_eq!(impaired.utilization_fee_earnings, 0);
            peak[1] = peak[1].max(land(
                &mut env,
                &request,
                &[&owners[2], &cold],
                &tracked,
                &[],
                Some((4, PercolatorError::EngineLockActive, 1)),
            ));
            rollbacks += 1;
            for (leg, size, price) in [
                (
                    1 - asset,
                    ADVERSE_SIZE_Q + RISK_INCREASE_Q,
                    if winner_long { 95 } else { 105 },
                ),
                (asset, WINNING_SIZE_Q, mark),
            ] {
                peak[0] = peak[0].max(env.trade_asset_with_cu(
                    leg,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    -direction * size,
                    price,
                    0,
                ));
            }
            book(&env, amounts, 0, 120, true);
            assert!(
                env.market_state().1.source_backing_buckets[domain].impaired_liened_backing_num > 0,
                "closing exposure alone must not be mistaken for lien retirement"
            );
            peak[1] = peak[1].max(land(
                &mut env,
                &request,
                &[&owners[2], &cold],
                &tracked,
                &[],
                Some((4, PercolatorError::EngineLockActive, 1)),
            ));
            rollbacks += 1;

            let crank = wrap(
                &env,
                ProgInstruction::PermissionlessCrank {
                    now_slot: EXPIRY,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolios[0], false),
                ],
            );
            let before_profile = profile(&env, asset);
            let before_seq = env.control_sequences(asset as usize);
            let peer_profile = profile(&env, 1 - asset);
            let peer_seq = env.control_sequences((1 - asset) as usize);
            let mut prefix = vec![
                request[0].clone(),
                crank,
                request[1].clone(),
                request[2].clone(),
            ];
            prefix.push(report(&env, asset, provider.pubkey(), epoch + 2, mark));
            peak[1] = peak[1].max(land(
                &mut env,
                &prefix,
                &[&owners[2], &cold, &provider],
                &tracked,
                &[],
                Some((6, PercolatorError::Unauthorized, 1)),
            ));
            rollbacks += 1;
            book(&env, amounts, 0, 120, true);
            prefix.pop();
            peak[2] = peak[2].max(land(
                &mut env,
                &prefix,
                &[&owners[2], &cold],
                &tracked,
                &[market, vault, wallets[2], portfolios[2], portfolios[0]],
                None,
            ));
            amounts[2] = 1;
            let mut user_debit = 1;
            book(&env, amounts, user_debit, 120, true);
            assert_role_stock(&env, domain ^ 1, 0);
            let mut expected_profile = before_profile;
            expected_profile.oracle_authority = cold.pubkey().to_bytes();
            expected_profile.backing_bucket_authority = cold.pubkey().to_bytes();
            let mut expected_seq = before_seq;
            expected_seq.authority_epoch += 2;
            assert_eq!(profile(&env, asset), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_seq);
            let normalized = env.market_state();
            assert_eq!(
                normalized.1.source_credit[domain].exact_positive_claim_num,
                120 * BOUND_SCALE
            );
            let restoration = [
                handoff(
                    &env,
                    asset,
                    cold.pubkey(),
                    provider.pubkey(),
                    processor::ASSET_AUTH_ORACLE,
                    epoch + 2,
                ),
                handoff(
                    &env,
                    asset,
                    cold.pubkey(),
                    provider.pubkey(),
                    processor::ASSET_AUTH_BACKING_BUCKET,
                    epoch + 3,
                ),
            ];
            peak[2] = peak[2].max(land(
                &mut env,
                &restoration,
                &[&cold, &provider],
                &tracked,
                &[market],
                None,
            ));
            assert_eq!(env.market_state(), normalized);
            assert_eq!(profile(&env, asset), before_profile);
            expected_seq.authority_epoch += 2;
            assert_eq!(env.control_sequences(asset as usize), expected_seq);

            let donate = exit(&env, owners[2].pubkey(), portfolios[2], wallets[2], REFILL);
            peak[2] = peak[2].max(land(
                &mut env,
                &[donate],
                &[&owners[2]],
                &tracked,
                &[market, vault, portfolios[2], wallets[2]],
                None,
            ));
            amounts[2] += REFILL;
            user_debit += REFILL;
            book(&env, amounts, user_debit, 120, true);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &wallets[2],
                    &wallets[3],
                    &owners[2].pubkey(),
                    &[],
                    REFILL as u64,
                )
                .unwrap(),
                &[&owners[2]],
            )
            .unwrap();
            amounts[2] -= REFILL;
            amounts[3] += REFILL;
            book(&env, amounts, user_debit, 120, true);
            // Prevalidate after the donation consumes its portfolio sequence.
            // Re-funding changes the stock gate, not the role epoch or these
            // instruction bytes. Each later submission is freshly signed.
            let retained = takeover(&env, epoch + 4);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let tx = Transaction::new_signed_with_payer(
                &[vec![heap_ix(), cu_ix()], retained.clone()].concat(),
                Some(&env.payer.pubkey()),
                &[&env.payer, &owners[2], &cold],
                env.svm.latest_blockhash(),
            );
            tx.verify().unwrap();
            let mut keys = tx.message.account_keys.clone();
            keys.extend_from_slice(&tracked);
            let frame = keys
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>();
            let simulated = env.svm.simulate_transaction(tx.into()).unwrap();
            peak[2] = peak[2].max(simulated.compute_units_consumed);
            assert_eq!(
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                frame
            );
            // The opposite side has no positive claim demand. Its independently
            // withdrawable principal still protects the same asset-wide role.
            assert_eq!(
                env.market_state().1.source_credit[domain ^ 1].positive_claim_bound_num,
                0
            );
            let refill = backing(
                &env,
                (domain ^ 1) as u16,
                provider.pubkey(),
                wallets[3],
                REFILL,
                false,
            );
            peak[2] = peak[2].max(land(
                &mut env,
                &[refill],
                &[&provider],
                &tracked,
                &[market, vault, wallets[3]],
                None,
            ));
            amounts[3] = 0;
            expected_seq.backing_top_up += 1;
            assert_eq!(env.control_sequences(asset as usize), expected_seq);
            assert_role_stock(&env, domain ^ 1, REFILL);
            book(&env, amounts, user_debit, 120, true);
            for remaining in [REFILL, 1] {
                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                assert_eq!(env.control_sequences(asset as usize), expected_seq);
                peak[1] = peak[1].max(land(
                    &mut env,
                    &retained,
                    &[&owners[2], &cold],
                    &tracked,
                    &[],
                    Some((4, PercolatorError::EngineLockActive, 1)),
                ));
                rollbacks += 1;
                book(&env, amounts, user_debit, 120, true);
                let pay = backing(
                    &env,
                    (domain ^ 1) as u16,
                    provider.pubkey(),
                    wallets[3],
                    if remaining == 1 { 1 } else { REFILL - 1 },
                    true,
                );
                peak[2] = peak[2].max(land(
                    &mut env,
                    &[pay],
                    &[&provider],
                    &tracked,
                    &[market, vault, wallets[3]],
                    None,
                ));
                amounts[3] = if remaining == 1 { REFILL } else { REFILL - 1 };
                assert_role_stock(&env, domain ^ 1, REFILL - amounts[3]);
                assert_eq!(env.control_sequences(asset as usize), expected_seq);
                book(&env, amounts, user_debit, 120, true);
            }
            peak[2] = peak[2].max(land(
                &mut env,
                &retained,
                &[&owners[2], &cold],
                &tracked,
                &[market, vault, portfolios[2], wallets[2]],
                None,
            ));
            amounts[2] += 1;
            user_debit += 1;
            expected_seq.authority_epoch += 2;
            assert_eq!(env.control_sequences(asset as usize), expected_seq);
            assert_eq!(profile(&env, asset), expected_profile);
            let current_report = report(
                &env,
                asset,
                cold.pubkey(),
                expected_seq.authority_epoch,
                mark,
            );
            let finish = exit(
                &env,
                owners[2].pubkey(),
                portfolios[2],
                wallets[2],
                BYSTANDER_DEPOSIT - user_debit,
            );
            peak[2] = peak[2].max(land(
                &mut env,
                &[current_report, finish],
                &[&cold, &owners[2]],
                &tracked,
                &[market, vault, portfolios[2], wallets[2]],
                None,
            ));
            amounts[2] = BYSTANDER_DEPOSIT - REFILL;
            book(&env, amounts, BYSTANDER_DEPOSIT, 120, true);
            assert_role_stock(&env, domain ^ 1, 0);
            assert_eq!(
                env.market_state().1.source_credit[domain].exact_positive_claim_num,
                120 * BOUND_SCALE
            );
            expected_seq.oracle_observation += 1;
            assert_eq!(env.control_sequences(asset as usize), expected_seq);
            assert_eq!(profile(&env, 1 - asset), peer_profile);
            assert_eq!(env.control_sequences((1 - asset) as usize), peer_seq);
        }
    }
    assert_eq!(rollbacks, 24);
    assert_cu_within("lien boundary setup/progress", peak[0], 750_000);
    assert_cu_within("lien boundary rollback", peak[1], 600_000);
    assert_cu_within("lien boundary continuation", peak[2], 600_000);
    eprintln!("INV-005 lien retirement role boundary: worlds=4, exact SPL-prefix rollbacks={rollbacks}, normalization rollbacks=4, empty takeovers=8, coholder restorations=4, refunds=4, peak CU [setup/progress, rollback, continuation]={peak:?}");
}
