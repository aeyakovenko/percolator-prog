//! Row 424 / INV-070: scanner rediscovery with native custody and external surplus.
//! SyncNative before expiry or after rediscovery must not enlarge booked recovery.
//! Public construction only; no pending receipts, optional insurance ledger or fees.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const BACKING: u64 = 61;
const RAW: u64 = 53;

fn native_frame(empty: &Account, amount: u64, unsynced: u64) -> Account {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.is_native, COption::Some(empty.lamports));
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + unsynced;
    expected
}

#[test]
fn v16_program_native_terminal_scan_excludes_synced_surplus_from_rediscovered_insurance() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;

    let lock = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
    let mut peaks = [0u64; 4]; // User cleanup, scanner, rejected transactions, custody/close.
    let mut commits = 0;
    let mut rollbacks = 0;
    let mut outcomes = Vec::new();
    for side in 0..2 {
        for sync_before_expiry in [false, true] {
            let mut env = inv081_public_native_market_with_params(
                2,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            env.portfolio_account_len = state::portfolio_account_len_for_market_slots(2).unwrap();
            let admin = env.admin.insecure_clone();
            let beneficiary = Keypair::new();
            env.svm
                .airdrop(&beneficiary.pubkey(), 1_000_000_000)
                .unwrap();
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&beneficiary),
                0,
                processor::ASSET_AUTH_INSURANCE,
                beneficiary.pubkey().to_bytes(),
            )
            .unwrap();
            env.svm.warp_to_slot(1);
            for asset in [0, 1] {
                env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            }
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
            let tokens = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            let reserve =
                create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), env.mint);
            let destination =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            let empty_tokens = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let empty_reserve = env.svm.get_account(&reserve).unwrap();
            let empty_destination = env.svm.get_account(&destination).unwrap();
            for (owner, token, amount) in owners
                .iter()
                .zip(tokens)
                .zip(CAPITAL)
                .map(|((owner, token), amount)| (owner, token, amount))
                .chain([
                    (&beneficiary, reserve, SPENT),
                    (&admin, destination, BACKING),
                ])
            {
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::transfer(&owner.pubkey(), &token, amount),
                        spl_token::instruction::sync_native(&spl_token::ID, &token).unwrap(),
                    ],
                    &[owner],
                )
                .unwrap();
            }
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
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: side as u16,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    amount: SPENT.into(),
                },
                vec![
                    AccountMeta::new(beneficiary.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(reserve, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&beneficiary],
            )
            .unwrap();
            env.top_up_backing_bucket_from_admin_token_with_cu(
                destination,
                (2 + side) as u16,
                BACKING.into(),
                EXPIRY,
            );
            env.trade_asset_with_cu(
                0,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                (10 * POS_SCALE) as i128 * if side == 0 { 1 } else { -1 },
                100,
                0,
            );
            for offset in 0..5 {
                let slot = offset + 2;
                let delta = 5 * (offset + 1).min(4);
                env.svm.warp_to_slot(slot);
                env.push_auth_mark_for_asset_as_admin(
                    0,
                    slot,
                    if side == 0 { 100 + delta } else { 100 - delta },
                );
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
            assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 200);
            assert_eq!(
                env.portfolio_state(portfolios[1]).pnl.get(),
                -i128::from(SPENT)
            );
            env.svm.warp_to_slot(40);
            env.resolve();
            env.svm.warp_to_slot(EXPIRY - 1);
            for actor in [1, 0, 2] {
                for _ in 0..8 {
                    if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                        break;
                    }
                    env.svm.expire_blockhash();
                    peaks[0] = peaks[0].max(
                        env.send(
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
                        .unwrap(),
                    );
                }
                assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
                assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
                env.send(
                    env.close_portfolio_ix(portfolios[actor]),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
            }

            let sequences = env.control_sequences(0);
            let sibling_sequences = env.control_sequences(1);
            let initial_ledger = env.market_state().1.resolved_payout_ledger;
            let mint_before = env.svm.get_account(&env.mint);
            let beneficiary_before = env.svm.get_account(&beneficiary.pubkey());
            let close_at = |authority_epoch| {
                wrap(
                    &env,
                    ProgInstruction::CloseSlab { authority_epoch },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                )
            };
            let close = close_at(sequences.authority_epoch);
            let final_close = close_at(sequences.authority_epoch + 1);
            let payout = wrap(
                &env,
                ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: sequences.authority_epoch,
                    amount: BACKING.into(),
                },
                vec![
                    AccountMeta::new_readonly(beneficiary.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(reserve, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let beneficiary_key = beneficiary.pubkey();
            drop(beneficiary);
            let mut tracked = vec![
                env.market,
                env.vault,
                env.mint,
                reserve,
                destination,
                admin.pubkey(),
                beneficiary_key,
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(tokens);
            tracked.extend(portfolios);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let total_lamports = |env: &V16CuEnv| {
                tracked
                    .iter()
                    .filter_map(|key| env.svm.get_account(key))
                    .map(|account| account.lamports)
                    .sum::<u64>()
            };
            let initial_lamports = total_lamports(&env);
            let market_only = [env.market];
            let custody = [env.market, env.vault, reserve];
            let sync = spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap();
            let bad = Instruction {
                program_id: system_program::ID,
                accounts: Vec::new(),
                data: vec![255],
            };
            let mut send = |env: &mut V16CuEnv,
                            ixs: &[Instruction],
                            signers: &[&Keypair],
                            changed: &[Pubkey],
                            rejection: Option<(u8, InstructionError)>,
                            successes,
                            category: usize| {
                if rejection.is_some() {
                    rollbacks += 1;
                } else {
                    commits += 1;
                }
                peaks[category] = peaks[category].max(land(
                    env, ixs, signers, &tracked, changed, rejection, successes,
                ));
            };
            let check = |env: &V16CuEnv,
                         normalized: bool,
                         restored: u64,
                         paid: u64,
                         cursor: u128,
                         donated: bool,
                         synced: bool| {
                let (cfg, group) = env.market_state();
                let market = env.svm.get_account(&env.market).unwrap();
                let header = market_group_header_bytes(&market.data);
                let fresh = if normalized {
                    0
                } else {
                    u128::from(BACKING) * BOUND_SCALE
                };
                assert_eq!(cfg.terminal_slab_scan_progress, cursor);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.vault, u128::from(BACKING - paid));
                assert_eq!(group.insurance, u128::from(restored - paid));
                assert_eq!(group.backing_provider_earnings_total, 0);
                for domain in 0..4 {
                    let source = group.source_credit[domain];
                    let bucket = group.source_backing_buckets[domain];
                    let expected_fresh = if domain == 2 + side { fresh } else { 0 };
                    assert_eq!(bucket.fresh_unliened_backing_num, expected_fresh);
                    assert_eq!(source.fresh_reserved_backing_num, expected_fresh);
                    assert_eq!(
                        (
                            bucket.valid_liened_backing_num,
                            bucket.utilization_fee_earnings
                        ),
                        (0, 0)
                    );
                    assert_eq!(
                        (
                            source.valid_liened_backing_num,
                            source.positive_claim_bound_num
                        ),
                        (0, 0)
                    );
                    assert_eq!(
                        source.provider_receivable_num,
                        if domain == 1 - side {
                            u128::from(CAPITAL[1]) * BOUND_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        group.insurance_domain_budget[domain],
                        if domain == side {
                            u128::from(SPENT - paid)
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        group.insurance_domain_spent[domain],
                        if domain == side {
                            u128::from(SPENT - restored)
                        } else {
                            0
                        }
                    );
                }
                assert!(group
                    .assets
                    .iter()
                    .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
                assert_eq!(group.source_backing_buckets[2 + side].expiry_slot, EXPIRY);
                assert_eq!(
                    group.source_backing_buckets[2 + side].status,
                    if normalized {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
                assert_eq!(
                    header.insurance_domain_budget_remaining_total.get(),
                    u128::from(restored - paid)
                );
                let residual = group
                    .vault
                    .checked_sub(group.insurance + fresh / BOUND_SCALE)
                    .unwrap();
                let recoverable = (group.source_credit[1 - side].provider_receivable_num
                    / BOUND_SCALE)
                    .min(group.insurance_domain_spent[side])
                    .min(residual);
                assert_eq!(
                    group.insurance + recoverable,
                    if normalized {
                        u128::from(BACKING - paid)
                    } else {
                        0
                    }
                );
                let mut ledger = initial_ledger;
                if normalized {
                    ledger.snapshot_residual += u128::from(BACKING);
                }
                assert_eq!(group.resolved_payout_ledger, ledger);
                let mut expected_sequences = sequences;
                expected_sequences.authority_epoch += u64::from(paid != 0);
                assert_eq!(env.control_sequences(0), expected_sequences);
                assert_eq!(env.control_sequences(1), sibling_sequences);
                let wrapped = if synced { RAW } else { 0 };
                let unsynced = if donated && !synced { RAW } else { 0 };
                assert_eq!(
                    env.svm.get_account(&env.vault),
                    Some(native_frame(
                        &empty_vault,
                        BACKING - paid + wrapped,
                        unsynced
                    ))
                );
                assert_eq!(
                    env.svm.get_account(&reserve),
                    Some(native_frame(&empty_reserve, paid, 0))
                );
                assert_eq!(
                    env.svm.get_account(&destination),
                    Some(empty_destination.clone())
                );
                for actor in 0..3 {
                    assert_eq!(
                        env.svm.get_account(&tokens[actor]),
                        Some(native_frame(&empty_tokens[actor], PAYOUTS[actor], 0))
                    );
                    assert!(env
                        .svm
                        .get_account(&portfolios[actor])
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                }
                assert_eq!(env.svm.get_account(&env.mint), mint_before);
                assert_eq!(env.svm.get_account(&beneficiary_key), beneficiary_before);
                assert_eq!(total_lamports(env), initial_lamports);
                assert_eq!(
                    tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                        + env.token_amount(reserve)
                        + env.token_amount(env.vault),
                    CAPITAL.iter().sum::<u64>() + SPENT + BACKING + wrapped
                );
                assert_market_stock_census(
                    "native scanner rediscovery",
                    &group,
                    &market.data,
                    &[],
                    u128::from(env.token_amount(env.vault) - wrapped),
                )
                .unwrap();
                assert_reservation_encumbrance_census("native scanner rediscovery", &group, &[])
                    .unwrap();
            };

            check(&env, false, 0, 0, 0, false, false);
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
                1,
            );
            check(&env, false, 0, 0, 1, false, false);
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            expected_admin.lamports -= RAW;
            let donation = system_instruction::transfer(&admin.pubkey(), &env.vault, RAW);
            let vault_only = [env.vault];
            send(
                &mut env,
                &[donation],
                &[&admin],
                &[admin.pubkey(), vault_only[0]],
                None,
                (0, 0),
                3,
            );
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            check(&env, false, 0, 0, 1, true, false);
            if sync_before_expiry {
                send(&mut env, &[sync.clone()], &[], &vault_only, None, (0, 1), 3);
            }
            check(&env, false, 0, 0, 1, true, sync_before_expiry);
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &[],
                Some((2, lock.clone())),
                (0, 0),
                2,
            );
            send(
                &mut env,
                &[payout.clone()],
                &[],
                &[],
                Some((2, lock.clone())),
                (0, 0),
                2,
            );
            let before_expiry = env.svm.get_account(&env.market).unwrap();
            env.svm.warp_to_slot(EXPIRY);
            assert_eq!(
                env.svm.get_account(&env.market),
                Some(before_expiry.clone())
            );

            // Both scanner steps and the native transfer execute before the rejected suffix.
            send(
                &mut env,
                &[close.clone(), close.clone(), payout.clone(), bad.clone()],
                &[&admin],
                &[],
                Some((5, InstructionError::InvalidInstructionData)),
                (3, 1),
                2,
            );
            check(&env, false, 0, 0, 1, true, sync_before_expiry);
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
                1,
            );
            check(&env, true, 0, 0, 0, true, sync_before_expiry);
            let slot_start =
                MARKET_GROUP_OFF + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
            let slot_end = slot_start
                + std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
            assert_eq!(
                &env.svm.get_account(&env.market).unwrap().data[slot_start..slot_end],
                &before_expiry.data[slot_start..slot_end]
            );
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
                1,
            );
            check(&env, true, BACKING, 0, 0, true, sync_before_expiry);
            if !sync_before_expiry {
                send(&mut env, &[sync], &[], &vault_only, None, (0, 1), 3);
            }
            check(&env, true, BACKING, 0, 0, true, true);
            send(
                &mut env,
                &[close],
                &[&admin],
                &[],
                Some((2, lock.clone())),
                (0, 0),
                2,
            );
            send(
                &mut env,
                &[payout.clone(), bad.clone()],
                &[],
                &[],
                Some((3, InstructionError::InvalidInstructionData)),
                (1, 1),
                2,
            );
            check(&env, true, BACKING, 0, 0, true, true);
            send(&mut env, &[payout.clone()], &[], &custody, None, (1, 1), 3);
            check(&env, true, BACKING, BACKING, 0, true, true);
            send(
                &mut env,
                &[payout],
                &[],
                &[],
                // Only the 53-atom external surplus remains; custody preflight precedes epochs.
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32),
                )),
                (0, 0),
                2,
            );

            let market_before = env.svm.get_account(&env.market).unwrap();
            let mut admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            send(
                &mut env,
                &[final_close.clone(), bad],
                &[&admin],
                &[],
                Some((3, InstructionError::InvalidInstructionData)),
                (1, 2),
                2,
            );
            check(&env, true, BACKING, BACKING, 0, true, true);
            let closing = [env.market, env.vault, destination, admin.pubkey()];
            send(
                &mut env,
                &[final_close],
                &[&admin],
                &closing,
                None,
                (1, 2),
                3,
            );
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            admin_before.lamports +=
                market_before.lamports + empty_vault.lamports - tombstone.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(admin_before));
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0));
            assert_eq!(
                env.svm.get_account(&destination),
                Some(native_frame(&empty_destination, RAW, 0))
            );
            assert_eq!(
                env.svm.get_account(&reserve),
                Some(native_frame(&empty_reserve, BACKING, 0))
            );
            assert_eq!(env.svm.get_account(&env.mint), mint_before);
            assert_eq!(total_lamports(&env), initial_lamports);
            outcomes.push((
                tokens.map(|key| env.token_amount(key)),
                env.token_amount(reserve),
                env.token_amount(destination),
                tombstone.lamports,
            ));
            println!("native scanner: side={side}, sync_before_expiry={sync_before_expiry}, cursor=1->0, restored={BACKING}, surplus={RAW}");
        }
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!((commits, rollbacks), (28, 28));
    for peak in peaks {
        assert_cu_within("native scanner rediscovery", peak, 400_000);
    }
    println!("INV-070 native scanner: 4 histories, {commits} commits, {rollbacks} exact rollbacks, 4 rediscoveries; peak CU [user cleanup, scanner, rejection, custody/close]={peaks:?}");
}
