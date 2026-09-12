//! INV-021/024/067/070/073/081: absent recipients retain reserve and close progress
//! across rollback of actual vault closure, with a lazy or previously paid ledger.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

const CU_BOUND: u64 = 1_200_000;

pub(crate) fn verify_terminal_reserve_close_retry() {
    let mut payout_peak = 0;
    let mut rejection_peak = 0;
    let mut close_peak = 0;
    for earnings_prefix in [0, 17] {
        let TerminalEarningsWorld {
            mut env,
            admin,
            incumbent,
            successor,
            mut wallets,
            mut tokens,
            portfolios,
            mint_frame,
        } = terminal_earnings_world();
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
        drop((incumbent, successor, beneficiary));
        assert!(!wallets.contains(&env.payer.pubkey()));
        assert!(!wallets.contains(&admin.pubkey()));
        assert_ne!(wallets[2], wallets[4]);

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
        let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let authority = env.control_sequences(0);
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let stock = |env: &V16CuEnv, paid: [u64; 3]| {
            let image = env.svm.get_account(&env.market).unwrap();
            let (cfg, group) = env.market_state();
            let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
            assert_eq!(cfg.terminal_slab_scan_progress, 0);
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
            assert_eq!(group.insurance_domain_budget, vec![group.insurance, 0]);
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                group.insurance
            );
            assert_eq!(
                group.backing_provider_earnings_total,
                u128::from(EARNINGS - paid[1])
            );
            let bucket = group.source_backing_buckets[1];
            assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(BACKING - paid[0]) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            assert_eq!(
                bucket.utilization_fee_earnings,
                u128::from(EARNINGS - paid[1])
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(
                bucket.consumed_liened_backing_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].provider_receivable_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
            assert_eq!(env.control_sequences(0), authority);
            assert_eq!(
                state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                profile
            );
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let amounts = [PAYOUTS[0], PAYOUTS[1], paid[0] + paid[1], 0, paid[2]];
            for ((key, frame), amount) in tokens
                .into_iter()
                .zip(&token_frames)
                .zip(amounts)
                .chain(std::iter::once(((env.vault, &vault_frame), remaining)))
            {
                let mut expected = frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amount;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(&key), Some(expected));
            }
            assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
            assert_eq!(env.token_amount(admin_token), 0);
            let record = env.svm.get_account(&ledger).unwrap();
            if paid[1] == 0 {
                assert_eq!(record, empty_ledger);
            } else {
                let record = state::read_backing_domain_ledger(&record.data).unwrap();
                assert_eq!(record.market_group, env.market.to_bytes());
                assert_eq!(record.authority, wallets[2].to_bytes());
                assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
                assert_eq!(
                    record.last_observed_bucket_earnings_atoms,
                    u128::from(EARNINGS - paid[1])
                );
            }
            crate::support::fuzz_model::assert_market_stock_census(
                "absent recipient reserve close retry",
                &group,
                &image.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "absent recipient reserve close retry",
                &group,
                &[],
            )
            .unwrap();
        };
        stock(&env, [0; 3]);
        let prefix = [101, earnings_prefix, 7];
        let prefix_ixs = prefix
            .into_iter()
            .enumerate()
            .filter(|(_, amount)| *amount != 0)
            .map(|(kind, amount)| reserve_payout(&env, wallets, tokens, ledger, kind, amount))
            .collect::<Vec<_>>();
        let allowed = [env.market, env.vault, ledger, tokens[2], tokens[4]];
        payout_peak = payout_peak.max(land(
            &mut env,
            &prefix_ixs,
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        stock(&env, prefix);

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
                authority_epoch: authority.authority_epoch,
            }
            .encode(),
        };
        rejection_peak = rejection_peak.max(land(
            &mut env,
            std::slice::from_ref(&close),
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        ));
        stock(&env, prefix);
        // Keep the exact reserve instructions across rollback; only refresh transaction blockhashes.
        let stock_amounts = [BACKING, EARNINGS, INSURANCE];
        let mut retirement = [2, 1, 0]
            .into_iter()
            .map(|kind| {
                reserve_payout(
                    &env,
                    wallets,
                    tokens,
                    ledger,
                    kind,
                    stock_amounts[kind] - prefix[kind],
                )
            })
            .collect::<Vec<_>>();
        for ix in &retirement {
            assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
        }
        retirement.push(close.clone());
        let retirement_bytes = bincode::serialize(&retirement).unwrap();
        let mut repeated_close = retirement.clone();
        repeated_close.push(close.clone());
        rejection_peak = rejection_peak.max(land(
            &mut env,
            &repeated_close,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((6, PercolatorError::InvalidAccountLen)),
        ));
        stock(&env, prefix);
        assert_eq!(bincode::serialize(&retirement).unwrap(), retirement_bytes);

        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports
            - tombstone_rent;
        close_peak = close_peak.max(land(
            &mut env,
            &retirement,
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
        assert!(env.svm.get_account(&env.vault).is_none_or(|account| {
            account.lamports == 0
                && account.data.is_empty()
                && account.owner == solana_sdk::system_program::ID
                && !account.executable
        }));
        let final_amounts = [PAYOUTS[0], PAYOUTS[1], BACKING + EARNINGS, 0, INSURANCE];
        for ((key, frame), amount) in tokens.into_iter().zip(&token_frames).zip(final_amounts) {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        assert_eq!(final_amounts.iter().sum::<u64>(), SUPPLY);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        let final_ledger = env.svm.get_account(&ledger).unwrap();
        assert_eq!(final_ledger.lamports, empty_ledger.lamports);
        assert_eq!(final_ledger.owner, env.program_id);
        let record = state::read_backing_domain_ledger(&final_ledger.data).unwrap();
        assert_eq!(record.market_group, env.market.to_bytes());
        assert_eq!(record.authority, wallets[2].to_bytes());
        assert_eq!(record.total_earnings_withdrawn_atoms, EARNINGS.into());
        assert_eq!(record.last_observed_bucket_earnings_atoms, 0);

        // A fresh delivery after retirement cannot repeat payment or the rent refund.
        rejection_peak = rejection_peak.max(land(
            &mut env,
            &retirement,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::InvalidAccountLen)),
        ));
        rejection_peak = rejection_peak.max(land(
            &mut env,
            &[close],
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::InvalidAccountLen)),
        ));
    }
    for (label, cu) in [
        ("unsigned reserve paid prefix", payout_peak),
        ("reserve final-close rollback/replay", rejection_peak),
        ("reserve payout and final-close retry", close_peak),
    ] {
        assert_cu_within(label, cu, CU_BOUND);
    }
    eprintln!("INV-073 reserve final-close retry: worlds=2, exact_rollbacks=8, payout_prefix_peak={payout_peak}, rejection_peak={rejection_peak}, final_close_peak={close_peak}, ceiling={CU_BOUND}");
}
