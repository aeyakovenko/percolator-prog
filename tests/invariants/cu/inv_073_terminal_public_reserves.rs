//! INV-073: partial public reserve payments compose with expiry and mechanical close.
//! Late expiry during unfinished user settlement also preserves unsigned reserve exits.

use super::*;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_pending_users_cross_late_expiry_before_unsigned_reserve_payouts() {
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };

    const LIMIT: u64 = 600_000;
    let mut peak = 0;
    let mut user_calls = 0;
    let mut rollbacks = 0;
    let mut positive_claim_expiries = 0;
    let mut waiting_claimants = 0;
    for delivery in [100, 101] {
        for expire_between in [false, true] {
            for winner_first in [false, true] {
                for insurance_first in [false, true] {
                    let (world, users) = terminal_earnings_world_with_user_signers(false, None);
                    let TerminalEarningsWorld {
                        mut env,
                        admin,
                        incumbent,
                        successor,
                        mut wallets,
                        mut tokens,
                        portfolios,
                        mint_frame,
                    } = world;
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
                    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                    let vault_frame = env.svm.get_account(&env.vault).unwrap();
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::close_account(
                            &spl_token::ID,
                            &tokens[4],
                            &wallets[4],
                            &wallets[4],
                            &[],
                        )
                        .unwrap(),
                        &[&beneficiary],
                    )
                    .unwrap();
                    drop((incumbent, successor, beneficiary));
                    assert!(!wallets.contains(&env.payer.pubkey()));
                    assert!(!wallets.contains(&admin.pubkey()));
                    assert!(env
                        .svm
                        .get_account(&tokens[4])
                        .is_none_or(|a| a.lamports == 0));
                    let closed_insurance = env.svm.get_account(&tokens[4]);
                    let market_frame = env.svm.get_account(&env.market).unwrap();
                    let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
                    let epoch = env.control_sequences(0).authority_epoch;
                    let ledger = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &ledger,
                        state::backing_domain_ledger_account_len(),
                        env.program_id,
                    );
                    let ledger = ledger.pubkey();
                    let empty_ledger = env.svm.get_account(&ledger);
                    let tracked = [
                        env.market,
                        env.vault,
                        env.mint,
                        ledger,
                        admin.pubkey(),
                        admin_token,
                    ]
                    .into_iter()
                    .chain(wallets)
                    .chain(tokens)
                    .chain(portfolios)
                    .collect::<Vec<_>>();
                    let repair = Instruction {
                        program_id: associated_token_program_id(),
                        accounts: vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(tokens[4], false),
                            AccountMeta::new_readonly(wallets[4], false),
                            AccountMeta::new_readonly(env.mint, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: vec![1],
                    };
                    let reserves = std::array::from_fn::<_, 3, _>(|kind| {
                        reserve_payout(&env, wallets, tokens, ledger, kind, 1)
                    });
                    let payouts = std::array::from_fn::<_, 2, _>(|actor| Instruction {
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
                    });
                    let run = |env: &mut V16CuEnv,
                               ixs: &[Instruction],
                               signers: &[&Keypair],
                               allowed: &[Pubkey],
                               rent,
                               refund,
                               rejection| {
                        for meta in ixs.iter().flat_map(|ix| &ix.accounts) {
                            assert!(!meta.is_signer || !wallets[2..].contains(&meta.pubkey));
                        }
                        let cu = land(
                            env, ixs, signers, &tracked, allowed, rent, refund, rejection,
                        );
                        assert_cu_within("INV-073 pending users and late expiry", cu, LIMIT);
                        cu
                    };
                    let stock = |env: &V16CuEnv, fees_paid: u64, insurance_paid: u64| {
                        let group = env.market_state().1;
                        let amounts = tokens.map(|key| {
                            env.svm
                                .get_account(&key)
                                .filter(|a| a.lamports != 0)
                                .map_or(0, |a| TokenAccount::unpack(&a.data).unwrap().amount)
                        });
                        assert_eq!(amounts[2..], [fees_paid, 0, insurance_paid]);
                        for actor in 0..2 {
                            assert!(amounts[actor] <= PAYOUTS[actor]);
                        }
                        for actor in 0..5 {
                            if actor == 4 && insurance_paid == 0 {
                                assert_eq!(env.svm.get_account(&tokens[actor]), closed_insurance);
                                continue;
                            }
                            let mut expected = token_frames[actor].clone();
                            let mut token = TokenAccount::unpack(&expected.data).unwrap();
                            assert_eq!(token.owner, wallets[actor]);
                            assert_eq!(token.mint, env.mint);
                            token.amount = amounts[actor];
                            TokenAccount::pack(token, &mut expected.data).unwrap();
                            assert_eq!(env.svm.get_account(&tokens[actor]), Some(expected));
                        }
                        assert_eq!(
                            amounts.iter().sum::<u64>() + env.token_amount(env.vault),
                            SUPPLY
                        );
                        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                        let mut expected = vault_frame.clone();
                        let mut vault = TokenAccount::unpack(&expected.data).unwrap();
                        vault.amount = SUPPLY - amounts.iter().sum::<u64>();
                        TokenAccount::pack(vault, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(&env.vault), Some(expected));
                        assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
                        assert_eq!(group.insurance_domain_budget[0], group.insurance);
                        assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
                        assert_domain_budget_remaining_total_consistent(&group, "pending expiry");
                        assert_eq!(
                            group.backing_provider_earnings_total,
                            u128::from(EARNINGS - fees_paid)
                        );
                        assert_eq!(
                            group.source_backing_buckets[1].utilization_fee_earnings,
                            u128::from(EARNINGS - fees_paid)
                        );
                        assert_eq!(env.token_amount(admin_token), 0);
                        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                        let mut image = env.svm.get_account(&env.market).unwrap();
                        assert_eq!(
                            state::read_asset_oracle_profile(&image.data, 0).unwrap(),
                            profile
                        );
                        assert_eq!(
                            env.control_sequences(0).authority_epoch,
                            epoch + u64::from(insurance_paid != 0)
                        );
                        let accounts = portfolios
                            .iter()
                            .filter(|key| env.svm.get_account(key).is_some_and(|a| a.lamports != 0))
                            .map(|key| env.portfolio_state(*key))
                            .collect::<Vec<_>>();
                        assert_market_stock_census(
                            "pending expiry",
                            &group,
                            &image.data,
                            &accounts,
                            group.vault,
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census("pending expiry", &group, &accounts)
                            .unwrap();
                        state::market_view_mut(&mut image.data)
                            .unwrap()
                            .1
                            .validate_shape()
                            .unwrap();
                        if fees_paid == 0 {
                            assert_eq!(env.svm.get_account(&ledger), empty_ledger);
                        } else {
                            let account = env.svm.get_account(&ledger).unwrap();
                            let record = state::read_backing_domain_ledger(&account.data).unwrap();
                            assert_eq!(account.lamports, empty_ledger.as_ref().unwrap().lamports);
                            assert_eq!(record.market_group, env.market.to_bytes());
                            assert_eq!(record.authority, wallets[2].to_bytes());
                            assert_eq!(record.total_earnings_withdrawn_atoms, fees_paid.into());
                            assert_eq!(
                                record.last_observed_bucket_earnings_atoms,
                                u128::from(EARNINGS - fees_paid)
                            );
                            assert_eq!(record.total_principal_withdrawn_atoms, 0);
                        }
                    };
                    env.resolve();
                    env.svm.warp_to_slot(7);
                    stock(&env, 0, 0);
                    let rank = |env: &V16CuEnv| {
                        let accounts = portfolios.map(|p| env.portfolio_state(p));
                        [
                            env.market_state().1.source_backing_buckets[..2]
                                .iter()
                                .filter(|bucket| bucket.status == BackingBucketStatusV16::Fresh)
                                .count() as u128,
                            env.market_state().1.source_backing_buckets[..2]
                                .iter()
                                .map(|bucket| bucket.impaired_liened_backing_num)
                                .sum(),
                            accounts
                                .iter()
                                .map(|p| {
                                    u128::from(percolator::active_bitmap_count_ones(active_bitmap(
                                        p,
                                    )))
                                })
                                .sum(),
                            accounts
                                .iter()
                                .flat_map(|p| p.source_domains.iter())
                                .map(|source| source.source_claim_liened_num.get())
                                .sum(),
                            accounts
                                .iter()
                                .flat_map(|p| p.source_domains.iter())
                                .filter(|source| source.is_occupied())
                                .count() as u128,
                            accounts
                                .iter()
                                .map(|p| p.pnl.get().min(0).unsigned_abs())
                                .sum(),
                            (0..2)
                                .map(|i| u128::from(PAYOUTS[i] - env.token_amount(tokens[i])))
                                .sum(),
                        ]
                    };
                    let order = if winner_first { [0, 1] } else { [1, 0] };
                    let mut calls = 0;
                    for turn in 0..8 {
                        if turn == usize::from(expire_between) {
                            let before = env.svm.get_account(&env.market);
                            let group = env.market_state().1;
                            assert!(group.c_tot > 0);
                            assert!(portfolios
                                .iter()
                                .any(|p| !resolved_portfolio_is_terminal(&env, *p)));
                            assert_eq!(
                                group.source_backing_buckets[1].status,
                                BackingBucketStatusV16::Fresh
                            );
                            positive_claim_expiries +=
                                usize::from(group.source_claim_bound_total_num > 0);
                            env.svm.warp_to_slot(delivery);
                            assert_eq!(env.svm.get_account(&env.market), before);
                            // A funded ATA prefix cannot turn a pending user claim into insurance.
                            for kind in 0..3 {
                                let ixs = if kind == 2 {
                                    vec![repair.clone(), reserves[kind].clone()]
                                } else {
                                    vec![reserves[kind].clone()]
                                };
                                peak = peak.max(run(
                                    &mut env,
                                    &ixs,
                                    &[],
                                    &[],
                                    0,
                                    None,
                                    Some((1 + ixs.len() as u8, PercolatorError::EngineLockActive)),
                                ));
                                rollbacks += 1;
                                stock(&env, 0, 0);
                            }
                        }
                        let actor = order[turn % 2];
                        if !resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                            if expire_between && winner_first && turn == 2 {
                                // The winner detached before expiry. After the peer normalizes
                                // backing, that peer must still settle before the winner can pay.
                                assert!(percolator::active_bitmap_is_empty(active_bitmap(
                                    &env.portfolio_state(portfolios[0])
                                )));
                                assert!(!percolator::active_bitmap_is_empty(active_bitmap(
                                    &env.portfolio_state(portfolios[1])
                                )));
                                assert_eq!(env.token_amount(tokens[0]), 0);
                                peak = peak.max(run(
                                    &mut env,
                                    &[payouts[actor].clone()],
                                    &[],
                                    &[],
                                    0,
                                    None,
                                    Some((2, PercolatorError::EngineNonProgress)),
                                ));
                                rollbacks += 1;
                                waiting_claimants += 1;
                                stock(&env, 0, 0);
                                continue;
                            }
                            // The exact successful settlement prefix must roll back with a
                            // forbidden reserve suffix, including any expiry and SPL payout.
                            peak = peak.max(run(
                                &mut env,
                                &[payouts[actor].clone(), reserves[1].clone()],
                                &[],
                                &[],
                                0,
                                None,
                                Some((3, PercolatorError::EngineLockActive)),
                            ));
                            rollbacks += 1;
                            stock(&env, 0, 0);
                            let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
                            let before = rank(&env);
                            peak = peak.max(run(
                                &mut env,
                                &[payouts[actor].clone()],
                                &[],
                                &allowed,
                                0,
                                None,
                                None,
                            ));
                            assert!(
                                rank(&env) < before,
                                "expiry/leg/payout rank must decrease: {before:?} -> {:?}",
                                rank(&env)
                            );
                            calls += 1;
                            stock(&env, 0, 0);
                        }
                        if portfolios
                            .iter()
                            .all(|p| resolved_portfolio_is_terminal(&env, *p))
                        {
                            break;
                        }
                    }
                    user_calls += calls;
                    for actor in 0..2 {
                        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
                        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
                    }
                    assert_eq!(env.market_state().1.c_tot, 0);
                    assert_eq!(env.market_state().1.materialized_portfolio_count, 2);
                    peak = peak.max(run(
                        &mut env,
                        &[repair.clone(), reserves[2].clone()],
                        &[],
                        &[],
                        0,
                        None,
                        Some((3, PercolatorError::EngineLockActive)),
                    ));
                    rollbacks += 1;
                    for actor in order {
                        let delete = Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(wallets[actor], true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[actor], false),
                            ],
                            data: env.close_portfolio_ix(portfolios[actor]).encode(),
                        };
                        let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
                        let portfolio_lamports =
                            env.svm.get_account(&portfolios[actor]).unwrap().lamports;
                        let allowed = [env.market, portfolios[actor]];
                        peak = peak.max(run(
                            &mut env,
                            &[delete],
                            &[&users[actor]],
                            &allowed,
                            0,
                            None,
                            None,
                        ));
                        assert_eq!(
                            env.svm.get_account(&env.market).unwrap().lamports,
                            market_lamports + portfolio_lamports
                        );
                        assert!(env
                            .svm
                            .get_account(&portfolios[actor])
                            .is_none_or(|a| a.lamports == 0));
                        stock(&env, 0, 0);
                    }
                    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
                    assert_eq!(env.market_state().1.source_claim_bound_total_num, 0);
                    assert_eq!(
                        env.market_state().1.source_backing_buckets[1].status,
                        BackingBucketStatusV16::Expired
                    );
                    // Retained principal bytes now reject for expiry, after the user gate clears.
                    peak = peak.max(run(
                        &mut env,
                        &[reserves[0].clone()],
                        &[],
                        &[],
                        0,
                        None,
                        Some((2, PercolatorError::EngineStale)),
                    ));
                    rollbacks += 1;
                    let mut paid = [0; 2];
                    for kind in if insurance_first { [2, 1] } else { [1, 2] } {
                        let amount = if kind == 1 { EARNINGS } else { INSURANCE };
                        let payout = reserve_payout(&env, wallets, tokens, ledger, kind, amount);
                        let ixs = if kind == 2 {
                            vec![repair.clone(), payout]
                        } else {
                            vec![payout]
                        };
                        let mut aborted = ixs.clone();
                        let mut expired_principal =
                            reserve_payout(&env, wallets, tokens, ledger, 0, 1);
                        expired_principal.data = ProgInstruction::WithdrawBackingBucket {
                            domain: 1,
                            market_id: env.asset_market_id(0),
                            authority_epoch: env.control_sequences(0).authority_epoch
                                + u64::from(kind == 2),
                            amount: 1,
                        }
                        .encode();
                        aborted.push(expired_principal);
                        peak = peak.max(run(
                            &mut env,
                            &aborted,
                            &[],
                            &[],
                            0,
                            None,
                            Some((1 + aborted.len() as u8, PercolatorError::EngineStale)),
                        ));
                        rollbacks += 1;
                        stock(&env, paid[0], paid[1]);
                        let actor = if kind == 1 { 2 } else { 4 };
                        let allowed = [env.market, env.vault, ledger, tokens[actor]];
                        let rent = if kind == 2 {
                            env.svm
                                .minimum_balance_for_rent_exemption(TokenAccount::LEN)
                        } else {
                            0
                        };
                        peak = peak.max(run(&mut env, &ixs, &[], &allowed, rent, None, None));
                        paid[kind - 1] = amount;
                        stock(&env, paid[0], paid[1]);
                    }
                    for kind in [1, 2] {
                        let replay = reserve_payout(&env, wallets, tokens, ledger, kind, 1);
                        peak = peak.max(run(
                            &mut env,
                            &[replay],
                            &[],
                            &[],
                            0,
                            None,
                            Some((2, PercolatorError::EngineLockActive)),
                        ));
                        rollbacks += 1;
                        stock(&env, EARNINGS, INSURANCE);
                    }
                    let group = env.market_state().1;
                    assert_eq!(group.vault, BACKING.into());
                    assert_eq!(group.pnl_pos_tot, 0);
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
                    assert_eq!(
                        group.source_backing_buckets[1].status,
                        BackingBucketStatusV16::Expired
                    );
                    assert_eq!(
                        group.source_backing_buckets[1].fresh_unliened_backing_num,
                        0
                    );
                    assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
                    let rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                    let refund = env.svm.get_account(&env.market).unwrap().lamports
                        + env.svm.get_account(&env.vault).unwrap().lamports
                        - rent;
                    let allowed = [env.market, env.vault, env.mint];
                    peak = peak.max(run(
                        &mut env,
                        &[close],
                        &[&admin],
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
                    let mut expected_mint = mint_frame;
                    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                    mint.supply = SUPPLY - BACKING;
                    Mint::pack(mint, &mut expected_mint.data).unwrap();
                    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                    assert_eq!(
                        tokens.map(|key| env.token_amount(key)),
                        [PAYOUTS[0], PAYOUTS[1], EARNINGS, 0, INSURANCE]
                    );
                    eprintln!("pending reserve expiry: delivery={delivery}, between={expire_between}, winner_first={winner_first}, insurance_first={insurance_first}, user_calls={calls}");
                }
            }
        }
    }
    assert_eq!(positive_claim_expiries, 16);
    assert_eq!(waiting_claimants, 4);
    eprintln!("INV-073 pending/expiry: 16 worlds, user_calls={user_calls}, exact_rollbacks={rollbacks}, positive_claim_expiries={positive_claim_expiries}, waiting_claimants={waiting_claimants}, retired={BACKING}/world, peak_CU={peak}");
}

