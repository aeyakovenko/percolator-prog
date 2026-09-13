//! INV-073 / row420: redeeming a paid native prefix cannot pin the unpaid claim.
//! A funded wrapped-SOL destination is closed publicly before its provider key is
//! dropped. Keeper-funded recreation and unsigned principal payment then exhaust
//! custody, independently of native value wrapped by destination initialization.
//! Market-authority participation in resolution and mechanical closure is retained.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_native_provider_redemption() {
    const PRINCIPAL: u64 = 401;
    const REDEEMED: u64 = 101;
    const EXTRA: u64 = 19;
    const LIMIT: u64 = 150_000;
    let mut peak = 0;
    for prefunded in [false, true] {
        for bundled in [false, true] {
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let provider = Keypair::new();
            let provider_key = provider.pubkey();
            assert_ne!(provider_key, admin.pubkey());
            assert_ne!(provider_key, env.payer.pubkey());
            env.svm.airdrop(&provider_key, 1_000_000_000).unwrap();
            peak = peak.max(env.init_market_cu);
            peak = peak.max(
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(&provider),
                    0,
                    processor::ASSET_AUTH_BACKING_BUCKET,
                    provider_key.to_bytes(),
                )
                .unwrap(),
            );
            let destination = create_ata_for_test(&mut env.svm, &env.payer, provider_key, env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_destination = env.svm.get_account(&destination).unwrap();
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            // These are expected Account images only, never installed into LiteSVM.
            let native_account = |empty: &solana_sdk::account::Account, amount: u64| {
                let mut expected = empty.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                assert_eq!(token.mint, spl_token::native_mint::ID);
                assert_eq!(token.is_native, COption::Some(rent));
                assert_eq!((token.amount, expected.lamports), (0, rent));
                token.amount = amount;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                expected.lamports += amount;
                expected
            };
            peak = peak.max(
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::transfer(&provider_key, &destination, PRINCIPAL),
                        spl_token::instruction::sync_native(&spl_token::ID, &destination).unwrap(),
                    ],
                    &[&provider],
                )
                .unwrap(),
            );
            env.svm.warp_to_slot(1);
            let sequences = env.control_sequences(0);
            let market_id = env.asset_market_id(0);
            peak = peak.max(
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain: 1,
                        market_id,
                        authority_epoch: sequences.authority_epoch,
                        intent_id: 0,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: PRINCIPAL.into(),
                        expiry_slot: 100,
                    },
                    vec![
                        AccountMeta::new(provider_key, true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&provider],
                )
                .unwrap(),
            );
            peak = peak.max(env.resolve());
            let sequences = env.control_sequences(0);
            let resolved_market = env.svm.get_account(&env.market).unwrap();
            let profile = state::read_asset_oracle_profile(&resolved_market.data, 0).unwrap();
            assert_eq!(profile.backing_bucket_authority, provider_key.to_bytes());
            let tracked = [
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                provider_key,
                destination,
                admin.pubkey(),
                admin_token,
            ];
            let market_key = env.market;
            let vault_key = env.vault;
            let payout = |amount: u64| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(provider_key, false),
                    AccountMeta::new(market_key, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(vault_key, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::WithdrawBackingBucket {
                    domain: 1,
                    market_id,
                    authority_epoch: sequences.authority_epoch,
                    amount: amount.into(),
                }
                .encode(),
            };
            let prefix = payout(REDEEMED);
            let remainder = payout(PRINCIPAL - REDEEMED);
            let stock = |env: &V16CuEnv, unpaid: u64| {
                let market = env.svm.get_account(&market_key).unwrap();
                assert_eq!(market.lamports, resolved_market.lamports);
                let (_, group) = state::read_market(&market.data).unwrap();
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(group.vault, unpaid.into());
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.insurance, 0);
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                let bucket = group.source_backing_buckets[1];
                let source = group.source_credit[1];
                assert_eq!(bucket.expiry_slot, if unpaid == 0 { 0 } else { 100 });
                assert_eq!(
                    bucket.status,
                    if unpaid == 0 {
                        BackingBucketStatusV16::Empty
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    u128::from(unpaid) * BOUND_SCALE
                );
                assert_eq!(
                    source.fresh_reserved_backing_num,
                    u128::from(unpaid) * BOUND_SCALE
                );
                assert_eq!(
                    (
                        bucket.valid_liened_backing_num,
                        bucket.consumed_liened_backing_num
                    ),
                    (0, 0)
                );
                assert_eq!(
                    (source.provider_receivable_num, source.spent_backing_num),
                    (0, 0)
                );
                assert_eq!(
                    env.svm.get_account(&vault_key),
                    Some(native_account(&empty_vault, unpaid))
                );
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                assert_market_stock_census(
                    "native provider redemption",
                    &group,
                    &market.data,
                    &[],
                    unpaid.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("native provider redemption", &group, &[])
                    .unwrap();
            };
            stock(&env, PRINCIPAL);
            peak = peak.max(land(
                &mut env,
                &[prefix],
                &[],
                &tracked,
                &[market_key, vault_key, destination],
                0,
                None,
                None,
            ));
            stock(&env, PRINCIPAL - REDEEMED);
            assert_eq!(
                env.svm.get_account(&destination),
                Some(native_account(&empty_destination, REDEEMED))
            );
            let mut redeemed_wallet = env.svm.get_account(&provider_key).unwrap();
            redeemed_wallet.lamports += rent + REDEEMED;
            let redeem = spl_token::instruction::close_account(
                &spl_token::ID,
                &destination,
                &provider_key,
                &provider_key,
                &[],
            )
            .unwrap();
            peak = peak.max(land(
                &mut env,
                &[redeem],
                &[&provider],
                &tracked,
                &[destination],
                0,
                Some((provider_key, rent + REDEEMED)),
                None,
            ));
            drop(provider);
            assert!(env
                .svm
                .get_account(&destination)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(
                env.svm.get_account(&provider_key),
                Some(redeemed_wallet.clone())
            );
            stock(&env, PRINCIPAL - REDEEMED);
            let mut calls = 0;
            let wrapped = if prefunded { EXTRA } else { 0 };
            if prefunded {
                let fund =
                    system_instruction::transfer(&env.payer.pubkey(), &destination, rent + EXTRA);
                peak = peak.max(land(
                    &mut env,
                    &[fund],
                    &[],
                    &tracked,
                    &[destination],
                    rent + EXTRA,
                    None,
                    None,
                ));
                calls += 1;
                let account = env.svm.get_account(&destination).unwrap();
                assert_eq!(account.owner, solana_sdk::system_program::ID);
                assert!(account.data.is_empty());
                assert_eq!(account.lamports, rent + EXTRA);
                stock(&env, PRINCIPAL - REDEEMED);
            }
            let repair = Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(destination, false),
                    AccountMeta::new_readonly(provider_key, false),
                    AccountMeta::new_readonly(env.mint, false),
                    AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: vec![1],
            };
            let repair_rent = if prefunded { 0 } else { rent };
            if !bundled {
                peak = peak.max(land(
                    &mut env,
                    &[repair.clone()],
                    &[],
                    &tracked,
                    &[destination],
                    repair_rent,
                    None,
                    None,
                ));
                calls += 1;
                stock(&env, PRINCIPAL - REDEEMED);
                assert_eq!(
                    env.svm.get_account(&destination),
                    Some(native_account(&empty_destination, wrapped))
                );
            }
            let continuation = if bundled {
                vec![repair, remainder]
            } else {
                vec![remainder]
            };
            peak = peak.max(land(
                &mut env,
                &continuation,
                &[],
                &tracked,
                &[market_key, vault_key, destination],
                if bundled { repair_rent } else { 0 },
                None,
                None,
            ));
            calls += 1;
            stock(&env, 0);
            let paid_destination =
                native_account(&empty_destination, PRINCIPAL - REDEEMED + wrapped);
            assert_eq!(
                env.svm.get_account(&destination),
                Some(paid_destination.clone())
            );
            assert_eq!(
                REDEEMED + env.token_amount(destination),
                PRINCIPAL + wrapped
            );
            assert_eq!(
                env.svm.get_account(&provider_key),
                Some(redeemed_wallet.clone())
            );

            let market = env.svm.get_account(&market_key).unwrap();
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = market.lamports + rent - tombstone_rent;
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(market_key, false),
                    AccountMeta::new(vault_key, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            peak = peak.max(land(
                &mut env,
                &[close],
                &[&admin],
                &tracked,
                &[market_key, vault_key],
                0,
                Some((admin.pubkey(), refund)),
                None,
            ));
            calls += 1;
            assert_eq!(calls, 2 + usize::from(prefunded) + usize::from(!bundled));
            assert!(calls <= 4);
            let tombstone = env.svm.get_account(&market_key).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(env
                .svm
                .get_account(&vault_key)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(env.svm.get_account(&destination), Some(paid_destination));
            assert_eq!(env.svm.get_account(&provider_key), Some(redeemed_wallet));
            assert_cu_within("native provider redeemed prefix", peak, LIMIT);
            eprintln!("INV-073 native provider: prefunded={prefunded}, bundled={bundled}, redeemed={REDEEMED}, unpaid_paid={}, external_wrapped={wrapped}, continuation_calls={calls}/4, peak={peak} CU", PRINCIPAL - REDEEMED);
        }
    }
}
