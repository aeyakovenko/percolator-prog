//! INV-024 / row 429: a returning reserve holder inherits only the unpaid fee tail.
//! Reusing that holder's ledger neither restores old consent nor replenishes fees
//! paid during the intervening tenure. Insurance remains a separate entitlement.

use super::*;

const CU_LIMIT: u32 = 1_200_000;

fn sign(env: &V16CuEnv, ixs: &[Instruction], signers: &[&Keypair], tag: u32) -> Transaction {
    let ixs = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT - tag),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let signers = [&env.payer]
        .into_iter()
        .chain(signers.iter().copied())
        .collect::<Vec<_>>();
    Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    )
}

// Retained transactions are delivered without rebuilding their message or signatures.
fn deliver(
    env: &mut V16CuEnv,
    tx: Transaction,
    tracked: &[Pubkey],
    rejection: Option<(u8, PercolatorError, usize)>,
) {
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let writable = tx
        .message
        .account_keys
        .iter()
        .enumerate()
        .filter(|(index, _)| tx.message.is_writable(*index))
        .map(|(_, key)| *key)
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error, spl_payouts)) = rejection {
        let failure = result.expect_err("retained consent or exhausted entitlement must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            usize::from(index - 2)
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", spl_token::ID))
                .count(),
            spl_payouts
        );
        failure.meta
    } else {
        result.expect("authorized terminal continuation")
    };
    assert!(meta.compute_units_consumed <= u64::from(CU_LIMIT));
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !writable.contains(&key) || key == env.payer.pubkey() {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame {key}"
            );
        }
    }
}

