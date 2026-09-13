//! Row 429, INV-024 with INV-005/025/027/036/070/081: terminal reserve value
//! remains bound to its beneficiary and role across depleted-insurance succession.
//!
//! Public route: System/SPL/ATA setup, signed trades and authenticated marks earn
//! 657 provider-fee atoms and consume 73 insurance atoms; resolve, user payouts
//! and portfolio deletion precede reserve payouts, consensual UpdateAssetAuthority,
//! SyncInsuranceLedger, expiry normalization and final CloseSlab. The shared loss
//! fixture uses public instructions; only Clock, blockhash and signer SOL use VM
//! controls. Decoded Account copies below are assertions, never VM state writes.
//!
//! Eight finite histories cross succession before any recovery versus after a
//! seven-atom recovery prefix, partial/full recovery with excess burn, and both
//! final fee/insurance payout orders. An input-derived oracle distinguishes each
//! beneficiary's payments and ledger, protected provider earnings, remaining spent
//! history and exact burn. A former beneficiary's ledger cannot accompany the
//! successor's payout, including rollback of a fee payment and lazy recredit.
//!
//! This adds depleted-stock/delayed-recredit succession, not funded role exchange
//! or cleanup submitter attribution. No arbitrary-history generator, unconsented
//! role transfer, live shutdown admin fallback, other asset/rail, absent signer,
//! ledger disposal or generic replay/closure claim; row 429 remains OPEN.

use super::*;

#[derive(Default)]
struct Book {
    principal_paid: u64,
    source_paid: u64,
    fees_paid: u64,
    old_paid: u64,
    new_paid: u64,
    recovered: u64,
    expired: bool,
    transferred: bool,
    new_ledger_initialized: bool,
}

