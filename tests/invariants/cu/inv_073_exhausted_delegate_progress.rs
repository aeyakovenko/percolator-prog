//! INV-073/078/082, rows 420/433: exhausting an existing SPL allowance restores
//! unsigned provider payouts to the same custody without owner revocation.
//! INV-018/021/024/067/070/081: paid value, unpaid claims and rent stay separate.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_exhausted_spl_delegate_restores_absent_provider_terminal_progress() {
    const PREFIX: [u64; 3] = [101, 17, 0];
    const ALLOWANCE: u64 = PREFIX[0] + PREFIX[1];
    const STOCK: [u64; 3] = [BACKING, EARNINGS, INSURANCE];
    let mut peak = 0;
    for order in [[0, 1], [1, 0]] {
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
        assert!(!wallets.contains(&env.payer.pubkey()));
        let sink = create_ata_for_test(&mut env.svm, &env.payer, Pubkey::new_unique(), env.mint);
        let ledger = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &ledger,
            state::backing_domain_ledger_account_len(),
            env.program_id,
        );
        let ledger = ledger.pubkey();
        let tracked: Vec<_> = [
            env.market,
            env.vault,
            env.vault_authority,
            env.mint,
            ledger,
            sink,
        ]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect();
        let prefix =
            [0, 1].map(|kind| reserve_payout(&env, wallets, tokens, ledger, kind, PREFIX[kind]));
        let changed = [env.market, env.vault, tokens[2], ledger];
        land(&mut env, &prefix, &[], &tracked, &changed, 0, None, None);
        let approve = spl_token::instruction::approve(
            &spl_token::ID,
            &tokens[2],
            &env.payer.pubkey(),
            &wallets[2],
            &[],
            ALLOWANCE,
        )
        .unwrap();
        land(
            &mut env,
            &[approve],
            &[&provider],
            &tracked,
            &[tokens[2]],
            0,
            None,
            None,
        );
        drop((provider, operator));

        let custody: Vec<_> = tokens.into_iter().chain([sink, env.vault]).collect();
        let frames: Vec<_> = custody
            .iter()
            .map(|key| env.svm.get_account(key).unwrap())
            .collect();
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let ledger_rent = env.svm.get_account(&ledger).unwrap().lamports;
        let sequences = env.control_sequences(0);
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let provider_token = TokenAccount::unpack(&frames[2].data).unwrap();
        assert_eq!(provider_token.owner, wallets[2]);
        assert_eq!(provider_token.mint, env.mint);
        assert_eq!(provider_token.state, AccountState::Initialized);
        assert_eq!(provider_token.is_native, COption::None);
        assert_eq!(provider_token.close_authority, COption::None);

        let check = |env: &V16CuEnv, paid: [u64; 3], spent: u64| {
            let remaining = STOCK.iter().sum::<u64>() - paid.iter().sum::<u64>();
            let allowance = ALLOWANCE - spent;
            let amounts = [
                PAYOUTS[0],
                PAYOUTS[1],
                paid[0] + paid[1] - spent,
                0,
                paid[2],
                spent,
                remaining,
            ];
            for (i, ((key, frame), amount)) in custody.iter().zip(&frames).zip(amounts).enumerate()
            {
                let mut expected = frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amount;
                if i == 2 {
                    token.delegate = if allowance == 0 {
                        COption::None
                    } else {
                        COption::Some(env.payer.pubkey())
                    };
                    token.delegated_amount = allowance;
                }
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(
                    env.svm.get_account(key),
                    Some(expected),
                    "custody/rent: {key}"
                );
            }
            assert_eq!(amounts.iter().sum::<u64>(), SUPPLY);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let image = env.svm.get_account(&env.market).unwrap();
            assert_eq!(image.lamports, market_rent);
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
            assert_eq!(group.vault, u128::from(remaining));
            assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                group.insurance
            );
            assert_eq!(group.insurance_domain_budget[0], group.insurance);
            assert!(group.insurance_domain_budget[1..]
                .iter()
                .all(|value| *value == 0));
            assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
            let principal = u128::from(BACKING - paid[0]) * BOUND_SCALE;
            let bucket = group.source_backing_buckets[1];
            let source = group.source_credit[1];
            assert_eq!(bucket.fresh_unliened_backing_num, principal);
            assert_eq!(source.fresh_reserved_backing_num, principal);
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
                bucket.utilization_fee_earnings,
                u128::from(EARNINGS - paid[1])
            );
            assert_eq!(
                group.backing_provider_earnings_total,
                bucket.utilization_fee_earnings
            );
            assert_eq!(
                bucket.status,
                if principal == 0 {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(
                state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                profile
            );
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch += u64::from(paid[2] != 0);
            assert_eq!(env.control_sequences(0), expected_sequences);
            let record = env.svm.get_account(&ledger).unwrap();
            assert_eq!(record.lamports, ledger_rent);
            let record = state::read_backing_domain_ledger(&record.data).unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(record.domain, 1);
            assert_eq!(record.total_earnings_withdrawn_atoms, u128::from(paid[1]));
            assert_eq!(
                record.last_observed_bucket_earnings_atoms,
                u128::from(EARNINGS - paid[1])
            );
            crate::support::fuzz_model::assert_market_stock_census(
                "exhausted provider delegate",
                &group,
                &image.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "exhausted provider delegate",
                &group,
                &[],
            )
            .unwrap();
            (remaining, allowance)
        };
        let mut run = |env: &mut V16CuEnv, ixs: &[Instruction], changed: &[Pubkey], error| {
            assert!(ixs
                .iter()
                .flat_map(|ix| &ix.accounts)
                .all(|meta| !meta.is_signer || meta.pubkey == env.payer.pubkey()));
            peak = peak.max(land(env, ixs, &[], &tracked, changed, 0, None, error));
        };
        let spend = |amount| {
            spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[2],
                &sink,
                &env.payer.pubkey(),
                &[],
                amount,
            )
            .unwrap()
        };
        let partial_spend = spend(ALLOWANCE - 1);
        let last_spend = spend(1);
        let tails = [0, 1].map(|kind| {
            reserve_payout(
                &env,
                wallets,
                tokens,
                ledger,
                kind,
                STOCK[kind] - PREFIX[kind],
            )
        });
        let mut paid = PREFIX;
        let mut rank = check(&env, paid, 0);
        for kind in order {
            run(
                &mut env,
                &[tails[kind].clone()],
                &[],
                Some((2, PercolatorError::InvalidTokenAccount)),
            );
            assert_eq!(check(&env, paid, 0), rank);
        }
        // The SPL prefix really executes, but its remaining allowance blocks the payout.
        run(
            &mut env,
            &[partial_spend.clone(), tails[order[0]].clone()],
            &[],
            Some((3, PercolatorError::InvalidTokenAccount)),
        );
        assert_eq!(check(&env, paid, 0), rank);
        run(&mut env, &[partial_spend], &[tokens[2], sink], None);
        let next = check(&env, paid, ALLOWANCE - 1);
        assert!(next < rank);
        rank = next;
        run(
            &mut env,
            &[tails[order[0]].clone()],
            &[],
            Some((2, PercolatorError::InvalidTokenAccount)),
        );
        assert_eq!(check(&env, paid, ALLOWANCE - 1), rank);

        // Consuming the final approved atom clears delegation before the unchanged payout.
        let exhausted_changed = [env.market, env.vault, tokens[2], sink, ledger];
        run(
            &mut env,
            &[last_spend, tails[order[0]].clone()],
            &exhausted_changed,
            None,
        );
        paid[order[0]] = STOCK[order[0]];
        let next = check(&env, paid, ALLOWANCE);
        assert!(next < rank);
        rank = next;
        run(&mut env, &[tails[order[1]].clone()], &changed, None);
        paid[order[1]] = STOCK[order[1]];
        let next = check(&env, paid, ALLOWANCE);
        assert!(next < rank);
        rank = next;
        let insurance = reserve_payout(&env, wallets, tokens, ledger, 2, INSURANCE);
        let insurance_changed = [env.market, env.vault, tokens[4]];
        run(&mut env, &[insurance], &insurance_changed, None);
        paid[2] = INSURANCE;
        let next = check(&env, paid, ALLOWANCE);
        assert!(next < rank);
        assert_eq!(next, (0, 0));

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
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = market_rent + frames.last().unwrap().lamports - rent;
        let closed = [env.market, env.vault];
        peak = peak.max(land(
            &mut env,
            &[close],
            &[&admin],
            &tracked,
            &closed,
            0,
            Some((admin.pubkey(), refund)),
            None,
        ));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, rent);
        assert!(env.svm.get_account(&env.vault).is_none_or(|account| {
            account.lamports == 0
                && account.data.is_empty()
                && account.owner == solana_sdk::system_program::ID
        }));
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        assert_eq!(env.token_amount(tokens[2]), BACKING + EARNINGS - ALLOWANCE);
        assert_eq!(env.token_amount(sink), ALLOWANCE);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>() + ALLOWANCE,
            SUPPLY
        );
    }
    eprintln!("exhausted SPL delegate: 2 payout orders, 8 exact rejections, 2 terminal closures; peak CU={peak}");
}