pub(super) fn reserve_payout(
    env: &V16CuEnv,
    wallets: [Pubkey; 5],
    tokens: [Pubkey; 5],
    ledger: Pubkey,
    kind: usize,
    amount: u64,
) -> Instruction {
    let actor = if kind == 2 { 4 } else { 2 };
    let mut accounts = vec![
        AccountMeta::new_readonly(wallets[actor], false),
        AccountMeta::new(env.market, false),
    ];
    if kind == 1 {
        accounts.push(AccountMeta::new(ledger, false));
    }
    accounts.extend([
        AccountMeta::new(tokens[actor], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    let market_id = env.asset_market_id(0);
    let authority_epoch = env.control_sequences(0).authority_epoch;
    let amount = amount.into();
    let ix = match kind {
        0 => ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            authority_epoch,
            amount,
        },
        1 => ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            authority_epoch,
            amount,
        },
        2 => ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id,
            authority_epoch,
            amount,
        },
        _ => unreachable!(),
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

pub(crate) fn verify_terminal_public_reserve_seniority() {
    let TerminalEarningsWorld {
        mut env,
        incumbent,
        successor,
        wallets,
        tokens,
        portfolios,
        mint_frame,
        ..
    } = terminal_earnings_world_with_exit(false);
    drop((incumbent, successor));
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger.pubkey();
    let reserves = std::array::from_fn::<_, 3, _>(|kind| {
        reserve_payout(&env, wallets, tokens, ledger, kind, 1)
    });
    let tracked = [env.market, env.vault, env.mint, ledger]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .collect::<Vec<_>>();
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::ExpectedSigner)),
        );
    }
    env.resolve();
    env.svm.warp_to_slot(7);
    assert!(env.market_state().1.c_tot > 0);
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        );
    }
    for _ in 0..8 {
        for actor in [1, 0] {
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
            let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
            land(&mut env, &[ix], &[], &tracked, &allowed, 0, None, None);
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
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
    assert_eq!(
        (
            env.market_state().1.c_tot,
            env.market_state().1.materialized_portfolio_count
        ),
        (0, 2)
    );
    // Mechanical deletion is still a separate prerequisite of the existing reserve routes.
    for ix in &reserves {
        land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        );
    }
    assert_eq!(env.token_amount(env.vault), BACKING + EARNINGS + INSURANCE);
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
}

