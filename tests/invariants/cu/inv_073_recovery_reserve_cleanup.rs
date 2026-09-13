//! INV-073, row433: unavailable reserve beneficiaries cannot block economic exit
//! after a funded Recovery force-close, even when their SPL destinations are absent.
//! INV-018/021/024/027/067/069/070/071/078/081/082: custody reconstruction and
//! unsigned reserve payments cross the last-materialized-portfolio gate atomically.
//!
//! Unlike the existing resolved replacement/frozen-destination witnesses, both
//! histories begin with exposed Recovery positions and earned fees. Unlike rows
//! 420/421's expiry/exhaustion histories, all three reserve claims survive and are
//! actually paid. This is classic SPL only, adding no row418 token-variant matrix.
//! Owner and reserve keys are dropped before force-close. The market authority
//! still participates in resolution and mechanical deletion/retirement: row433 OPEN.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_recovery_reserve_cleanup() {
    let mut peak = 0;
    let mut rollbacks = 0;
    for order in [[0, 1], [1, 0]] {
        let (
            TerminalEarningsWorld {
                mut env,
                admin,
                incumbent,
                successor,
                mut wallets,
                mut tokens,
                portfolios,
                mint_frame,
            },
            users,
        ) = terminal_earnings_world_with_user_signers(false, None);
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
        let admin_token = tokens[4];
        wallets[4] = beneficiary.pubkey();
        tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
        assert!(!wallets.contains(&env.payer.pubkey()));
        assert!(!wallets.contains(&admin.pubkey()));
        assert_ne!(wallets[2], wallets[4]);

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
        let tracked = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            ledger,
            admin.pubkey(),
            admin_token,
            solana_sdk::sysvar::clock::id(),
        ]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect::<Vec<_>>();
        let token_rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        for (actor, signer) in [(2, &incumbent), (4, &beneficiary)] {
            assert_eq!(env.token_amount(tokens[actor]), 0);
            let close = spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[actor],
                &wallets[actor],
                &wallets[actor],
                &[],
            )
            .unwrap();
            peak = peak.max(land(
                &mut env,
                &[close],
                &[signer],
                &tracked,
                &[tokens[actor]],
                0,
                Some((wallets[actor], token_rent)),
                None,
            ));
        }
        drop((incumbent, successor, beneficiary, users));
        let absent = [tokens[2], tokens[4]].map(|key| env.svm.get_account(&key));
        assert!(absent.iter().all(|account| account
            .as_ref()
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty())));
        let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
        let repairs = [2, 4].map(|actor| Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new_readonly(wallets[actor], false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: vec![1],
        });
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 2, 0);
        let recovery = env.market_state().1;
        assert_eq!(recovery.mode, MarketModeV16::Live);
        assert_eq!(recovery.assets[0].lifecycle, AssetLifecycleV16::Recovery);
        assert_eq!(
            (
                recovery.assets[0].oi_eff_long_q,
                recovery.assets[0].oi_eff_short_q
            ),
            (1_050 * POS_SCALE, 1_050 * POS_SCALE)
        );
        assert_eq!(recovery.backing_provider_earnings_total, EARNINGS.into());
        let reserves: [Instruction; 3] = std::array::from_fn(|kind| {
            reserve_payout(
                &env,
                wallets,
                tokens,
                ledger,
                kind,
                [BACKING, EARNINGS, INSURANCE][kind],
            )
        });

        // Recovery has not yet granted terminal spending authority. Custody repair
        // completes first, so only the signature boundary can satisfy this check.
        if order[0] == 0 {
            for kind in 0..3 {
                peak = peak.max(land(
                    &mut env,
                    &[
                        repairs[usize::from(kind == 2)].clone(),
                        reserves[kind].clone(),
                    ],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                rollbacks += 1;
            }
        }
        env.svm.warp_to_slot(7);
        let force_close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[0], false),
                AccountMeta::new(portfolios[1], false),
            ],
            data: ProgInstruction::ForceCloseAbandonedAsset {
                asset_index: 0,
                now_slot: 7,
                close_q: 1_050 * POS_SCALE,
            }
            .encode(),
        };
        let allowed = [env.market, portfolios[0], portfolios[1]];
        peak = peak.max(land(
            &mut env,
            &[force_close],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        let recovered = env.market_state().1;
        assert_eq!(
            (
                recovered.assets[0].oi_eff_long_q,
                recovered.assets[0].oi_eff_short_q
            ),
            (0, 0)
        );
        for portfolio in portfolios {
            assert!(!has_active_leg_for_asset(
                &env.portfolio_state(portfolio),
                0
            ));
        }
        assert_eq!(env.token_amount(env.vault), SUPPLY);
        assert_eq!(recovered.backing_provider_earnings_total, EARNINGS.into());
        peak = peak.max(env.resolve());
        assert_eq!(env.market_state().1.resolved_slot, 7);
        env.svm.warp_to_slot(12);
        let mut calls = [0; 2];
        for _ in 0..8 {
            for actor in order {
                if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    continue;
                }
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(wallets[actor], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                    .encode(),
                };
                let before = [env.market, portfolios[actor]].map(|key| env.svm.get_account(&key));
                let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
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
                calls[actor] += 1;
                assert_ne!(
                    before,
                    [env.market, portfolios[actor]].map(|key| env.svm.get_account(&key)),
                    "accepted user continuation must make progress"
                );
                assert_eq!(
                    env.token_amount(tokens[0])
                        + env.token_amount(tokens[1])
                        + env.token_amount(env.vault),
                    SUPPLY
                );
            }
            if portfolios
                .iter()
                .all(|key| resolved_portfolio_is_terminal(&env, *key))
            {
                break;
            }
        }
        for actor in 0..2 {
            assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
            assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
        }
        assert_eq!(env.market_state().1.materialized_portfolio_count, 2);
        assert_eq!(env.market_state().1.c_tot, 0);
        assert_eq!(env.token_amount(env.vault), BACKING + EARNINGS + INSURANCE);
        assert_eq!(
            [tokens[2], tokens[4]].map(|key| env.svm.get_account(&key)),
            absent
        );

        let cleanup = portfolios.map(|portfolio| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: env.close_portfolio_ix(portfolio).encode(),
        });
        let first = order[0];
        let last = order[1];
        let allowed = [env.market, portfolios[first]];
        let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
        let first_rent = env.svm.get_account(&portfolios[first]).unwrap().lamports;
        peak = peak.max(land(
            &mut env,
            &[cleanup[first].clone()],
            &[&admin],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_lamports + first_rent
        );
        assert_eq!(env.market_state().1.materialized_portfolio_count, 1);
        if order[0] == 0 {
            for kind in 0..3 {
                peak = peak.max(land(
                    &mut env,
                    &[
                        repairs[usize::from(kind == 2)].clone(),
                        reserves[kind].clone(),
                    ],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::EngineLockActive)),
                ));
                rollbacks += 1;
            }
        }

        // This rollback crosses the admission boundary: last deletion moves rent,
        // earnings really pay and initialize the ledger, then a wrong-role payout
        // rejects. All compiled/tracked Accounts, including absent custody, return
        // exactly to the one-portfolio prefix; only the two-signature fee persists.
        let mut wrong_destination = reserves[2].clone();
        wrong_destination.accounts[2].pubkey = tokens[2];
        let rejected = [
            cleanup[last].clone(),
            repairs[0].clone(),
            reserves[1].clone(),
            repairs[1].clone(),
            wrong_destination,
        ];
        peak = peak.max(land(
            &mut env,
            &rejected,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((6, PercolatorError::InvalidTokenAccount)),
        ));
        rollbacks += 1;
        assert_eq!(env.market_state().1.materialized_portfolio_count, 1);
        assert_eq!(env.svm.get_account(&ledger), Some(empty_ledger));
        assert_eq!(
            [tokens[2], tokens[4]].map(|key| env.svm.get_account(&key)),
            absent
        );
        let last_rent = env.svm.get_account(&portfolios[last]).unwrap().lamports;
        let allowed = [env.market, portfolios[last]];
        peak = peak.max(land(
            &mut env,
            &[cleanup[last].clone()],
            &[&admin],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_lamports + first_rent + last_rent
        );
        assert_eq!(env.market_state().1.materialized_portfolio_count, 0);

        // The same payout bytes now succeed with just the keeper. Opposite orders
        // exercise the shared provider destination both before and after its lazy
        // earnings ledger exists; idempotent reconstruction charges rent only once.
        let reserve_order = if first == 0 { [0, 2, 1] } else { [1, 2, 0] };
        let mut created = [false; 2];
        let mut paid = [0u64; 3];
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        for kind in reserve_order {
            let destination = usize::from(kind == 2);
            let actor = if kind == 2 { 4 } else { 2 };
            let allowed = [env.market, env.vault, tokens[actor], ledger];
            let rent = if created[destination] { 0 } else { token_rent };
            peak = peak.max(land(
                &mut env,
                &[repairs[destination].clone(), reserves[kind].clone()],
                &[],
                &tracked,
                &allowed,
                rent,
                None,
                None,
            ));
            created[destination] = true;
            paid[kind] = [BACKING, EARNINGS, INSURANCE][kind];
            let image = env.svm.get_account(&env.market).unwrap();
            let (_, group) = env.market_state();
            let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(group.vault, remaining.into());
            assert_eq!(env.token_amount(env.vault), remaining);
            assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                group.insurance
            );
            assert_eq!(
                group.backing_provider_earnings_total,
                u128::from(EARNINGS - paid[1])
            );
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                u128::from(BACKING - paid[0]) * BOUND_SCALE
            );
            assert_eq!(
                group.source_backing_buckets[1].utilization_fee_earnings,
                u128::from(EARNINGS - paid[1])
            );
            assert_eq!(
                state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                profile
            );
            for (index, reserve_actor) in [2, 4].into_iter().enumerate() {
                if !created[index] {
                    continue;
                }
                let account = env.svm.get_account(&tokens[reserve_actor]).unwrap();
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(account.lamports, token_rent);
                assert_eq!(token.owner, wallets[reserve_actor]);
                assert_eq!(token.mint, env.mint);
                assert_eq!(token.state, AccountState::Initialized);
                assert_eq!(
                    (token.delegate, token.close_authority),
                    (COption::None, COption::None)
                );
                assert_eq!(
                    token.amount,
                    if index == 0 {
                        paid[0] + paid[1]
                    } else {
                        paid[2]
                    }
                );
            }
            assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            if paid[1] != 0 {
                let record =
                    state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data)
                        .unwrap();
                assert_eq!(record.authority, wallets[2].to_bytes());
                assert_eq!(record.market_group, env.market.to_bytes());
                assert_eq!(record.domain, 1);
                assert_eq!(record.total_earnings_withdrawn_atoms, EARNINGS.into());
                assert_eq!(record.last_observed_bucket_earnings_atoms, 0);
            }
            crate::support::fuzz_model::assert_market_stock_census(
                "Recovery reserve cleanup",
                &group,
                &image.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "Recovery reserve cleanup",
                &group,
                &[],
            )
            .unwrap();
            let mut image = image;
            state::market_view_mut(&mut image.data)
                .unwrap()
                .1
                .validate_shape()
                .unwrap();
        }
        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_token, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = env.svm.get_account(&env.market).unwrap().lamports
            + env.svm.get_account(&env.vault).unwrap().lamports
            - tombstone_rent;
        let allowed = [env.market, env.vault];
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
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert!(env
            .svm
            .get_account(&env.vault)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [PAYOUTS[0], PAYOUTS[1], BACKING + EARNINGS, 0, INSURANCE]
        );
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
            SUPPLY
        );
        assert_eq!(env.token_amount(admin_token), 0);
        eprintln!(
            "row433 Recovery cleanup: order={order:?}, user_calls={calls:?}/16, reserves={paid:?}"
        );
    }
    assert_eq!(rollbacks, 8);
    eprintln!(
        "row433 Recovery reserve cleanup: 2 worlds, {rollbacks} exact rollbacks, peak={peak} CU"
    );
}
