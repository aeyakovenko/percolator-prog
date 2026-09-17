//! INV-073 / rows 420 and 433: absent-provider earned fees remain payable when
//! one ledger spans both configured quote rails. A keeper pays a fee prefix on
//! one rail, a rejected suffix+close bundle rolls back the suffix and ledger
//! initialization state, and the suffix then retries on the other rail. Principal
//! and insurance stay separate until their own unsigned terminal withdrawals.
//! Native-primary worlds additionally restore an absent ATA inside the unsigned
//! fee payment and roll back both creation and payment at a premature close.
//! A redeemed native fee prefix survives wallet disappearance, cross-rail fee
//! completion, native principal custody repair and final-close rollback/retry.
//! Arbitrary histories, recredit and native-secondary placement remain open.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use terminal_reserve_destination_recovery::land;

const PREFIX: u64 = 17;

pub(crate) fn verify_dual_quote_earnings_progress() {
    verify_dual_quote_earnings_histories(false);
}

#[test]
fn v16_program_redeemed_native_fee_prefix_survives_absent_provider_and_dual_quote_close() {
    verify_dual_quote_earnings_histories(true);
}

fn verify_dual_quote_earnings_histories(redeem_prefix: bool) {
    let mut worlds = 0;
    let mut peak = [0u64; 3];
    let mut rollbacks = 0;
    let histories: &[(bool, usize)] = if redeem_prefix {
        &[(true, 0)]
    } else {
        &[(false, 0), (false, 1), (true, 0), (true, 1)]
    };
    for &(native, first_rail) in histories {
        let (
            TerminalEarningsWorld {
                mut env,
                admin,
                incumbent: provider,
                successor,
                wallets,
                tokens,
                portfolios,
                mint_frame,
            },
            users,
            secondary,
        ) = if native {
            let (world, users, secondary) =
                terminal_earnings_world_with_optional_secondary_quote(true, None, 0, true, true);
            (world, users, secondary.unwrap())
        } else {
            terminal_earnings_world_with_dual_spl_quote(true)
        };
        let provider_key = provider.pubkey();
        assert_eq!(provider_key, wallets[2]);
        assert_eq!(env.token_amount(tokens[2]), 0);
        assert_eq!(env.token_amount(secondary.provider_token), 0);
        assert_eq!(env.token_amount(env.vault), BACKING + EARNINGS + INSURANCE);
        assert_eq!(env.token_amount(secondary.vault), EARNINGS);

        let ledger = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &ledger,
            state::backing_domain_ledger_account_len(),
            env.program_id,
        );
        let ledger = ledger.pubkey();
        let config = env.market_state().0;
        let sequences = env.control_sequences(0);
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        assert_eq!(profile.backing_bucket_authority, provider_key.to_bytes());
        let market_frame = env.svm.get_account(&env.market).unwrap();
        let primary_vault_frame = env.svm.get_account(&env.vault).unwrap();
        let secondary_vault_frame = env.svm.get_account(&secondary.vault).unwrap();
        let secondary_provider_frame = env.svm.get_account(&secondary.provider_token).unwrap();
        let secondary_admin_frame = env.svm.get_account(&secondary.admin_token).unwrap();
        let primary_provider_frame = env.svm.get_account(&tokens[2]).unwrap();
        let primary_admin_frame = env.svm.get_account(&tokens[4]).unwrap();
        let admin_frame = env.svm.get_account(&admin.pubkey()).unwrap();
        let tracked = [
            env.market,
            env.vault,
            env.vault_authority,
            env.mint,
            secondary.mint,
            secondary.vault,
            secondary.provider_token,
            secondary.admin_token,
            ledger,
            admin.pubkey(),
        ]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect::<Vec<_>>();
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        if native && !redeem_prefix {
            let remove = spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[2],
                &provider_key,
                &provider_key,
                &[],
            )
            .unwrap();
            peak[0] = peak[0].max(land(
                &mut env,
                &[remove],
                &[&provider],
                &tracked,
                &[tokens[2]],
                0,
                Some((provider_key, rent)),
                None,
            ));
        }
        let mut provider = if redeem_prefix {
            Some(provider)
        } else {
            drop(provider);
            None
        };
        drop((successor, users));
        let repair = Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new_readonly(provider_key, false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: vec![1],
        };
        let payer_key = env.payer.pubkey();
        let payment_bundle = |ix: Instruction, rail: usize| {
            let mut ixs = Vec::new();
            if native && rail == 0 {
                ixs.push(repair.clone());
            }
            ixs.push(ix);
            assert!(ixs
                .iter()
                .flat_map(|ix| &ix.accounts)
                .all(|meta| { !meta.is_signer || meta.pubkey == payer_key }));
            ixs
        };
        // Expected images only: never installed into LiteSVM.
        let token_image = |initial: &solana_sdk::account::Account, amount: u64| {
            let mut expected = initial.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            if token.is_native.is_some() {
                expected.lamports = expected.lamports - token.amount + amount;
            }
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            expected
        };

        let payout = |env: &V16CuEnv, rail: usize, amount: u64| -> Instruction {
            let (dest, vault) = if rail == 0 {
                (tokens[2], env.vault)
            } else {
                (secondary.provider_token, secondary.vault)
            };
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(provider_key, false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(ledger, false),
                    AccountMeta::new(dest, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::WithdrawBackingBucketEarnings {
                    domain: 1,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    amount: amount.into(),
                }
                .encode(),
            }
        };
        let principal = |env: &V16CuEnv| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(provider_key, false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawBackingBucket {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount: BACKING.into(),
            }
            .encode(),
        };
        let insurance = |env: &V16CuEnv| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(wallets[4], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount: INSURANCE.into(),
            }
            .encode(),
        };
        let close = |env: &V16CuEnv| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(secondary.vault, false),
                AccountMeta::new(secondary.admin_token, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let check = |env: &V16CuEnv,
                     fees: [u64; 2],
                     redeemed: u64,
                     principal_paid: u64,
                     insurance_paid: u64,
                     closed: bool| {
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(
                env.svm.get_account(&secondary.mint),
                Some(secondary.mint_frame.clone())
            );
            if redeemed != 0 {
                assert_eq!(redeemed, PREFIX);
                assert!(env.svm.get_account(&provider_key).is_none_or(|account| {
                    account.lamports == 0
                        && account.data.is_empty()
                        && account.owner == solana_sdk::system_program::ID
                        && !account.executable
                }));
            }
            if closed {
                let tombstone = env.svm.get_account(&env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(
                    env.svm.get_account(&tokens[2]),
                    Some(token_image(
                        &primary_provider_frame,
                        BACKING + fees[0] - redeemed
                    ))
                );
                assert_eq!(
                    env.svm.get_account(&secondary.provider_token),
                    Some(token_image(&secondary_provider_frame, fees[1]))
                );
                assert_eq!(
                    env.svm.get_account(&tokens[4]),
                    Some(token_image(&primary_admin_frame, INSURANCE + fees[1]))
                );
                assert_eq!(
                    env.svm.get_account(&secondary.admin_token),
                    Some(token_image(&secondary_admin_frame, EARNINGS - fees[1]))
                );
                let mut expected_admin = admin_frame.clone();
                expected_admin.lamports += market_frame.lamports - tombstone.lamports + 2 * rent;
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                for vault in [env.vault, secondary.vault] {
                    assert!(env.svm.get_account(&vault).is_none_or(|a| {
                        a.lamports == 0
                            && a.data.is_empty()
                            && a.owner == solana_sdk::system_program::ID
                    }));
                }
                return;
            }
            assert_eq!(env.market_state().0, config);
            assert_eq!(
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0
                )
                .unwrap(),
                profile
            );
            let group = env.market_state().1;
            let paid_fees = fees.iter().sum::<u64>();
            let remaining =
                BACKING + EARNINGS + INSURANCE - principal_paid - insurance_paid - paid_fees;
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(group.vault, remaining.into());
            assert_eq!(
                group.backing_provider_earnings_total,
                u128::from(EARNINGS - paid_fees)
            );
            let bucket = group.source_backing_buckets[1];
            assert_eq!(
                bucket.utilization_fee_earnings,
                u128::from(EARNINGS - paid_fees)
            );
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(BACKING - principal_paid) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
            assert_eq!(group.insurance_domain_budget[0], group.insurance);
            assert!(group.insurance_domain_budget[1..]
                .iter()
                .all(|amount| *amount == 0));
            assert_eq!(env.token_amount(tokens[0]), PAYOUTS[0]);
            assert_eq!(env.token_amount(tokens[1]), PAYOUTS[1]);
            if native && principal_paid == 0 && ((fees[0] == 0 && !redeem_prefix) || redeemed != 0)
            {
                assert!(env
                    .svm
                    .get_account(&tokens[2])
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            } else {
                assert_eq!(
                    env.svm.get_account(&tokens[2]),
                    Some(token_image(
                        &primary_provider_frame,
                        principal_paid + fees[0] - redeemed
                    ))
                );
            }
            assert_eq!(env.token_amount(secondary.provider_token), fees[1]);
            assert_eq!(env.token_amount(tokens[4]), insurance_paid);
            assert_eq!(env.token_amount(secondary.admin_token), 0);
            assert_eq!(
                env.token_amount(env.vault),
                remaining + fees[1],
                "secondary-paid fees leave explicit primary surplus"
            );
            assert_eq!(
                env.token_amount(secondary.vault),
                EARNINGS - fees[1],
                "secondary custody is liquidity, not extra entitlement"
            );
            assert_eq!(
                env.token_amount(env.vault) + env.token_amount(secondary.vault),
                remaining + EARNINGS
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_frame.lamports
            );
            assert_eq!(
                env.svm.get_account(&env.vault),
                Some(token_image(&primary_vault_frame, remaining + fees[1]))
            );
            assert_eq!(
                env.svm.get_account(&tokens[4]),
                Some(token_image(&primary_admin_frame, insurance_paid))
            );
            assert_eq!(
                env.svm.get_account(&secondary.vault).unwrap().lamports,
                secondary_vault_frame.lamports
            );
            assert_eq!(
                env.svm
                    .get_account(&secondary.provider_token)
                    .unwrap()
                    .lamports,
                secondary_provider_frame.lamports
            );
            assert_eq!(
                env.svm
                    .get_account(&secondary.admin_token)
                    .unwrap()
                    .lamports,
                secondary_admin_frame.lamports
            );
            if paid_fees == 0 {
                assert!(env.svm.get_account(&ledger).is_some());
            } else {
                let ledger_account = env.svm.get_account(&ledger).unwrap();
                let record = state::read_backing_domain_ledger(&ledger_account.data).unwrap();
                assert_eq!(record.market_group, env.market.to_bytes());
                assert_eq!(record.authority, provider_key.to_bytes());
                assert_eq!(record.domain, 1);
                assert_eq!(record.total_principal_atoms, 0);
                assert_eq!(record.total_earnings_atoms, 0);
                assert_eq!(record.total_earnings_withdrawn_atoms, u128::from(paid_fees));
                assert_eq!(
                    record.last_observed_bucket_earnings_atoms,
                    u128::from(EARNINGS - paid_fees)
                );
            }
            assert_eq!(env.control_sequences(0), {
                let mut expected = sequences;
                expected.authority_epoch += u64::from(insurance_paid != 0);
                expected
            });
            assert_market_stock_census(
                "dual quote earned-fee progress",
                &group,
                &env.svm.get_account(&env.market).unwrap().data,
                &[],
                remaining.into(),
            )
            .unwrap();
            assert_reservation_encumbrance_census("dual quote earned-fee progress", &group, &[])
                .unwrap();
        };

        check(&env, [0, 0], 0, 0, 0, false);
        let second_rail = 1 - first_rail;
        let first = payout(&env, first_rail, PREFIX);
        assert!(first.accounts.iter().all(|meta| !meta.is_signer));
        let first_allowed = [
            env.market,
            ledger,
            [env.vault, secondary.vault][first_rail],
            [tokens[2], secondary.provider_token][first_rail],
        ];
        let first = payment_bundle(first, first_rail);
        if native && first_rail == 0 && !redeem_prefix {
            let mut rejected = first.clone();
            rejected.push(close(&env));
            peak[1] = peak[1].max(land(
                &mut env,
                &rejected,
                &[],
                &tracked,
                &[],
                0,
                None,
                Some((4, PercolatorError::EngineLockActive)),
            ));
            rollbacks += 1;
            check(&env, [0, 0], 0, 0, 0, false);
        }
        peak[0] = peak[0].max(land(
            &mut env,
            &first,
            &[],
            &tracked,
            &first_allowed,
            if native && first_rail == 0 && !redeem_prefix {
                rent
            } else {
                0
            },
            None,
            None,
        ));
        let mut fees = [0, 0];
        fees[first_rail] = PREFIX;
        check(&env, fees, 0, 0, 0, false);

        let mut redeemed = 0;
        if let Some(provider) = provider.take() {
            let paid_ledger = env.svm.get_account(&ledger);
            let remove = spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[2],
                &provider_key,
                &provider_key,
                &[],
            )
            .unwrap();
            peak[0] = peak[0].max(land(
                &mut env,
                &[remove],
                &[&provider],
                &tracked,
                &[tokens[2]],
                0,
                Some((provider_key, rent + PREFIX)),
                None,
            ));
            let wallet_lamports = env.svm.get_account(&provider_key).unwrap().lamports;
            let drain = system_instruction::transfer(&provider_key, &wallets[3], wallet_lamports);
            peak[0] = peak[0].max(land(
                &mut env,
                &[drain],
                &[&provider],
                &tracked,
                &[provider_key],
                0,
                Some((wallets[3], wallet_lamports)),
                None,
            ));
            drop(provider);
            redeemed = PREFIX;
            assert_eq!(env.svm.get_account(&ledger), paid_ledger);
            check(&env, fees, redeemed, 0, 0, false);
        }

        let suffix = payout(&env, second_rail, EARNINGS - PREFIX);
        let premature_close = close(&env);
        let suffix = payment_bundle(suffix, second_rail);
        let mut rejected = suffix.clone();
        rejected.push(premature_close);
        peak[1] = peak[1].max(land(
            &mut env,
            &rejected,
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2 + suffix.len() as u8, PercolatorError::EngineLockActive)),
        ));
        rollbacks += 1;
        check(&env, fees, redeemed, 0, 0, false);

        let suffix_allowed = [
            env.market,
            ledger,
            [env.vault, secondary.vault][second_rail],
            [tokens[2], secondary.provider_token][second_rail],
        ];
        peak[0] = peak[0].max(land(
            &mut env,
            &suffix,
            &[],
            &tracked,
            &suffix_allowed,
            if native && second_rail == 0 { rent } else { 0 },
            None,
            None,
        ));
        fees[second_rail] = EARNINGS - PREFIX;
        check(&env, fees, redeemed, 0, 0, false);

        let principal_ix = principal(&env);
        let principal_ixs = if redeem_prefix {
            payment_bundle(principal_ix, 0)
        } else {
            vec![principal_ix]
        };
        if redeem_prefix {
            let mut rejected = principal_ixs.clone();
            rejected.push(close(&env));
            peak[1] = peak[1].max(land(
                &mut env,
                &rejected,
                &[],
                &tracked,
                &[],
                0,
                None,
                Some((4, PercolatorError::EngineLockActive)),
            ));
            rollbacks += 1;
            check(&env, fees, redeemed, 0, 0, false);
        }
        let principal_allowed = [env.market, env.vault, tokens[2]];
        peak[0] = peak[0].max(land(
            &mut env,
            &principal_ixs,
            &[],
            &tracked,
            &principal_allowed,
            if redeem_prefix { rent } else { 0 },
            None,
            None,
        ));
        check(&env, fees, redeemed, BACKING, 0, false);
        let insurance_ix = insurance(&env);
        let insurance_allowed = [env.market, env.vault, tokens[4]];
        assert!(insurance_ix.accounts.iter().all(|meta| !meta.is_signer));
        peak[0] = peak[0].max(land(
            &mut env,
            &[insurance_ix],
            &[],
            &tracked,
            &insurance_allowed,
            0,
            None,
            None,
        ));
        check(&env, fees, redeemed, BACKING, INSURANCE, false);
        let close_ix = close(&env);
        let paid_ledger = env.svm.get_account(&ledger);
        if redeem_prefix {
            peak[1] = peak[1].max(land(
                &mut env,
                &[close_ix.clone(), close_ix.clone()],
                &[],
                &tracked,
                &[],
                0,
                None,
                Some((3, PercolatorError::InvalidAccountLen)),
            ));
            rollbacks += 1;
            check(&env, fees, redeemed, BACKING, INSURANCE, false);
        }
        let close_allowed = [
            env.market,
            admin.pubkey(),
            env.vault,
            secondary.vault,
            tokens[4],
            secondary.admin_token,
        ];
        peak[2] = peak[2].max(land(
            &mut env,
            &[close_ix],
            &[],
            &tracked,
            &close_allowed,
            0,
            None,
            None,
        ));
        check(&env, fees, redeemed, BACKING, INSURANCE, true);
        assert_eq!(env.svm.get_account(&ledger), paid_ledger);
        worlds += 1;
    }
    assert_eq!(worlds, histories.len());
    assert_eq!(rollbacks, if redeem_prefix { 3 } else { 5 });
    eprintln!(
        "INV-073 dual quote earnings progress: redeem_prefix={redeem_prefix}, worlds={worlds}, fee={EARNINGS}, prefix={PREFIX}, rollbacks={rollbacks}, payments={}, closures={worlds}, peak_cu={peak:?}",
        worlds * 4
    );
}
