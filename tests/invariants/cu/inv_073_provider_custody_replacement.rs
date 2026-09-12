//! INV-073/018/024/063: reassigned paid custody cannot pin unpaid provider claims.
//! Public replacement creation and payments need no provider signature. Principal
//! expiry and earnings telemetry remain independent of the old token account owner.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_provider_custody_replacement() {
    const PREFIX: [u64; 3] = [101, 17, 0];
    let mut success_peak = 0;
    let mut rejection_peak = 0;
    let mut close_peak = 0;
    for delivery in [99, 100, 101] {
        for bundled_creation in [false, true] {
            let expired = delivery >= 100;
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
            let custodian = Keypair::new();
            let custodian_key = custodian.pubkey();
            assert!(!wallets.contains(&env.payer.pubkey()));
            assert!(!wallets.contains(&custodian_key));
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger.pubkey();
            let seed = "provider-terminal-custody";
            let replacement =
                Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap();
            assert!(!tokens.contains(&replacement));
            let tracked = [
                env.market,
                env.vault,
                env.vault_authority,
                env.mint,
                ledger,
                replacement,
                custodian_key,
            ]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .collect::<Vec<_>>();
            for kind in [0, 1] {
                let ix = reserve_payout(&env, wallets, tokens, ledger, kind, PREFIX[kind]);
                let allowed = [env.market, env.vault, ledger, tokens[2]];
                success_peak = success_peak.max(land(
                    &mut env,
                    &[ix],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
            }
            assert_eq!(env.token_amount(tokens[2]), PREFIX[0] + PREFIX[1]);
            let reassign = spl_token::instruction::set_authority(
                &spl_token::ID,
                &tokens[2],
                Some(&custodian_key),
                spl_token::instruction::AuthorityType::AccountOwner,
                &wallets[2],
                &[],
            )
            .unwrap();
            success_peak = success_peak.max(land(
                &mut env,
                &[reassign],
                &[&provider],
                &tracked,
                &[tokens[2]],
                0,
                None,
                None,
            ));
            drop((provider, operator, custodian));

            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let old_token = TokenAccount::unpack(&token_frames[2].data).unwrap();
            assert_eq!(old_token.owner, custodian_key);
            assert_eq!(old_token.amount, PREFIX[0] + PREFIX[1]);
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let market_frame = env.svm.get_account(&env.market).unwrap();
            let ledger_frame = env.svm.get_account(&ledger).unwrap();
            let ledger_record = state::read_backing_domain_ledger(&ledger_frame.data).unwrap();
            assert_eq!(ledger_record.market_group, env.market.to_bytes());
            assert_eq!(ledger_record.authority, wallets[2].to_bytes());
            assert_eq!(ledger_record.domain, 1);
            assert_eq!(
                ledger_record.total_earnings_withdrawn_atoms,
                PREFIX[1].into()
            );
            let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
            let sequences = env.control_sequences(0);
            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let creation = vec![
                system_instruction::create_account_with_seed(
                    &env.payer.pubkey(),
                    &replacement,
                    &env.payer.pubkey(),
                    seed,
                    token_rent,
                    TokenAccount::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_account3(
                    &spl_token::ID,
                    &replacement,
                    &env.mint,
                    &wallets[2],
                )
                .unwrap(),
            ];
            let mut current_tokens = tokens;
            current_tokens[2] = replacement;
            let principal = reserve_payout(
                &env,
                wallets,
                current_tokens,
                ledger,
                0,
                BACKING - PREFIX[0],
            );
            let earnings = reserve_payout(
                &env,
                wallets,
                current_tokens,
                ledger,
                1,
                EARNINGS - PREFIX[1],
            );
            let old_earnings =
                reserve_payout(&env, wallets, tokens, ledger, 1, EARNINGS - PREFIX[1]);
            let insurance = reserve_payout(&env, wallets, tokens, ledger, 2, INSURANCE);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(tokens[4], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            let stock = |env: &V16CuEnv, paid: [u64; 3], normalized: bool, present: bool| {
                let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
                let replacement_paid = paid[0] + paid[1] - PREFIX[0] - PREFIX[1];
                for actor in 0..5 {
                    let mut expected = token_frames[actor].clone();
                    if actor == 4 {
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = paid[2];
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                    }
                    assert_eq!(env.svm.get_account(&tokens[actor]), Some(expected));
                }
                if present {
                    let mut expected = token_frames[2].clone();
                    expected.lamports = token_rent;
                    let mut token = old_token;
                    token.owner = wallets[2];
                    token.amount = replacement_paid;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&replacement), Some(expected));
                } else {
                    assert_eq!(replacement_paid, 0);
                    assert!(env.svm.get_account(&replacement).is_none());
                }
                assert_eq!(
                    tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                        + replacement_paid
                        + remaining,
                    SUPPLY
                );
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                let mut vault = vault_frame.clone();
                let mut token = TokenAccount::unpack(&vault.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut vault.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(vault));
                let group = env.market_state().1;
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
                assert_eq!(group.vault, remaining.into());
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
                assert!(group.insurance_domain_spent.iter().all(|v| *v == 0));
                let bucket = group.source_backing_buckets[1];
                let source = group.source_credit[1];
                let fresh = if normalized {
                    0
                } else {
                    u128::from(BACKING - paid[0]) * BOUND_SCALE
                };
                assert_eq!(bucket.expiry_slot, 100);
                assert_eq!(bucket.fresh_unliened_backing_num, fresh);
                assert_eq!(source.fresh_reserved_backing_num, fresh);
                assert_eq!(bucket.valid_liened_backing_num, 0);
                assert_eq!(
                    bucket.consumed_liened_backing_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(
                    source.provider_receivable_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(source.spent_backing_num, u128::from(PROFIT) * BOUND_SCALE);
                assert_eq!(
                    bucket.status,
                    if fresh == 0 {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - paid[1])
                );
                assert_eq!(
                    group.backing_provider_earnings_total,
                    bucket.utilization_fee_earnings
                );
                let market = env.svm.get_account(&env.market).unwrap();
                assert_eq!(market.lamports, market_frame.lamports);
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                assert_eq!(env.control_sequences(0), sequences);
                crate::support::fuzz_model::assert_market_stock_census(
                    "provider custody replacement",
                    &group,
                    &market.data,
                    &[],
                    remaining.into(),
                )
                .unwrap();
                crate::support::fuzz_model::assert_reservation_encumbrance_census(
                    "provider custody replacement",
                    &group,
                    &[],
                )
                .unwrap();
                let mut expected_ledger = ledger_frame.clone();
                let mut record = ledger_record;
                record.total_earnings_withdrawn_atoms = paid[1].into();
                record.last_observed_bucket_earnings_atoms = u128::from(EARNINGS - paid[1]);
                state::write_backing_domain_ledger(&mut expected_ledger.data, &record).unwrap();
                assert_eq!(env.svm.get_account(&ledger), Some(expected_ledger));
            };
            stock(&env, PREFIX, false, false);
            env.svm.warp_to_slot(delivery);
            if !bundled_creation {
                success_peak = success_peak.max(land(
                    &mut env,
                    &creation,
                    &[],
                    &tracked,
                    &[replacement],
                    token_rent,
                    None,
                    None,
                ));
                stock(&env, PREFIX, false, true);
            }

            let mut continuation = Vec::new();
            if expired {
                continuation.push(close.clone());
            }
            if bundled_creation {
                continuation.extend(creation);
            }
            if !expired {
                continuation.push(principal);
            }
            continuation.push(earnings);
            let signers = if expired { vec![&admin] } else { vec![] };
            let mut rejected = continuation.clone();
            rejected.push(old_earnings);
            // The old populated custody remains owned by its new SPL owner. The
            // suffix checks restoration after replacement payment and ledger update.
            rejection_peak = rejection_peak.max(land(
                &mut env,
                &rejected,
                &signers,
                &tracked,
                &[],
                0,
                None,
                Some((
                    (continuation.len() + 2) as u8,
                    PercolatorError::InvalidTokenAccount,
                )),
            ));
            stock(&env, PREFIX, false, !bundled_creation);
            let allowed = [env.market, env.vault, ledger, replacement];
            success_peak = success_peak.max(land(
                &mut env,
                &continuation,
                &signers,
                &tracked,
                &allowed,
                if bundled_creation { token_rent } else { 0 },
                None,
                None,
            ));
            let mut paid = [if expired { PREFIX[0] } else { BACKING }, EARNINGS, 0];
            stock(&env, paid, expired, true);
            let allowed = [env.market, env.vault, tokens[4]];
            success_peak = success_peak.max(land(
                &mut env,
                &[insurance],
                &[],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            paid[2] = INSURANCE;
            stock(&env, paid, expired, true);

            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = market_frame.lamports + vault_frame.lamports - rent;
            let allowed = [env.market, env.vault, env.mint];
            close_peak = close_peak.max(land(
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
            let burned = if expired { BACKING - PREFIX[0] } else { 0 };
            let mut expected_mint = mint_frame;
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= burned;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                env.svm.get_account(&tokens[2]),
                Some(token_frames[2].clone())
            );
            assert_eq!(
                env.token_amount(replacement),
                paid[0] + EARNINGS - PREFIX[0] - PREFIX[1]
            );
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                    + env.token_amount(replacement)
                    + burned,
                SUPPLY
            );
            eprintln!("INV-073 provider custody: delivery={delivery}, bundled_creation={bundled_creation}, old_paid={}, replacement_paid={}, retired={burned}", PREFIX[0] + PREFIX[1], env.token_amount(replacement));
        }
    }
    eprintln!("INV-073 provider custody replacement: worlds=6, exact_rollbacks=6, success_peak={success_peak}, rejection_peak={rejection_peak}, close_peak={close_peak}, ceiling=1200000");
}
