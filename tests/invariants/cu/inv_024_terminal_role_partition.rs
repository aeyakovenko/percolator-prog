//! INV-005/024/036/081: merging and separating terminal reserve holders moves
//! only the transferred role's unpaid stock, even when the former holder pays.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_terminal_insurer_merge_split_preserves_provider_fee_attribution() {
    const FIRST: [u64; 3] = [101, 17, 7];
    const MERGED: [u64; 3] = [103, 19, 5];
    const STOCK: [u64; 3] = [BACKING, EARNINGS, INSURANCE];
    let mut peak = 0;
    for former_insurer_pays in [false, true] {
        for order in [[0, 1, 2], [2, 1, 0]] {
            let TerminalEarningsWorld {
                mut env,
                admin,
                incumbent,
                successor,
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
            let tracked = [env.market, env.vault, env.mint, ledger, env.payer.pubkey()]
                .into_iter()
                .chain(wallets)
                .chain(tokens)
                .chain(portfolios)
                .collect::<Vec<_>>();
            let terminal = env.market_state().1;
            let config = env.market_state().0;
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let sequences = env.control_sequences(0);
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let empty_ledger = env.svm.get_account(&ledger).unwrap();
            let mut paid = [0; 3];
            let mut insurance_paid = [0; 5];
            let mut handoffs = 0;
            let mut insurer = 4;

            let check = |env: &V16CuEnv,
                         paid: [u64; 3],
                         insurance_paid: [u64; 5],
                         handoffs: u64,
                         insurer: usize| {
                assert_eq!(insurance_paid.iter().sum::<u64>(), paid[2]);
                let amounts = [
                    PAYOUTS[0],
                    PAYOUTS[1],
                    paid[0] + paid[1] + insurance_paid[2],
                    insurance_paid[3],
                    insurance_paid[4],
                ];
                let remaining = STOCK.iter().sum::<u64>() - paid.iter().sum::<u64>();
                for ((key, frame), amount) in tokens
                    .into_iter()
                    .zip(&token_frames)
                    .chain(std::iter::once((env.vault, &vault_frame)))
                    .zip(amounts.into_iter().chain(std::iter::once(remaining)))
                {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                let (current_config, group) = env.market_state();
                assert_eq!(current_config, config);
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
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - paid[1])
                );
                let bucket = group.source_backing_buckets[1];
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - paid[1])
                );
                let principal = u128::from(BACKING - paid[0]) * BOUND_SCALE;
                assert_eq!(bucket.fresh_unliened_backing_num, principal);
                assert_eq!(group.source_credit[1].fresh_reserved_backing_num, principal);
                assert_eq!(bucket.valid_liened_backing_num, 0);
                assert_eq!(
                    bucket.consumed_liened_backing_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[1].provider_receivable_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..]
                    .iter()
                    .all(|value| *value == 0));
                assert_eq!(
                    group.insurance_domain_spent,
                    terminal.insurance_domain_spent
                );
                assert_domain_budget_remaining_total_consistent(&group, "merged reserve holders");
                for domain in 0..group.source_credit.len() {
                    if domain != 1 {
                        assert_eq!(group.source_credit[domain], terminal.source_credit[domain]);
                        assert_eq!(
                            group.source_backing_buckets[domain],
                            terminal.source_backing_buckets[domain]
                        );
                    }
                }
                let mut expected_profile = profile;
                expected_profile.insurance_authority = wallets[insurer].to_bytes();
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    expected_profile
                );
                let mut expected_sequences = sequences;
                expected_sequences.authority_epoch += handoffs;
                assert_eq!(env.control_sequences(0), expected_sequences);
                let image = env.svm.get_account(&ledger).unwrap();
                if paid[1] == 0 {
                    assert_eq!(image, empty_ledger);
                } else {
                    let record = state::read_backing_domain_ledger(&image.data).unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.authority, wallets[2].to_bytes());
                    assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
                    assert_eq!(
                        record.last_observed_bucket_earnings_atoms,
                        u128::from(EARNINGS - paid[1])
                    );
                }
                let mut image = env.svm.get_account(&env.market).unwrap();
                state::market_view_mut(&mut image.data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            check(&env, paid, insurance_paid, handoffs, insurer);

            // Each stage pays the same three economic classes. Only insurance changes owner.
            for stage in 0..3 {
                for kind in order {
                    let amount = match stage {
                        0 => FIRST[kind],
                        1 => MERGED[kind],
                        _ => STOCK[kind] - FIRST[kind] - MERGED[kind],
                    };
                    let mut role_wallets = wallets;
                    let mut role_tokens = tokens;
                    role_wallets[4] = wallets[insurer];
                    role_tokens[4] = tokens[insurer];
                    let ix = reserve_payout(&env, role_wallets, role_tokens, ledger, kind, amount);
                    assert!(!ix.accounts.iter().any(|meta| meta.is_signer));
                    let recipient = if kind == 2 { insurer } else { 2 };
                    let allowed = [env.market, env.vault, tokens[recipient]]
                        .into_iter()
                        .chain((kind == 1).then_some(ledger))
                        .collect::<Vec<_>>();
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
                    paid[kind] += amount;
                    if kind == 2 {
                        insurance_paid[insurer] += amount;
                    }
                    check(&env, paid, insurance_paid, handoffs, insurer);
                }
                if stage == 2 {
                    break;
                }
                let (from, to) = if stage == 0 {
                    (&admin, &incumbent)
                } else {
                    (&incumbent, &successor)
                };
                let rotate = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(from.pubkey(), true),
                        AccountMeta::new_readonly(to.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::UpdateAssetAuthority {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: sequences.authority_epoch + handoffs,
                        kind: processor::ASSET_AUTH_INSURANCE,
                        new_pubkey: to.pubkey().to_bytes(),
                    }
                    .encode(),
                };
                let allowed = [env.market];
                peak = peak.max(land(
                    &mut env,
                    &[rotate],
                    &[from, to],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                handoffs += 1;
                insurer = if stage == 0 { 2 } else { 3 };
                check(&env, paid, insurance_paid, handoffs, insurer);
                if stage == 0 && former_insurer_pays {
                    env.payer = admin.insecure_clone();
                }
            }
            assert_eq!(paid, STOCK);
            assert_eq!(
                insurance_paid,
                [0, 0, MERGED[2], INSURANCE - FIRST[2] - MERGED[2], FIRST[2]]
            );
            assert_eq!(env.token_amount(tokens[2]), BACKING + EARNINGS + MERGED[2]);
            assert_eq!(env.token_amount(tokens[4]), FIRST[2]);
            let ledger_frame = env.svm.get_account(&ledger);
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
                    authority_epoch: sequences.authority_epoch + handoffs,
                }
                .encode(),
            };
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund =
                env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports - rent;
            let signers = if former_insurer_pays {
                vec![]
            } else {
                vec![&admin]
            };
            let allowed = [env.market, env.vault];
            peak = peak.max(land(
                &mut env,
                &[close],
                &signers,
                &tracked,
                &allowed,
                0,
                Some((admin.pubkey(), refund)),
                None,
            ));
            let slab = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&slab);
            assert_eq!(
                (slab.lamports, slab.data.len()),
                (rent, percolator_prog::constants::HEADER_LEN)
            );
            if let Some(vault) = env.svm.get_account(&env.vault) {
                assert_eq!(vault.lamports, 0);
                assert!(vault.data.is_empty());
            }
            assert_eq!(env.svm.get_account(&ledger), ledger_frame);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
                SUPPLY
            );
        }
    }
    println!("terminal insurer merge/split: 4 worlds, 36 reserve payments, 8 consensual handoffs, 4 slab closes; peak CU={peak}");
}
