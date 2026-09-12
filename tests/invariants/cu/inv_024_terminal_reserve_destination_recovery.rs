//! INV-024/073/080/082: terminal reserve custody repair preserves signer gates,
//! distinct principal/earned-fee/insurance claims, and keeper-funded rent.
//! The current reserve routes require signatures even after the user exit window.

use super::*;

const CU_LIMIT: u64 = 400_000;

fn assert_closed(env: &V16CuEnv, key: Pubkey) {
    if let Some(account) = env.svm.get_account(&key) {
        assert_eq!(account.lamports, 0);
        assert!(account.data.is_empty());
        assert_eq!(account.owner, solana_sdk::system_program::ID);
        assert!(!account.executable);
    }
}

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rent: u64,
    refund: Option<(Pubkey, u64)>,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + signers.len()
    );
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("unsigned reserve suffix must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        // Count completed top-level repair/value instructions, including actual SPL payout.
        for program in [env.program_id, associated_token_program_id()] {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                ixs[..usize::from(index) - 2]
                    .iter()
                    .filter(|ix| ix.program_id == program)
                    .count()
            );
        }
        failure.meta
    } else {
        result.expect("authorized reserve continuation")
    };
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee + if rejected { 0 } else { rent };
        }
        if !rejected {
            if let Some((owner, amount)) = refund {
                if key == owner {
                    account.as_mut().unwrap().lamports += amount;
                }
            }
        }
        if rejected || !allowed.contains(&key) {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame {key}"
            );
        }
    }
    assert_cu_within(
        "terminal reserve destination recovery",
        meta.compute_units_consumed,
        CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_terminal_reserve_destination_repair_preserves_signer_gates_and_value() {
    let mut peak_cu = 0;
    for earnings_first in [false, true] {
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
        assert_ne!(incumbent.pubkey(), admin.pubkey());
        assert_ne!(successor.pubkey(), admin.pubkey());
        assert!(!wallets.contains(&env.payer.pubkey()));
        let ledgers = [
            state::backing_domain_ledger_account_len(),
            state::insurance_ledger_account_len(),
        ]
        .map(|len| {
            let key = Keypair::new();
            system_create_account_for_test(&mut env.svm, &env.payer, &key, len, env.program_id);
            key.pubkey()
        });
        let tracked = [env.market, env.vault, env.mint, env.vault_authority]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .chain(ledgers)
            .collect::<Vec<_>>();
        let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let ledger_frames = ledgers.map(|key| env.svm.get_account(&key).unwrap());
        let market_frame = env.svm.get_account(&env.market).unwrap();
        let token_rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        for (actor, signer) in [(2, &incumbent), (4, &admin)] {
            assert_eq!(env.token_amount(tokens[actor]), 0);
            assert_eq!(token_frames[actor].lamports, token_rent);
            let close = spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[actor],
                &wallets[actor],
                &wallets[actor],
                &[],
            )
            .unwrap();
            peak_cu = peak_cu.max(land(
                &mut env,
                &[close],
                &[signer],
                &tracked,
                &[tokens[actor]],
                0,
                Some((wallets[actor], token_rent)),
                None,
            ));
            assert_closed(&env, tokens[actor]);
        }
        let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
        let repair = |actor| Instruction {
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
        };
        let repairs = [repair(2), repair(2), repair(4)];
        let amounts = [BACKING, EARNINGS, INSURANCE];
        let payouts: [Instruction; 3] = std::array::from_fn(|kind| {
            let actor = if kind == 2 { 4 } else { 2 };
            let mut accounts = vec![
                AccountMeta::new(wallets[actor], true),
                AccountMeta::new(env.market, false),
            ];
            if kind == 1 {
                accounts.push(AccountMeta::new(ledgers[0], false));
            }
            accounts.extend([
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ]);
            if kind == 2 {
                accounts.push(AccountMeta::new(ledgers[1], false));
            }
            let market_id = env.asset_market_id(0);
            let authority_epoch = env.control_sequences(0).authority_epoch;
            let ix = match kind {
                0 => ProgInstruction::WithdrawBackingBucket {
                    domain: 1,
                    market_id,
                    authority_epoch,
                    amount: BACKING.into(),
                },
                1 => ProgInstruction::WithdrawBackingBucketEarnings {
                    domain: 1,
                    market_id,
                    authority_epoch,
                    amount: EARNINGS.into(),
                },
                2 => ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 0,
                    market_id,
                    authority_epoch,
                    amount: INSURANCE.into(),
                },
                _ => unreachable!(),
            };
            Instruction {
                program_id: env.program_id,
                accounts,
                data: ix.encode(),
            }
        });
        let unsigned = payouts.clone().map(|mut ix| {
            ix.accounts[0].is_signer = false;
            ix
        });
        let stock = |env: &V16CuEnv, paid: [u64; 3], present: [bool; 2]| {
            let group = env.market_state().1;
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let remaining = BACKING + EARNINGS + INSURANCE - paid.iter().sum::<u64>();
            let expected = [PAYOUTS[0], PAYOUTS[1], paid[0] + paid[1], 0, paid[2]];
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
            assert_eq!(expected.iter().sum::<u64>() + remaining, SUPPLY);
            for actor in 0..5 {
                if (actor == 2 && !present[0]) || (actor == 4 && !present[1]) {
                    assert_eq!(expected[actor], 0);
                    assert_closed(env, tokens[actor]);
                } else {
                    let mut account = token_frames[actor].clone();
                    let mut token = TokenAccount::unpack(&account.data).unwrap();
                    assert_eq!((token.owner, token.mint), (wallets[actor], env.mint));
                    token.amount = expected[actor];
                    TokenAccount::pack(token, &mut account.data).unwrap();
                    assert_eq!(env.svm.get_account(&tokens[actor]), Some(account));
                }
                assert_eq!(env.svm.get_account(&wallets[actor]), wallet_frames[actor]);
            }
            let mut vault = vault_frame.clone();
            let mut token = TokenAccount::unpack(&vault.data).unwrap();
            token.amount = remaining;
            TokenAccount::pack(token, &mut vault.data).unwrap();
            assert_eq!(env.svm.get_account(&env.vault), Some(vault));
            let bucket = group.source_backing_buckets[1];
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
            assert_eq!(
                group.backing_provider_earnings_total,
                bucket.utilization_fee_earnings
            );
            assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
            assert_eq!(group.insurance_domain_budget[0], group.insurance);
            assert!(group.insurance_domain_budget[1..]
                .iter()
                .all(|amount| *amount == 0));
            assert!(group
                .insurance_domain_spent
                .iter()
                .all(|amount| *amount == 0));
            let market = env.svm.get_account(&env.market).unwrap();
            assert_eq!(market.lamports, market_frame.lamports);
            crate::support::fuzz_model::assert_market_stock_census(
                "terminal reserve repair",
                &group,
                &market.data,
                &[],
                remaining.into(),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "terminal reserve repair",
                &group,
                &[],
            )
            .unwrap();
            for index in 0..2 {
                let account = env.svm.get_account(&ledgers[index]).unwrap();
                assert_eq!(
                    (
                        account.lamports,
                        account.owner,
                        account.executable,
                        account.rent_epoch
                    ),
                    (
                        ledger_frames[index].lamports,
                        ledger_frames[index].owner,
                        ledger_frames[index].executable,
                        ledger_frames[index].rent_epoch
                    )
                );
                if index == 0 && paid[1] != 0 {
                    let record = state::read_backing_domain_ledger(&account.data).unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.authority, incumbent.pubkey().to_bytes());
                    assert_eq!(record.domain, 1);
                    assert_eq!(record.total_principal_atoms, 0);
                    assert_eq!(record.total_principal_withdrawn_atoms, 0);
                    assert_eq!(record.total_deposited_atoms, 0);
                    assert_eq!(record.total_earnings_withdrawn_atoms, paid[1].into());
                    assert_eq!(
                        record.last_observed_bucket_earnings_atoms,
                        u128::from(EARNINGS - paid[1])
                    );
                    assert_eq!(
                        (
                            record.total_earnings_atoms,
                            record.cumulative_loss_atoms,
                            record.cumulative_recovery_atoms
                        ),
                        (0, 0, 0)
                    );
                } else if index == 1 && paid[2] != 0 {
                    let record = state::read_insurance_ledger(&account.data).unwrap();
                    assert_eq!(record.market_group, env.market.to_bytes());
                    assert_eq!(record.authority, admin.pubkey().to_bytes());
                    assert_eq!(
                        record.total_principal_atoms,
                        u128::from(INSURANCE - paid[2])
                    );
                    assert_eq!(record.total_withdrawn_atoms, paid[2].into());
                    assert_eq!(
                        record.last_observed_insurance_atoms,
                        u128::from(INSURANCE - paid[2])
                    );
                    assert_eq!(
                        (record.cumulative_profit_atoms, record.cumulative_loss_atoms),
                        (0, 0)
                    );
                } else {
                    assert_eq!(account, ledger_frames[index]);
                }
            }
        };
        stock(&env, [0; 3], [false; 2]);
        if !earnings_first {
            for kind in 0..3 {
                peak_cu = peak_cu.max(land(
                    &mut env,
                    &[repairs[kind].clone(), unsigned[kind].clone()],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                stock(&env, [0; 3], [false; 2]);
                let other = if kind == 2 { 0 } else { 2 };
                let signer = if kind == 2 { &admin } else { &incumbent };
                peak_cu = peak_cu.max(land(
                    &mut env,
                    &[
                        repairs[kind].clone(),
                        payouts[kind].clone(),
                        repairs[other].clone(),
                        unsigned[other].clone(),
                    ],
                    &[signer],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((5, PercolatorError::ExpectedSigner)),
                ));
                stock(&env, [0; 3], [false; 2]);
            }
        }
        let mut paid = [0; 3];
        let mut present = [false; 2];
        for kind in if earnings_first { [1, 2, 0] } else { [0, 2, 1] } {
            let reserve = usize::from(kind == 2);
            let actor = if kind == 2 { 4 } else { 2 };
            let signer = if kind == 2 { &admin } else { &incumbent };
            let mut allowed = vec![env.market, env.vault, tokens[actor]];
            if kind != 0 {
                allowed.push(ledgers[reserve]);
            }
            peak_cu = peak_cu.max(land(
                &mut env,
                &[repairs[kind].clone(), payouts[kind].clone()],
                &[signer],
                &tracked,
                &allowed,
                if present[reserve] { 0 } else { token_rent },
                None,
                None,
            ));
            paid[kind] = amounts[kind];
            present[reserve] = true;
            stock(&env, paid, present);
        }
        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = market_frame.lamports + vault_frame.lamports - tombstone_rent;
        let allowed = [env.market, env.vault];
        peak_cu = peak_cu.max(land(
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
        assert_closed(&env, env.vault);
        assert_eq!(env.token_amount(tokens[2]), BACKING + EARNINGS);
        assert_eq!(env.token_amount(tokens[4]), INSURANCE);
        assert_eq!(
            env.token_amount(tokens[3]),
            0,
            "live insurance operator is not the terminal beneficiary"
        );
    }
    eprintln!("INV-024/073 reserve repair: 2 worlds, 6 exact rollbacks, 6 authorized payouts, 4 ATA repairs, 2 slab closes; peak_CU={peak_cu}");
}
