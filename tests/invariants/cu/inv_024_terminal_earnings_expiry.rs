//! INV-024/063/070/073: expiry releases only the unpaid principal, never earned
//! provider fees. Reserve signatures remain necessary after bounded normalization.

use super::*;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal() {
    const PRINCIPAL_PAID: u64 = 101;
    const RETIRED: u64 = BACKING - PRINCIPAL_PAID;
    let mut peak = 0;
    for expiry_delivery in [100, 101] {
        for insurance_first in [false, true] {
            let TerminalEarningsWorld {
                mut env,
                admin,
                incumbent: provider,
                successor: operator,
                wallets,
                tokens,
                portfolios,
                mint_frame,
            } = terminal_earnings_world();
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
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let market_frame = env.svm.get_account(&env.market).unwrap();
            let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
            let sequences = env.control_sequences(0);
            let tracked = [env.market, env.vault, env.mint, ledger]
                .into_iter()
                .chain(wallets)
                .chain(tokens)
                .chain(portfolios)
                .collect::<Vec<_>>();
            let wrap = |ix: ProgInstruction, accounts| Instruction {
                program_id: env.program_id,
                accounts,
                data: ix.encode(),
            };
            let reserve = |actor: usize, earnings: bool, insurance: bool, amount: u64| {
                let mut accounts = vec![
                    AccountMeta::new(wallets[actor], true),
                    AccountMeta::new(env.market, false),
                ];
                if earnings {
                    accounts.push(AccountMeta::new(ledger, false));
                }
                accounts.extend([
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]);
                let ix = if insurance {
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch,
                        amount: amount.into(),
                    }
                } else if earnings {
                    ProgInstruction::WithdrawBackingBucketEarnings {
                        domain: 1,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch,
                        amount: amount.into(),
                    }
                } else {
                    ProgInstruction::WithdrawBackingBucket {
                        domain: 1,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch,
                        amount: amount.into(),
                    }
                };
                wrap(ix, accounts)
            };
            let principal = reserve(2, false, false, PRINCIPAL_PAID);
            let late_principal = reserve(2, false, false, 1);
            let earnings = reserve(2, true, false, EARNINGS);
            let insurance = reserve(4, false, true, INSURANCE);
            let admin_earnings = reserve(4, true, false, EARNINGS);
            let operator_insurance = reserve(3, false, true, INSURANCE);
            let insurance_overdraw = reserve(4, false, true, INSURANCE + 1);
            let mut unsigned_earnings = earnings.clone();
            unsigned_earnings.accounts[0].is_signer = false;
            let mut unsigned_insurance = insurance.clone();
            unsigned_insurance.accounts[0].is_signer = false;
            let close = wrap(
                ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(tokens[4], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let stock = |env: &V16CuEnv, principal_paid: u64, expired: bool, paid: [bool; 2]| {
                let fees_paid = if paid[0] { EARNINGS } else { 0 };
                let insurance_paid = if paid[1] { INSURANCE } else { 0 };
                let amounts = [
                    PAYOUTS[0],
                    PAYOUTS[1],
                    principal_paid + fees_paid,
                    0,
                    insurance_paid,
                ];
                for ((key, frame), amount) in tokens.into_iter().zip(&token_frames).zip(amounts) {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                let mut market = env.svm.get_account(&env.market).unwrap();
                let (_, view) = state::market_view_mut(&mut market.data).unwrap();
                view.validate_shape().unwrap();
                let group = env.market_state().1;
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!((group.c_tot, group.materialized_portfolio_count), (0, 0));
                assert_eq!(group.source_claim_bound_total_num, 0);
                let remaining =
                    BACKING - principal_paid + EARNINGS - fees_paid + INSURANCE - insurance_paid;
                assert_eq!(group.vault, remaining.into());
                assert_eq!(env.token_amount(env.vault), remaining);
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                let mut expected_vault = vault_frame.clone();
                let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut expected_vault.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
                assert_domain_budget_remaining_total_consistent(&group, "earned fee expiry");
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - fees_paid)
                );
                let bucket = group.source_backing_buckets[1];
                let source = group.source_credit[1];
                let fresh = if expired {
                    0
                } else {
                    u128::from(BACKING - principal_paid) * BOUND_SCALE
                };
                assert_eq!(bucket.expiry_slot, 100);
                assert_eq!(
                    bucket.status,
                    if expired {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(bucket.fresh_unliened_backing_num, fresh);
                assert_eq!(source.fresh_reserved_backing_num, fresh);
                assert_eq!(
                    (
                        source.provider_receivable_num,
                        source.valid_liened_backing_num,
                        source.spent_backing_num
                    ),
                    (
                        u128::from(PROFIT) * BOUND_SCALE,
                        0,
                        u128::from(PROFIT) * BOUND_SCALE
                    )
                );
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - fees_paid)
                );
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(market.lamports, market_frame.lamports);
                let account = env.svm.get_account(&ledger).unwrap();
                if paid[0] {
                    let record = state::read_backing_domain_ledger(&account.data).unwrap();
                    assert_eq!(record.authority, provider.pubkey().to_bytes());
                    assert_eq!(record.total_earnings_atoms, 0);
                    assert_eq!(record.total_earnings_withdrawn_atoms, EARNINGS.into());
                    assert_eq!(record.last_observed_bucket_earnings_atoms, 0);
                    assert_eq!(record.total_principal_atoms, 0);
                    assert_eq!(account.lamports, empty_ledger.lamports);
                } else {
                    assert_eq!(account, empty_ledger);
                }
            };
            stock(&env, 0, false, [false; 2]);
            let allowed = [env.market, env.vault, tokens[2]];
            peak = peak.max(land(
                &mut env,
                &[principal],
                &[&provider],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            stock(&env, PRINCIPAL_PAID, false, [false; 2]);
            env.svm.warp_to_slot(99);
            peak = peak.max(land(
                &mut env,
                &[close.clone()],
                &[&admin],
                &tracked,
                &[],
                0,
                None,
                Some((2, PercolatorError::EngineLockActive)),
            ));
            stock(&env, PRINCIPAL_PAID, false, [false; 2]);

            env.svm.warp_to_slot(expiry_delivery);
            // These prefixes normalize principal, then move real SPL value. The
            // failing suffix must restore both, including lazy earnings telemetry.
            for (ixs, signers, error) in [
                (
                    vec![close.clone(), insurance.clone(), unsigned_earnings.clone()],
                    vec![&admin],
                    PercolatorError::ExpectedSigner,
                ),
                (
                    vec![close.clone(), earnings.clone(), close.clone()],
                    vec![&admin, &provider],
                    PercolatorError::EngineLockActive,
                ),
            ] {
                peak = peak.max(land(
                    &mut env,
                    &ixs,
                    &signers,
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((4, error)),
                ));
                stock(&env, PRINCIPAL_PAID, false, [false; 2]);
            }
            let market_key = env.market;
            peak = peak.max(land(
                &mut env,
                &[close.clone()],
                &[&admin],
                &tracked,
                &[market_key],
                0,
                None,
                None,
            ));
            stock(&env, PRINCIPAL_PAID, true, [false; 2]);
            peak = peak.max(land(
                &mut env,
                &[earnings.clone(), unsigned_insurance],
                &[&provider],
                &tracked,
                &[],
                0,
                None,
                Some((3, PercolatorError::ExpectedSigner)),
            ));
            stock(&env, PRINCIPAL_PAID, true, [false; 2]);
            for (ix, signers, error) in [
                (unsigned_earnings, vec![], PercolatorError::ExpectedSigner),
                (admin_earnings, vec![&admin], PercolatorError::Unauthorized),
                (
                    operator_insurance,
                    vec![&operator],
                    PercolatorError::Unauthorized,
                ),
                (
                    insurance_overdraw,
                    vec![&admin],
                    PercolatorError::EngineLockActive,
                ),
                (
                    late_principal,
                    vec![&provider],
                    PercolatorError::EngineStale,
                ),
                (
                    close.clone(),
                    vec![&admin],
                    PercolatorError::EngineLockActive,
                ),
            ] {
                peak = peak.max(land(
                    &mut env,
                    &[ix],
                    &signers,
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((2, error)),
                ));
                stock(&env, PRINCIPAL_PAID, true, [false; 2]);
            }
            let mut paid = [false; 2];
            for kind in if insurance_first { [1, 0] } else { [0, 1] } {
                let (ix, signer, destination) = if kind == 0 {
                    (earnings.clone(), &provider, tokens[2])
                } else {
                    (insurance.clone(), &admin, tokens[4])
                };
                let mut allowed = vec![env.market, env.vault, destination];
                if kind == 0 {
                    allowed.push(ledger);
                }
                peak = peak.max(land(
                    &mut env,
                    &[ix],
                    &[signer],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                paid[kind] = true;
                stock(&env, PRINCIPAL_PAID, true, paid);
            }
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = market_frame.lamports + vault_frame.lamports - rent;
            let paid_ledger = env.svm.get_account(&ledger);
            let final_tokens = tokens.map(|key| env.svm.get_account(&key));
            let allowed = [env.market, env.vault, env.mint];
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
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(tokens.map(|key| env.svm.get_account(&key)), final_tokens);
            assert_eq!(env.svm.get_account(&ledger), paid_ledger);
            let mut expected_mint = mint_frame;
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply = SUPPLY - RETIRED;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)),
                [
                    PAYOUTS[0],
                    PAYOUTS[1],
                    PRINCIPAL_PAID + EARNINGS,
                    0,
                    INSURANCE
                ]
            );
        }
    }
    eprintln!("INV-024 earned fee expiry: 4 worlds, 40 exact rollbacks, 8 committed slab calls, retired={RETIRED}/world, provider={}, peak_CU={peak}", PRINCIPAL_PAID + EARNINGS);
}