#[test]
fn v16_program_depleted_insurance_succession_preserves_recovered_reserve_attribution() {
    assert_eq!((PROVIDER_FEE, SPENT, AVAILABLE), (657, 73, 176));
    let mut peak = 0;
    let mut histories = 0;
    let mut rollbacks = 0;
    for residual in [17, 101] {
        for old_recovery_prefix in [0, 7] {
            for fees_first in [false, true] {
                let recovery = residual.min(SPENT);
                let (world, old_beneficiary, admin_token) = terminal_fee_loss_world();
                let TerminalEarningsWorld {
                    mut env,
                    admin,
                    incumbent: provider,
                    successor: operator,
                    wallets,
                    tokens,
                    portfolios,
                    mint_frame,
                } = world;
                let new_beneficiary = Keypair::new();
                env.svm
                    .airdrop(&new_beneficiary.pubkey(), 1_000_000_000)
                    .unwrap();
                let new_token = create_ata_for_test(
                    &mut env.svm,
                    &env.payer,
                    new_beneficiary.pubkey(),
                    env.mint,
                );
                let all_tokens = [
                    tokens[0],
                    tokens[1],
                    tokens[2],
                    tokens[3],
                    tokens[4],
                    new_token,
                    admin_token,
                ];
                let ledgers = [
                    state::backing_domain_ledger_account_len(),
                    state::insurance_ledger_account_len(),
                    state::insurance_ledger_account_len(),
                ]
                .map(|len| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        len,
                        env.program_id,
                    );
                    key.pubkey()
                });
                let empty_ledgers = ledgers.map(|key| env.svm.get_account(&key).unwrap());
                let token_frames = all_tokens.map(|key| env.svm.get_account(&key).unwrap());
                let vault_frame = env.svm.get_account(&env.vault).unwrap();
                let config = env.market_state().0;
                let sequences = env.control_sequences(0);
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                let tracked = [
                    env.market,
                    env.vault,
                    env.mint,
                    admin.pubkey(),
                    new_beneficiary.pubkey(),
                ]
                .into_iter()
                .chain(wallets)
                .chain(all_tokens)
                .chain(ledgers)
                .chain(portfolios)
                .collect::<Vec<_>>();
                let mut book = Book::default();
                let check = |env: &V16CuEnv, book: &Book| {
                    let amounts = [
                        USER_PAID[0],
                        USER_PAID[1],
                        book.principal_paid + book.source_paid + book.fees_paid,
                        0,
                        book.old_paid,
                        book.new_paid,
                        0,
                    ];
                    let remaining = BACKING + SOURCE_PRINCIPAL + PROVIDER_FEE + AVAILABLE
                        - book.principal_paid
                        - book.source_paid
                        - book.fees_paid
                        - book.old_paid
                        - book.new_paid;
                    for ((key, frame), amount) in
                        all_tokens.into_iter().zip(&token_frames).zip(amounts)
                    {
                        let mut expected = frame.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = amount;
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(&key), Some(expected));
                    }
                    let mut expected_vault = vault_frame.clone();
                    let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
                    token.amount = remaining;
                    TokenAccount::pack(token, &mut expected_vault.data).unwrap();
                    assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                    assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg, config);
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
                    let fees = PROVIDER_FEE - book.fees_paid;
                    assert_eq!(group.backing_provider_earnings_total, fees.into());
                    let bucket = group.source_backing_buckets[1];
                    assert_eq!(bucket.utilization_fee_earnings, fees.into());
                    assert_eq!(
                        bucket.status,
                        if book.expired {
                            BackingBucketStatusV16::Expired
                        } else {
                            BackingBucketStatusV16::Fresh
                        }
                    );
                    let fresh = if book.expired {
                        0
                    } else {
                        BACKING - book.principal_paid
                    };
                    assert_eq!(
                        bucket.fresh_unliened_backing_num,
                        u128::from(fresh) * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_credit[1].fresh_reserved_backing_num,
                        u128::from(fresh) * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_credit[0].provider_receivable_num,
                        u128::from(SOURCE_PAID) * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_backing_buckets[0].fresh_unliened_backing_num,
                        u128::from(SOURCE_PRINCIPAL - book.source_paid) * BOUND_SCALE
                    );
                    let paid = book.old_paid + book.new_paid;
                    assert_eq!(
                        group.insurance,
                        u128::from(AVAILABLE + book.recovered - paid)
                    );
                    assert_eq!(
                        group.insurance_domain_spent,
                        [0, u128::from(SPENT - book.recovered)]
                    );
                    let long_paid = paid.min(INSURANCE);
                    assert_eq!(
                        group.insurance_domain_budget,
                        [
                            u128::from(INSURANCE - long_paid),
                            u128::from(INSURANCE_FEE - (paid - long_paid))
                        ]
                    );
                    assert_domain_budget_remaining_total_consistent(
                        &group,
                        "depleted beneficiary succession",
                    );
                    let mut expected_profile = profile;
                    if book.transferred {
                        expected_profile.insurance_authority = new_beneficiary.pubkey().to_bytes();
                    }
                    let mut expected_sequences = sequences;
                    expected_sequences.authority_epoch += u64::from(book.transferred);
                    assert_eq!(env.control_sequences(0), expected_sequences);
                    let mut image = env.svm.get_account(&env.market).unwrap();
                    assert_eq!(
                        state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                        expected_profile
                    );
                    state::market_view_mut(&mut image.data)
                        .unwrap()
                        .1
                        .validate_shape()
                        .unwrap();
                    crate::support::fuzz_model::assert_market_stock_census(
                        "depleted beneficiary succession",
                        &group,
                        &image.data,
                        &[],
                        remaining.into(),
                    )
                    .unwrap();
                    for index in 1..3 {
                        let initialized = if index == 1 {
                            book.old_paid != 0
                        } else {
                            book.new_ledger_initialized
                        };
                        let account = env.svm.get_account(&ledgers[index]).unwrap();
                        if !initialized {
                            assert_eq!(account, empty_ledgers[index]);
                            continue;
                        }
                        let old_recovered = if book.old_paid > AVAILABLE {
                            recovery
                        } else {
                            0
                        };
                        let expected = if index == 1 {
                            state::InsuranceLedgerAccountV16 {
                                market_group: env.market.to_bytes(),
                                authority: old_beneficiary.pubkey().to_bytes(),
                                total_withdrawn_atoms: book.old_paid.into(),
                                cumulative_profit_atoms: old_recovered.into(),
                                last_observed_insurance_atoms: u128::from(
                                    AVAILABLE + old_recovered - book.old_paid,
                                ),
                                ..Default::default()
                            }
                        } else {
                            let at_transfer = if old_recovery_prefix == 0 {
                                0
                            } else {
                                recovery - old_recovery_prefix
                            };
                            state::InsuranceLedgerAccountV16 {
                                market_group: env.market.to_bytes(),
                                authority: new_beneficiary.pubkey().to_bytes(),
                                total_withdrawn_atoms: book.new_paid.into(),
                                cumulative_profit_atoms: if old_recovery_prefix == 0 {
                                    book.recovered.into()
                                } else {
                                    0
                                },
                                last_observed_insurance_atoms: if book.new_paid == 0 {
                                    at_transfer.into()
                                } else {
                                    0
                                },
                                ..Default::default()
                            }
                        };
                        assert_eq!(
                            state::read_insurance_ledger(&account.data).unwrap(),
                            expected
                        );
                        assert_eq!(account.lamports, empty_ledgers[index].lamports);
                        assert_eq!(account.owner, env.program_id);
                    }
                    let fee_account = env.svm.get_account(&ledgers[0]).unwrap();
                    if book.fees_paid == 0 {
                        assert_eq!(fee_account, empty_ledgers[0]);
                    } else {
                        let ledger = state::read_backing_domain_ledger(&fee_account.data).unwrap();
                        assert_eq!(
                            (ledger.market_group, ledger.authority, ledger.domain),
                            (env.market.to_bytes(), provider.pubkey().to_bytes(), 1)
                        );
                        assert_eq!(
                            (
                                ledger.total_earnings_withdrawn_atoms,
                                ledger.last_observed_bucket_earnings_atoms
                            ),
                            (book.fees_paid.into(), fees.into())
                        );
                        assert_eq!(
                            (
                                ledger.total_principal_atoms,
                                ledger.total_deposited_atoms,
                                ledger.total_principal_withdrawn_atoms,
                                ledger.total_earnings_atoms
                            ),
                            (0, 0, 0, 0)
                        );
                    }
                };
                let reserve = |env: &V16CuEnv, kind, amount, new: bool, ledger| {
                    let mut recipients = wallets;
                    let mut destinations = tokens;
                    if new {
                        recipients[4] = new_beneficiary.pubkey();
                        destinations[4] = new_token;
                    }
                    let mut ix =
                        reserve_payout(env, recipients, destinations, ledger, kind, amount);
                    if kind == 2 {
                        ix.accounts.push(AccountMeta::new(ledger, false));
                    }
                    ix
                };
                let close = |env: &V16CuEnv| Instruction {
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
                check(&env, &book);
                let mut source = reserve(&env, 0, SOURCE_PRINCIPAL, false, ledgers[0]);
                source.data = ProgInstruction::WithdrawBackingBucket {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    amount: SOURCE_PRINCIPAL.into(),
                }
                .encode();
                let principal = reserve(&env, 0, BACKING - residual, false, ledgers[0]);
                let available = reserve(&env, 2, AVAILABLE, false, ledgers[1]);
                let allowed = [env.market, env.vault, tokens[2], tokens[4], ledgers[1]];
                peak = peak.max(land(
                    &mut env,
                    &[source, principal, available],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                book.source_paid = SOURCE_PRINCIPAL;
                book.principal_paid = BACKING - residual;
                book.old_paid = AVAILABLE;
                check(&env, &book);
                assert_eq!(env.market_state().1.insurance, 0);
                assert_eq!(
                    env.market_state().1.insurance_domain_spent,
                    [0, SPENT.into()]
                );

                if old_recovery_prefix != 0 {
                    env.svm.warp_to_slot(100);
                    let ix = close(&env);
                    let allowed = [env.market];
                    peak = peak.max(land(
                        &mut env,
                        &[ix],
                        &[&admin],
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    book.expired = true;
                    check(&env, &book);
                    let ix = reserve(&env, 2, old_recovery_prefix, false, ledgers[1]);
                    let allowed = [env.market, env.vault, tokens[4], ledgers[1]];
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
                    book.recovered = recovery;
                    book.old_paid += old_recovery_prefix;
                    check(&env, &book);
                }
                let handoff = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(old_beneficiary.pubkey(), true),
                        AccountMeta::new_readonly(new_beneficiary.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::UpdateAssetAuthority {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        kind: processor::ASSET_AUTH_INSURANCE,
                        new_pubkey: new_beneficiary.pubkey().to_bytes(),
                    }
                    .encode(),
                };
                let sync = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(new_beneficiary.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(ledgers[2], false),
                    ],
                    data: ProgInstruction::SyncInsuranceLedger.encode(),
                };
                let allowed = [env.market, ledgers[2]];
                peak = peak.max(land(
                    &mut env,
                    &[handoff, sync],
                    &[&old_beneficiary, &new_beneficiary],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                book.transferred = true;
                book.new_ledger_initialized = true;
                check(&env, &book);
                let old_ledger_frame = env.svm.get_account(&ledgers[1]);
                if !book.expired {
                    env.svm.warp_to_slot(100);
                    let ix = close(&env);
                    let allowed = [env.market];
                    peak = peak.max(land(
                        &mut env,
                        &[ix],
                        &[&admin],
                        &tracked,
                        &allowed,
                        0,
                        None,
                        None,
                    ));
                    book.expired = true;
                    check(&env, &book);
                }

                let fees = reserve(&env, 1, PROVIDER_FEE, false, ledgers[0]);
                let tail = recovery - old_recovery_prefix;
                let wrong_ledger = reserve(&env, 2, tail, true, ledgers[1]);
                peak = peak.max(land(
                    &mut env,
                    &[fees.clone(), wrong_ledger],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::Unauthorized)),
                ));
                rollbacks += 1;
                check(&env, &book);
                let payment = reserve(&env, 2, tail, true, ledgers[2]);
                for (is_fee, ix) in if fees_first {
                    [(true, fees), (false, payment)]
                } else {
                    [(false, payment), (true, fees)]
                } {
                    let allowed = [
                        env.market,
                        env.vault,
                        if is_fee { tokens[2] } else { new_token },
                        if is_fee { ledgers[0] } else { ledgers[2] },
                    ];
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
                    if is_fee {
                        book.fees_paid = PROVIDER_FEE;
                    } else {
                        book.new_paid = tail;
                        book.recovered = recovery;
                    }
                    check(&env, &book);
                    assert_eq!(env.svm.get_account(&ledgers[1]), old_ledger_frame);
                }
                // Depleted spent history and burnable stock cannot repay either ledger.
                if residual > recovery {
                    let overdraw = reserve(&env, 2, 1, true, ledgers[2]);
                    peak = peak.max(land(
                        &mut env,
                        &[overdraw],
                        &[],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((2, PercolatorError::EngineLockActive)),
                    ));
                    rollbacks += 1;
                    check(&env, &book);
                }
                let settled_tokens = all_tokens.map(|key| env.svm.get_account(&key));
                let settled_ledgers = ledgers.map(|key| env.svm.get_account(&key));
                let tombstone_rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                let refund = env.svm.get_account(&env.market).unwrap().lamports
                    + env.svm.get_account(&env.vault).unwrap().lamports
                    - tombstone_rent;
                let ix = close(&env);
                let allowed = [env.market, env.vault, env.mint];
                peak = peak.max(land(
                    &mut env,
                    &[ix],
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
                assert_eq!(
                    all_tokens.map(|key| env.svm.get_account(&key)),
                    settled_tokens
                );
                assert_eq!(
                    ledgers.map(|key| env.svm.get_account(&key)),
                    settled_ledgers
                );
                let burned = residual - recovery;
                let mut expected_mint = mint_frame;
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                mint.supply -= burned;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                assert_eq!(
                    all_tokens
                        .map(|key| env.token_amount(key))
                        .iter()
                        .sum::<u64>(),
                    SUPPLY - burned
                );
                assert_eq!(env.token_amount(tokens[4]), AVAILABLE + old_recovery_prefix);
                assert_eq!(env.token_amount(new_token), recovery - old_recovery_prefix);
                assert_eq!(
                    env.token_amount(tokens[2]),
                    BACKING - residual + SOURCE_PRINCIPAL + PROVIDER_FEE
                );
                assert_eq!(env.token_amount(tokens[3]), 0);
                assert_eq!(env.token_amount(admin_token), 0);
                assert_eq!(operator.pubkey(), wallets[3]);
                histories += 1;
            }
        }
    }
    assert_eq!((histories, rollbacks), (8, 12));
    assert_cu_within("depleted beneficiary succession", peak, 700_000);
    eprintln!("row429 depleted beneficiary succession: {histories} histories, {rollbacks} exact rollbacks, peak_CU={peak}");
}