#[test]
fn v16_program_terminal_provider_roundtrip_preserves_intervening_fee_payouts() {
    const FIRST: u64 = 17;
    const RETAINED: u64 = 29;
    for intermediate in [19, EARNINGS - FIRST - RETAINED + 1] {
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
        let ledgers = [Keypair::new(), Keypair::new()].map(|key| {
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            key.pubkey()
        });
        let tracked = [env.market, env.vault, env.mint]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .chain(ledgers)
            .collect::<Vec<_>>();
        let profiles =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        let sequences = env.control_sequences(0);
        let terminal = env.market_state().1;
        let wrap = |ix: ProgInstruction, accounts| Instruction {
            program_id: env.program_id,
            accounts,
            data: ix.encode(),
        };
        let earnings = |actor: usize, amount: u64, epoch: u64, ledger: Pubkey| {
            wrap(
                ProgInstruction::WithdrawBackingBucketEarnings {
                    domain: 1,
                    market_id: env.asset_market_id(0),
                    authority_epoch: epoch,
                    amount: amount.into(),
                },
                vec![
                    AccountMeta::new(wallets[actor], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(ledger, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            )
        };
        let rotate = |from: usize, to: usize, epoch| {
            wrap(
                ProgInstruction::UpdateAssetAuthority {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: epoch,
                    kind: processor::ASSET_AUTH_BACKING_BUCKET,
                    new_pubkey: wallets[to].to_bytes(),
                },
                vec![
                    AccountMeta::new(wallets[from], true),
                    AccountMeta::new_readonly(wallets[to], true),
                    AccountMeta::new(env.market, false),
                ],
            )
        };
        let principal = wrap(
            ProgInstruction::WithdrawBackingBucket {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: sequences.authority_epoch,
                amount: BACKING.into(),
            },
            vec![
                AccountMeta::new(wallets[2], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        let first = earnings(2, FIRST, sequences.authority_epoch, ledgers[0]);
        let old_ix = earnings(2, RETAINED, sequences.authority_epoch, ledgers[0]);
        let outward = rotate(2, 3, sequences.authority_epoch);
        let middle = earnings(3, intermediate, sequences.authority_epoch + 1, ledgers[1]);
        let returning = rotate(3, 2, sequences.authority_epoch + 1);
        let current = earnings(2, RETAINED, sequences.authority_epoch + 2, ledgers[0]);
        let remaining = EARNINGS - FIRST - intermediate;
        let exact = earnings(2, remaining, sequences.authority_epoch + 2, ledgers[0]);
        let overdraw = earnings(2, remaining + 1, sequences.authority_epoch + 2, ledgers[0]);
        let wrong_ledger = earnings(2, remaining, sequences.authority_epoch + 2, ledgers[1]);
        let mut unsigned = exact.clone();
        unsigned.accounts[0].is_signer = false;
        let mut readonly = exact.clone();
        readonly.accounts[2].is_writable = false;
        let insurance = wrap(
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: sequences.authority_epoch + 2,
                amount: INSURANCE.into(),
            },
            vec![
                AccountMeta::new(wallets[4], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        let close = wrap(
            ProgInstruction::CloseSlab {
                authority_epoch: sequences.authority_epoch + 2,
            },
            vec![
                AccountMeta::new(wallets[4], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
        );
        // Leave separate insurance liquidity in custody so overdraw reaches the fee-stock gate.
        let insurance_prefix = Instruction {
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: sequences.authority_epoch + 2,
                amount: 1,
            }
            .encode(),
            ..insurance.clone()
        };

        let check = |env: &V16CuEnv, fees: [u64; 2], insurer_paid: bool, handoffs: u64| {
            let insurance_paid = if insurer_paid { INSURANCE } else { 0 };
            let amounts = [
                PAYOUTS[0],
                PAYOUTS[1],
                BACKING + fees[0],
                fees[1],
                insurance_paid,
            ];
            for ((key, owner), amount) in tokens.into_iter().zip(wallets).zip(amounts) {
                let account = env.svm.get_account(&key).unwrap();
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(
                    (token.owner, token.mint, token.amount),
                    (owner, env.mint, amount)
                );
            }
            let group = env.market_state().1;
            let unpaid = EARNINGS - fees.iter().sum::<u64>();
            let reserve = unpaid + INSURANCE - insurance_paid;
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(group.backing_provider_earnings_total, unpaid.into());
            let mut bucket = terminal.source_backing_buckets[1];
            bucket.fresh_unliened_backing_num = 0;
            // Full principal exit leaves the historical consumed lien, but no fresh stock.
            bucket.status = BackingBucketStatusV16::Expired;
            bucket.utilization_fee_earnings = unpaid.into();
            assert_eq!(group.source_backing_buckets[1], bucket);
            let mut credit = terminal.source_credit[1];
            credit.fresh_reserved_backing_num = 0;
            credit.credit_epoch += 1;
            assert_eq!(group.source_credit[1], credit);
            assert_eq!(
                group.source_backing_buckets[0],
                terminal.source_backing_buckets[0]
            );
            assert_eq!(group.source_credit[0], terminal.source_credit[0]);
            assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
            assert_eq!(group.insurance_domain_budget[0], group.insurance);
            assert!(group.insurance_domain_budget[1..]
                .iter()
                .all(|value| *value == 0));
            assert_eq!(
                group.insurance_domain_spent,
                terminal.insurance_domain_spent
            );
            assert_domain_budget_remaining_total_consistent(&group, "terminal provider roundtrip");
            assert_eq!(group.vault, reserve.into());
            assert_eq!(env.token_amount(env.vault), reserve);
            assert_eq!(amounts.iter().sum::<u64>() + reserve, SUPPLY);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            let mut expected_profile = profiles;
            expected_profile.backing_bucket_authority =
                wallets[if handoffs == 1 { 3 } else { 2 }].to_bytes();
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
            state::market_view_mut(&mut env.svm.get_account(&env.market).unwrap().data)
                .unwrap()
                .1
                .validate_shape()
                .unwrap();
        };
        let tx = sign(&env, &[principal, first], &[&incumbent], 0);
        deliver(&mut env, tx, &tracked, None);
        check(&env, [FIRST, 0], false, 0);
        let old_ledger = env.svm.get_account(&ledgers[0]).unwrap();
        let retained = sign(&env, &[old_ix.clone()], &[&incumbent], 1);
        let before = tracked
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        env.svm
            .simulate_transaction(retained.clone().into())
            .expect("retained fee consent is initially admissible");
        assert_eq!(
            tracked
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            before
        );

        // The complete return, both fee payouts and lazy B-ledger initialization
        // execute before the old-epoch suffix rejects and rolls everything back.
        let roundtrip = [
            outward.clone(),
            middle.clone(),
            returning.clone(),
            exact.clone(),
        ];
        let mut rejected_bundle = roundtrip.to_vec();
        rejected_bundle.push(old_ix);
        let tx = sign(&env, &rejected_bundle, &[&incumbent, &successor], 2);
        deliver(
            &mut env,
            tx,
            &tracked,
            Some((6, PercolatorError::EngineStale, 2)),
        );
        check(&env, [FIRST, 0], false, 0);
        let tx = sign(&env, &[outward, middle], &[&incumbent, &successor], 3);
        deliver(&mut env, tx, &tracked, None);
        check(&env, [FIRST, intermediate], false, 1);
        assert_eq!(env.svm.get_account(&ledgers[0]), Some(old_ledger));
        let middle_ledger = env.svm.get_account(&ledgers[1]).unwrap();
        let middle_record = state::read_backing_domain_ledger(&middle_ledger.data).unwrap();
        assert_eq!(middle_record.authority, wallets[3].to_bytes());
        assert_eq!(
            middle_record.total_earnings_withdrawn_atoms,
            intermediate.into()
        );
        assert_eq!(
            middle_record.last_observed_bucket_earnings_atoms,
            remaining.into()
        );
        assert_eq!(middle_record.total_earnings_atoms, 0);
        let tx = sign(&env, &[returning], &[&incumbent, &successor], 4);
        deliver(&mut env, tx, &tracked, None);
        check(&env, [FIRST, intermediate], false, 2);
        assert_eq!(
            retained.message.recent_blockhash,
            env.svm.latest_blockhash()
        );
        deliver(
            &mut env,
            retained,
            &tracked,
            Some((2, PercolatorError::EngineStale, 0)),
        );

        for (tag, suffix, signers, error) in [
            (
                5,
                overdraw,
                vec![&admin, &incumbent],
                PercolatorError::EngineLockActive,
            ),
            (
                6,
                wrong_ledger,
                vec![&admin, &incumbent],
                PercolatorError::Unauthorized,
            ),
            (7, unsigned, vec![&admin], PercolatorError::ExpectedSigner),
            (
                8,
                readonly,
                vec![&admin, &incumbent],
                PercolatorError::ExpectedWritable,
            ),
        ] {
            let tx = sign(&env, &[insurance_prefix.clone(), suffix], &signers, tag);
            deliver(&mut env, tx, &tracked, Some((3, error, 1)));
            check(&env, [FIRST, intermediate], false, 2);
        }
        let tx = sign(&env, &[current], &[&incumbent], 9);
        let final_fee = if remaining < RETAINED {
            deliver(
                &mut env,
                tx,
                &tracked,
                Some((2, PercolatorError::EngineLockActive, 0)),
            );
            exact
        } else {
            deliver(&mut env, tx, &tracked, None);
            check(&env, [FIRST + RETAINED, intermediate], false, 2);
            let mut tail = exact;
            tail.data = ProgInstruction::WithdrawBackingBucketEarnings {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: sequences.authority_epoch + 2,
                amount: u128::from(remaining - RETAINED),
            }
            .encode();
            tail
        };
        let tx = sign(&env, &[final_fee, insurance], &[&incumbent, &admin], 10);
        deliver(&mut env, tx, &tracked, None);
        check(&env, [EARNINGS - intermediate, intermediate], true, 2);
        assert_eq!(env.svm.get_account(&ledgers[1]), Some(middle_ledger));
        let record =
            state::read_backing_domain_ledger(&env.svm.get_account(&ledgers[0]).unwrap().data)
                .unwrap();
        assert_eq!(
            (record.market_group, record.authority, record.domain),
            (env.market.to_bytes(), wallets[2].to_bytes(), 1)
        );
        assert_eq!(
            record.total_earnings_withdrawn_atoms,
            u128::from(EARNINGS - intermediate)
        );
        assert_eq!(
            (
                record.total_earnings_atoms,
                record.last_observed_bucket_earnings_atoms,
                record.total_principal_atoms
            ),
            (0, 0, 0)
        );
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let vault_rent = env.svm.get_account(&env.vault).unwrap().lamports;
        let admin_lamports = env.svm.get_account(&admin.pubkey()).unwrap().lamports;
        let tx = sign(&env, &[close], &[&admin], 11);
        deliver(&mut env, tx, &tracked, None);
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(
            env.svm.get_account(&admin.pubkey()).unwrap().lamports,
            admin_lamports + market_rent + vault_rent - tombstone.lamports
        );
        assert!(env
            .svm
            .get_account(&env.vault)
            .is_none_or(|account| account.lamports == 0));
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [
                PAYOUTS[0],
                PAYOUTS[1],
                BACKING + EARNINGS - intermediate,
                intermediate,
                INSURANCE
            ]
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
        eprintln!("terminal provider roundtrip: intermediate={intermediate}, returned_tail={remaining}, exact earned fees={EARNINGS}; full retirement");
    }
}
