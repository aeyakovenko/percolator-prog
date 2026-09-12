//! INV-073 / row433: a spent payout and closed ATA do not reset unpaid reserves.
//! Recreation, remaining payments and actual slab closure roll back together;
//! identical instructions retry with absent or independently recreated custody.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_recreated_reserve_close() {
    const PREFIX: [u64; 3] = [101, 17, INSURANCE];
    const SPENT: u64 = PREFIX[0] + PREFIX[1];
    const REMAINDER: u64 = BACKING + EARNINGS - SPENT;
    const CU_BOUND: u64 = 800_000;
    let mut peaks = [0; 4]; // setup, rejected close bundles, independent repair, final retry
    for repair_before_retry in [false, true] {
        let TerminalEarningsWorld {
            mut env,
            admin,
            incumbent: provider,
            successor: operator,
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
        let payee = Pubkey::new_unique();
        let spent_token = create_ata_for_test(&mut env.svm, &env.payer, payee, env.mint);
        let ledger = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &ledger,
            state::backing_domain_ledger_account_len(),
            env.program_id,
        );
        let ledger = ledger.pubkey();
        let tracked = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            ledger,
            admin.pubkey(),
            admin_token,
            payee,
            spent_token,
            solana_sdk::sysvar::clock::id(),
        ]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect::<Vec<_>>();
        let provider_frame = env.svm.get_account(&tokens[2]).unwrap();
        let token_rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        assert_eq!(provider_frame.lamports, token_rent);
        let prefix = PREFIX
            .into_iter()
            .enumerate()
            .map(|(kind, amount)| reserve_payout(&env, wallets, tokens, ledger, kind, amount))
            .collect::<Vec<_>>();
        let prefix_allowed = [env.market, env.vault, tokens[2], tokens[4], ledger];
        peaks[0] = peaks[0].max(land(
            &mut env,
            &prefix,
            &[],
            &tracked,
            &prefix_allowed,
            0,
            None,
            None,
        ));
        assert_eq!(env.token_amount(tokens[2]), SPENT);
        assert_eq!(env.token_amount(tokens[4]), INSURANCE);

        // Spending and closing are public SPL instructions before the provider disappears.
        let spend_and_close = [
            spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[2],
                &spent_token,
                &wallets[2],
                &[],
                SPENT,
            )
            .unwrap(),
            spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[2],
                &wallets[2],
                &wallets[2],
                &[],
            )
            .unwrap(),
        ];
        peaks[0] = peaks[0].max(land(
            &mut env,
            &spend_and_close,
            &[&provider],
            &tracked,
            &[tokens[2], spent_token],
            0,
            Some((wallets[2], token_rent)),
            None,
        ));
        drop((provider, operator, beneficiary));
        assert!(!wallets.contains(&env.payer.pubkey()));
        assert!(!wallets.contains(&admin.pubkey()));
        let absent = env.svm.get_account(&tokens[2]);
        assert!(absent.as_ref().is_none_or(|a| {
            a.lamports == 0 && a.data.is_empty() && a.owner == solana_sdk::system_program::ID
        }));
        let ledger_prefix = env.svm.get_account(&ledger).unwrap();
        let record_prefix = state::read_backing_domain_ledger(&ledger_prefix.data).unwrap();
        assert_eq!(record_prefix.market_group, env.market.to_bytes());
        assert_eq!(record_prefix.authority, wallets[2].to_bytes());
        assert_eq!(record_prefix.domain, 1);
        assert_eq!(
            record_prefix.total_earnings_withdrawn_atoms,
            PREFIX[1].into()
        );
        assert_eq!(
            record_prefix.last_observed_bucket_earnings_atoms,
            u128::from(EARNINGS - PREFIX[1])
        );
        let (config_prefix, group_prefix) = env.market_state();
        assert_eq!(group_prefix.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group_prefix.c_tot,
                group_prefix.pnl_pos_tot,
                group_prefix.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group_prefix.source_claim_bound_total_num, 0);
        assert_eq!(group_prefix.vault, REMAINDER.into());
        assert_eq!(group_prefix.insurance, 0);
        assert!(group_prefix
            .insurance_domain_budget
            .iter()
            .all(|amount| *amount == 0));
        assert_eq!(group_prefix.insurance_domain_budget_remaining_total, 0);
        assert_eq!(
            group_prefix.backing_provider_earnings_total,
            u128::from(EARNINGS - PREFIX[1])
        );
        let principal = u128::from(BACKING - PREFIX[0]) * BOUND_SCALE;
        assert_eq!(
            group_prefix.source_backing_buckets[1].fresh_unliened_backing_num,
            principal
        );
        assert_eq!(
            group_prefix.source_credit[1].fresh_reserved_backing_num,
            principal
        );
        assert_eq!(
            group_prefix.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(EARNINGS - PREFIX[1])
        );
        let stock = |env: &V16CuEnv, present: bool| {
            assert_eq!(env.market_state(), (config_prefix, group_prefix.clone()));
            assert_eq!(env.svm.get_account(&ledger), Some(ledger_prefix.clone()));
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(env.token_amount(spent_token), SPENT);
            assert_eq!(env.token_amount(tokens[4]), INSURANCE);
            assert_eq!(env.token_amount(env.vault), REMAINDER);
            assert_eq!(
                PAYOUTS.iter().sum::<u64>() + SPENT + INSURANCE + REMAINDER,
                SUPPLY
            );
            assert_eq!(
                env.svm.get_account(&tokens[2]),
                if present {
                    Some(provider_frame.clone())
                } else {
                    absent.clone()
                }
            );
            let market = env.svm.get_account(&env.market).unwrap();
            crate::support::fuzz_model::assert_market_stock_census(
                "recreated reserve close",
                &group_prefix,
                &market.data,
                &[],
                REMAINDER.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "recreated reserve close",
                &group_prefix,
                &[],
            )
            .unwrap();
        };
        stock(&env, false);
        let repair = Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new_readonly(wallets[2], false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: vec![1],
        };
        let remaining = [0, 1].map(|kind| {
            reserve_payout(
                &env,
                wallets,
                tokens,
                ledger,
                kind,
                [BACKING, EARNINGS][kind] - PREFIX[kind],
            )
        });
        assert!(remaining
            .iter()
            .flat_map(|ix| &ix.accounts)
            .all(|meta| !meta.is_signer));
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
        // Closing between the two remaining liabilities must undo creation and principal payment.
        peaks[1] = peaks[1].max(land(
            &mut env,
            &[
                repair.clone(),
                remaining[0].clone(),
                close.clone(),
                remaining[1].clone(),
            ],
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((4, PercolatorError::EngineLockActive)),
        ));
        stock(&env, false);
        let completion = vec![
            repair.clone(),
            remaining[0].clone(),
            remaining[1].clone(),
            close.clone(),
        ];
        let retained = bincode::serialize(&completion).unwrap();
        let mut rejected = completion.clone();
        rejected.push(close);
        peaks[1] = peaks[1].max(land(
            &mut env,
            &rejected,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((6, PercolatorError::InvalidAccountLen)),
        ));
        stock(&env, false);
        if repair_before_retry {
            peaks[2] = peaks[2].max(land(
                &mut env,
                &[repair],
                &[],
                &tracked,
                &[tokens[2]],
                token_rent,
                None,
                None,
            ));
            stock(&env, true);
        }
        assert_eq!(bincode::serialize(&completion).unwrap(), retained);
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = env.svm.get_account(&env.market).unwrap().lamports
            + env.svm.get_account(&env.vault).unwrap().lamports
            - tombstone_rent;
        let allowed = [env.market, env.vault, tokens[2], ledger];
        peaks[3] = peaks[3].max(land(
            &mut env,
            &completion,
            &[&admin],
            &tracked,
            &allowed,
            if repair_before_retry { 0 } else { token_rent },
            Some((admin.pubkey(), refund)),
            None,
        ));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert!(env.svm.get_account(&env.vault).is_none_or(|a| {
            a.lamports == 0
                && a.data.is_empty()
                && a.owner == solana_sdk::system_program::ID
                && !a.executable
        }));
        let mut expected_provider = provider_frame;
        let mut token = TokenAccount::unpack(&expected_provider.data).unwrap();
        token.amount = REMAINDER;
        TokenAccount::pack(token, &mut expected_provider.data).unwrap();
        assert_eq!(env.svm.get_account(&tokens[2]), Some(expected_provider));
        let final_ledger = env.svm.get_account(&ledger).unwrap();
        assert_eq!(
            (
                final_ledger.owner,
                final_ledger.lamports,
                final_ledger.executable,
                final_ledger.rent_epoch
            ),
            (
                ledger_prefix.owner,
                ledger_prefix.lamports,
                ledger_prefix.executable,
                ledger_prefix.rent_epoch
            )
        );
        let mut expected_record = record_prefix;
        expected_record.total_earnings_withdrawn_atoms = EARNINGS.into();
        expected_record.last_observed_bucket_earnings_atoms = 0;
        assert_eq!(
            state::read_backing_domain_ledger(&final_ledger.data).unwrap(),
            expected_record
        );
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                + env.token_amount(spent_token),
            SUPPLY
        );
        assert_eq!(env.token_amount(admin_token), 0);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        // A newly delivered idempotent-create prefix cannot repay or refund the retired market.
        peaks[1] = peaks[1].max(land(
            &mut env,
            &completion,
            &[&admin],
            &tracked,
            &[],
            0,
            None,
            Some((3, PercolatorError::InvalidAccountKind)),
        ));
    }
    for (label, cu) in [
        "setup",
        "rejected close bundles",
        "independent repair",
        "final retry",
    ]
    .into_iter()
    .zip(peaks)
    {
        assert_cu_within(label, cu, CU_BOUND);
    }
    eprintln!("row433 recreated reserve close: worlds=2, exact_rollbacks=6, spent_prefix={SPENT}, remaining_payment={REMAINDER}, closures=2, peak_CU={peaks:?}, ceiling={CU_BOUND}");
}