pub(crate) fn verify_terminal_public_reserve_disposition() {
    const PREFIX: [u64; 3] = [101, 17, 7];
    const STOCK: [u64; 3] = [BACKING, EARNINGS, INSURANCE];
    let mut peak = 0;
    for expire in [false, true] {
        for order in [
            [1, 0, 2],
            [0, 1, 2],
            [0, 2, 1],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
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
            let encumbered = [&incumbent, &beneficiary].map(|owner| {
                [false, true].map(|close_authority| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        TokenAccount::LEN,
                        spl_token::ID,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::initialize_account3(
                            &spl_token::ID,
                            &key.pubkey(),
                            &env.mint,
                            &owner.pubkey(),
                        )
                        .unwrap(),
                        &[],
                    )
                    .unwrap();
                    let instruction = if close_authority {
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &key.pubkey(),
                            Some(&admin.pubkey()),
                            spl_token::instruction::AuthorityType::CloseAccount,
                            &owner.pubkey(),
                            &[],
                        )
                        .unwrap()
                    } else {
                        spl_token::instruction::approve(
                            &spl_token::ID,
                            &key.pubkey(),
                            &admin.pubkey(),
                            &owner.pubkey(),
                            &[],
                            SUPPLY,
                        )
                        .unwrap()
                    };
                    send_raw_tx(&mut env.svm, &env.payer, instruction, &[owner]).unwrap();
                    key.pubkey()
                })
            });
            drop((incumbent, successor, beneficiary));
            assert!(!wallets.contains(&env.payer.pubkey()));
            assert!(!wallets.contains(&admin.pubkey()));
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
                ledger,
                admin.pubkey(),
                admin_token,
            ]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .chain(encumbered.into_iter().flatten())
            .collect::<Vec<_>>();
            let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let empty_ledger = env.svm.get_account(&ledger).unwrap();
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let epoch = env.control_sequences(0).authority_epoch;
            let wrap = |ix: ProgInstruction, accounts| Instruction {
                program_id: percolator_prog::id(),
                accounts,
                data: ix.encode(),
            };
            let payout = |env: &V16CuEnv, kind, amount| {
                reserve_payout(env, wallets, tokens, ledger, kind, amount)
            };
            let close = wrap(
                ProgInstruction::CloseSlab {
                    authority_epoch: epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let close_at = |env: &V16CuEnv| {
                let mut ix = close.clone();
                ix.data = ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode();
                ix
            };
            let unsigned_close = |env: &V16CuEnv| {
                let mut ix = close_at(env);
                ix.accounts[0].is_signer = false;
                ix
            };
            let stock = |env: &V16CuEnv, paid: [u64; 3], expired: bool, insurance_debits: u64| {
                let amounts = [PAYOUTS[0], PAYOUTS[1], paid[0] + paid[1], 0, paid[2]];
                for ((key, frame), amount) in tokens.into_iter().zip(&token_frames).zip(amounts) {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                let group = env.market_state().1;
                let remaining = STOCK.iter().sum::<u64>() - paid.iter().sum::<u64>();
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
                assert_eq!(env.token_amount(env.vault), remaining);
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                let mut expected_vault = vault_frame.clone();
                let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut expected_vault.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
                assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
                assert_eq!(
                    group.insurance_domain_budget_remaining_total,
                    group.insurance
                );
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - paid[1])
                );
                let bucket = group.source_backing_buckets[1];
                assert_eq!(
                    bucket.utilization_fee_earnings,
                    u128::from(EARNINGS - paid[1])
                );
                let principal = if expired {
                    0
                } else {
                    u128::from(BACKING - paid[0]) * BOUND_SCALE
                };
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
                assert_eq!(
                    bucket.status,
                    if principal == 0 {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                assert_eq!(
                    env.control_sequences(0).authority_epoch,
                    epoch + insurance_debits
                );
                assert_eq!(env.token_amount(admin_token), 0);
                let mut image = env.svm.get_account(&env.market).unwrap();
                state::market_view_mut(&mut image.data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
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
            };
            let mut paid = [0; 3];
            let mut insurance_debits = 0;
            stock(&env, paid, false, insurance_debits);
            for kind in order {
                let prefix = payout(&env, kind, PREFIX[kind]);
                let rejected_close = unsigned_close(&env);
                peak = peak.max(land(
                    &mut env,
                    &[prefix.clone(), rejected_close],
                    &[],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                stock(&env, paid, false, insurance_debits);
                for destination in encumbered[usize::from(kind == 2)] {
                    let mut blocked = payout(&env, kind, PREFIX[kind]);
                    blocked.accounts[if kind == 1 { 3 } else { 2 }].pubkey = destination;
                    peak = peak.max(land(
                        &mut env,
                        &[blocked],
                        &[],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((2, PercolatorError::InvalidTokenAccount)),
                    ));
                    stock(&env, paid, false, insurance_debits);
                }
                let actor = if kind == 2 { 4 } else { 2 };
                let allowed = [env.market, env.vault, ledger, tokens[actor]];
                peak = peak.max(land(
                    &mut env,
                    &[prefix],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                paid[kind] += PREFIX[kind];
                insurance_debits += u64::from(kind == 2);
                stock(&env, paid, false, insurance_debits);
            }
            if expire {
                env.svm.warp_to_slot(100);
                let allowed = [env.market];
                let close_ix = close_at(&env);
                peak = peak.max(land(
                    &mut env,
                    &[close_ix],
                    &[&admin],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                stock(&env, paid, true, insurance_debits);
            }
            for kind in order.into_iter().rev() {
                if expire && kind == 0 {
                    continue;
                }
                let blocked_close = close_at(&env);
                peak = peak.max(land(
                    &mut env,
                    &[blocked_close],
                    &[&admin],
                    &tracked,
                    &[],
                    0,
                    None,
                    Some((2, PercolatorError::EngineLockActive)),
                ));
                let actor = if kind == 2 { 4 } else { 2 };
                let allowed = [env.market, env.vault, ledger, tokens[actor]];
                let tail = payout(&env, kind, STOCK[kind] - PREFIX[kind]);
                peak = peak.max(land(
                    &mut env,
                    &[tail],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                paid[kind] = STOCK[kind];
                insurance_debits += u64::from(kind == 2);
                stock(&env, paid, expire, insurance_debits);
            }
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund =
                env.svm.get_account(&env.market).unwrap().lamports + vault_frame.lamports - rent;
            let allowed = [env.market, env.vault, env.mint];
            let final_close = close_at(&env);
            peak = peak.max(land(
                &mut env,
                &[final_close],
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
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            let burned = if expire { BACKING - PREFIX[0] } else { 0 };
            let mut expected_mint = mint_frame.clone();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= burned;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>() + burned,
                SUPPLY
            );
            assert_eq!(env.token_amount(tokens[2]), paid[0] + EARNINGS);
            assert_eq!(env.token_amount(tokens[4]), INSURANCE);
            assert_eq!(env.token_amount(admin_token), 0);
        }
    }
    eprintln!("INV-073 terminal public reserves: 12 worlds; peak CU={peak}");
}
