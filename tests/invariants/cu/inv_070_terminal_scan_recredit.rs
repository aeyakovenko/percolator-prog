//! INV-070: expiry must invalidate a prefix whose earlier insurance becomes actionable.
//!
//! The input-owned stock oracle and public fixture are shared with INV-071's local
//! withdrawal control. This probe additionally requires the scanner itself to find
//! the earlier entitlement, and compares scanner-first with withdrawal-first order.
//! Both source sides, exact/late expiry, and residual-limited/full recovery are sampled.
//! INV-024/025/041 own recipient/stock checks; INV-063/069/071 own expiry and progress;
//! INV-033/086/088 receive bounded observation evidence, not universal closure.

use super::*;
use inv_071_crank_progress::terminal_prefix_recredit::{
    fixture, land, stocks_at_cursor, wrap, RecreditFixture, CAPITAL, EXPIRY, PAYOUTS, SPENT,
};
use solana_sdk::{instruction::InstructionError, system_program};

#[path = "inv_070_terminal_scan_multiwave.rs"]
mod multiwave;

#[path = "inv_070_terminal_scan_native_surplus.rs"]
mod native_surplus;

#[test]
fn v16_program_rediscovered_insurance_recreates_prefunded_beneficiary_before_retirement() {
    const PREFUND: u64 = 19;
    let mut peak = 0;
    let mut commits = 0;
    let mut rollbacks = 0;
    for side in 0..2 {
        for backing in [61u64, 307] {
            let RecreditFixture {
                mut env,
                admin,
                beneficiary,
                owners,
                portfolios,
                tokens,
                reserve,
                destination,
                peak: fixture_peak,
            } = fixture(side, backing);
            peak = peak.max(fixture_peak);
            let recovered = CAPITAL[1].min(SPENT).min(backing);
            let keeper = Keypair::new();
            env.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
            let sequences = env.control_sequences(0);
            let ledger = env.market_state().1.resolved_payout_ledger;
            let reserve_before = env.svm.get_account(&reserve).unwrap();
            let beneficiary_before = env.svm.get_account(&beneficiary.pubkey()).unwrap();
            let close_at = |authority_epoch| {
                wrap(
                    &env,
                    ProgInstruction::CloseSlab { authority_epoch },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                )
            };
            let close = close_at(sequences.authority_epoch);
            let final_close = close_at(sequences.authority_epoch + 1);
            let payout = wrap(
                &env,
                ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: sequences.authority_epoch,
                    amount: recovered.into(),
                },
                vec![
                    AccountMeta::new_readonly(beneficiary.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(reserve, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let repair = Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(keeper.pubkey(), true),
                    AccountMeta::new(reserve, false),
                    AccountMeta::new_readonly(beneficiary.pubkey(), false),
                    AccountMeta::new_readonly(env.mint, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
                ],
                data: vec![],
            };
            let bad_suffix = Instruction {
                program_id: system_program::ID,
                accounts: vec![],
                data: vec![255],
            };
            let mut tracked = vec![
                env.market,
                env.vault,
                env.mint,
                reserve,
                destination,
                admin.pubkey(),
                beneficiary.pubkey(),
                keeper.pubkey(),
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(tokens);
            tracked.extend(portfolios);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let market_only = [env.market];
            let mut send = |env: &mut V16CuEnv,
                            ixs: &[Instruction],
                            signers: &[&Keypair],
                            changed: &[Pubkey],
                            rejection: Option<(u8, InstructionError)>,
                            successes| {
                if rejection.is_some() {
                    rollbacks += 1;
                } else {
                    commits += 1;
                }
                peak = peak.max(land(
                    env, ixs, signers, &tracked, changed, rejection, successes,
                ));
            };
            let check =
                |env: &V16CuEnv, normalized: bool, restored: u64, paid: u64, cursor: u128| {
                    let (cfg, group) = env.market_state();
                    let market = env.svm.get_account(&env.market).unwrap();
                    let fresh = if normalized {
                        0
                    } else {
                        u128::from(backing) * BOUND_SCALE
                    };
                    assert_eq!(cfg.terminal_slab_scan_progress, cursor);
                    assert_eq!(
                        (
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(
                        (group.vault, group.insurance),
                        ((backing - paid).into(), (restored - paid).into())
                    );
                    for domain in 0..4 {
                        let expected_fresh = if domain == 2 + side { fresh } else { 0 };
                        assert_eq!(
                            group.source_backing_buckets[domain].fresh_unliened_backing_num,
                            expected_fresh
                        );
                        assert_eq!(
                            group.source_credit[domain].fresh_reserved_backing_num,
                            expected_fresh
                        );
                        assert_eq!(
                            group.insurance_domain_budget[domain],
                            if domain == side {
                                (SPENT - paid).into()
                            } else {
                                0
                            }
                        );
                        assert_eq!(
                            group.insurance_domain_spent[domain],
                            if domain == side {
                                (SPENT - restored).into()
                            } else {
                                0
                            }
                        );
                    }
                    let mut expected_ledger = ledger;
                    if normalized {
                        expected_ledger.snapshot_residual += u128::from(backing);
                    }
                    assert_eq!(group.resolved_payout_ledger, expected_ledger);
                    let mut expected_sequences = sequences;
                    expected_sequences.authority_epoch += u64::from(paid != 0);
                    assert_eq!(env.control_sequences(0), expected_sequences);
                    assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
                    assert_eq!(env.token_amount(destination), 0);
                    assert_eq!(env.token_amount(env.vault), backing - paid);
                    let supply = CAPITAL.iter().sum::<u64>() + SPENT + backing;
                    assert_eq!(
                        PAYOUTS.iter().sum::<u64>() + paid + env.token_amount(env.vault),
                        supply
                    );
                    assert_eq!(
                        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                            .unwrap()
                            .supply,
                        supply
                    );
                    crate::support::fuzz_model::assert_market_stock_census(
                        "rediscovered insurance with unavailable custody",
                        &group,
                        &market.data,
                        &[],
                        group.vault,
                    )
                    .unwrap();
                    crate::support::fuzz_model::assert_reservation_encumbrance_census(
                        "rediscovered insurance with unavailable custody",
                        &group,
                        &[],
                    )
                    .unwrap();
                };
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
            );
            stocks_at_cursor(&env, side, backing, false, 0, 0, tokens, reserve, 1);

            // External wallet retirement and prefunding leave the saved market prefix exact.
            let retire_wallet = spl_token::instruction::close_account(
                &spl_token::ID,
                &reserve,
                &beneficiary.pubkey(),
                &beneficiary.pubkey(),
                &[],
            )
            .unwrap();
            send(
                &mut env,
                &[retire_wallet],
                &[&beneficiary],
                &[reserve, beneficiary.pubkey()],
                None,
                (0, 1),
            );
            let mut expected_beneficiary = beneficiary_before;
            expected_beneficiary.lamports += reserve_before.lamports;
            assert_eq!(
                env.svm.get_account(&beneficiary.pubkey()),
                Some(expected_beneficiary)
            );
            assert!(env
                .svm
                .get_account(&reserve)
                .is_none_or(|account| account.lamports == 0));
            drop(beneficiary);
            let prefund =
                solana_sdk::system_instruction::transfer(&keeper.pubkey(), &reserve, PREFUND);
            let keeper_before = env.svm.get_account(&keeper.pubkey()).unwrap();
            send(
                &mut env,
                &[prefund],
                &[&keeper],
                &[reserve, keeper.pubkey()],
                None,
                (0, 0),
            );
            let prefunded = env.svm.get_account(&reserve).unwrap();
            assert_eq!(
                (prefunded.owner, prefunded.lamports),
                (system_program::ID, PREFUND)
            );
            assert!(prefunded.data.is_empty());
            let mut expected_keeper = keeper_before;
            expected_keeper.lamports -= PREFUND;
            assert_eq!(
                env.svm.get_account(&keeper.pubkey()),
                Some(expected_keeper.clone())
            );
            check(&env, false, 0, 0, 1);
            let before_clock = env.svm.get_account(&env.market).unwrap();
            env.svm.warp_to_slot(EXPIRY);
            assert_eq!(env.svm.get_account(&env.market), Some(before_clock.clone()));
            let missing = InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32);
            send(
                &mut env,
                &[close.clone(), close.clone(), payout.clone()],
                &[&admin],
                &[],
                Some((4, missing.clone())),
                (2, 0),
            );
            check(&env, false, 0, 0, 1);
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
            );
            check(&env, true, 0, 0, 0);
            assert_eq!(
                market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 0),
                market_engine_slot_bytes(&before_clock.data, 0)
            );
            send(
                &mut env,
                &[close.clone()],
                &[&admin],
                &market_only,
                None,
                (1, 0),
            );
            check(&env, true, recovered, 0, 0);
            send(
                &mut env,
                &[close],
                &[&admin],
                &[],
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )),
                (0, 0),
            );
            send(
                &mut env,
                &[payout.clone()],
                &[],
                &[],
                Some((2, missing)),
                (0, 0),
            );

            // ATA allocation, keeper rent, SPL payment and the debit epoch must roll back together.
            send(
                &mut env,
                &[repair.clone(), payout.clone(), bad_suffix.clone()],
                &[&keeper],
                &[],
                Some((4, InstructionError::InvalidInstructionData)),
                (1, 4),
            );
            check(&env, true, recovered, 0, 0);
            assert_eq!(env.svm.get_account(&reserve), Some(prefunded));
            let payment = [env.market, env.vault, reserve, keeper.pubkey()];
            send(
                &mut env,
                &[repair, payout],
                &[&keeper],
                &payment,
                None,
                (1, 4),
            );
            check(&env, true, recovered, recovered, 0);
            stocks_at_cursor(
                &env, side, backing, true, recovered, recovered, tokens, reserve, 0,
            );
            let mut expected_reserve = reserve_before;
            let mut token = TokenAccount::unpack(&expected_reserve.data).unwrap();
            token.amount = recovered;
            TokenAccount::pack(token, &mut expected_reserve.data).unwrap();
            assert_eq!(
                env.svm.get_account(&reserve),
                Some(expected_reserve.clone())
            );
            expected_keeper.lamports -= expected_reserve.lamports - PREFUND;
            assert_eq!(env.svm.get_account(&keeper.pubkey()), Some(expected_keeper));

            let market = env.svm.get_account(&env.market).unwrap();
            let vault = env.svm.get_account(&env.vault).unwrap();
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= backing - recovered;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            let close_successes = (1, 1 + usize::from(backing != recovered));
            send(
                &mut env,
                &[final_close.clone(), bad_suffix],
                &[&admin],
                &[],
                Some((3, InstructionError::InvalidInstructionData)),
                close_successes,
            );
            let closing = [env.market, env.vault, env.mint, admin.pubkey()];
            send(
                &mut env,
                &[final_close],
                &[&admin],
                &closing,
                None,
                close_successes,
            );
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            expected_admin.lamports += market.lamports - tombstone.lamports + vault.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0));
            assert_eq!(env.token_amount(reserve), recovered);
            assert_eq!(env.token_amount(destination), 0);
        }
    }
    assert_eq!((commits, rollbacks), (28, 20));
    println!("INV-070 recreated insurance: 4 public histories, {commits} commits, {rollbacks} exact rollbacks, 4 scanner rediscoveries, peak={peak} CU");
}

#[test]
fn v16_program_rediscovered_insurance_retries_one_lamport_short_custody_repair() {
    const BACKING: u64 = 61;
    let RecreditFixture {
        mut env,
        admin,
        beneficiary,
        owners,
        portfolios,
        tokens,
        reserve,
        destination,
        mut peak,
    } = fixture(0, BACKING);
    let keeper = Keypair::new();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    env.svm.airdrop(&keeper.pubkey(), rent - 1).unwrap();
    let empty_reserve = env.svm.get_account(&reserve).unwrap();
    assert_eq!(empty_reserve.lamports, rent);
    let initial_sequences = env.control_sequences(0);
    let initial_ledger = env.market_state().1.resolved_payout_ledger;
    let close_at = |authority_epoch| {
        wrap(
            &env,
            ProgInstruction::CloseSlab { authority_epoch },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
        )
    };
    let close = close_at(initial_sequences.authority_epoch);
    let final_close = close_at(initial_sequences.authority_epoch + 1);
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(keeper.pubkey(), true),
            AccountMeta::new(reserve, false),
            AccountMeta::new_readonly(beneficiary.pubkey(), false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
        ],
        data: vec![],
    };
    let payout = wrap(
        &env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: initial_sequences.authority_epoch,
            amount: BACKING.into(),
        },
        vec![
            AccountMeta::new_readonly(beneficiary.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(reserve, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    );
    let mut tracked = vec![
        env.market,
        env.vault,
        env.mint,
        reserve,
        destination,
        admin.pubkey(),
        beneficiary.pubkey(),
        keeper.pubkey(),
        solana_sdk::sysvar::clock::ID,
    ];
    tracked.extend(tokens);
    tracked.extend(portfolios);
    tracked.extend(owners.each_ref().map(Signer::pubkey));
    let market_only = [env.market];
    let payment = [env.market, env.vault, reserve, keeper.pubkey()];
    let closing = [env.market, env.vault, admin.pubkey()];
    let mut commits = 0;
    let mut rollbacks = 0;
    let mut send = |env: &mut V16CuEnv,
                    ixs: &[Instruction],
                    signers: &[&Keypair],
                    changed: &[Pubkey],
                    rejection: Option<(u8, InstructionError)>,
                    successes| {
        commits += usize::from(rejection.is_none());
        rollbacks += usize::from(rejection.is_some());
        peak = peak.max(land(
            env, ixs, signers, &tracked, changed, rejection, successes,
        ));
    };
    send(
        &mut env,
        &[close.clone()],
        &[&admin],
        &market_only,
        None,
        (1, 0),
    );
    stocks_at_cursor(&env, 0, BACKING, false, 0, 0, tokens, reserve, 1);
    let parked = env.svm.get_account(&env.market).unwrap();
    let retire = spl_token::instruction::close_account(
        &spl_token::ID,
        &reserve,
        &beneficiary.pubkey(),
        &beneficiary.pubkey(),
        &[],
    )
    .unwrap();
    send(
        &mut env,
        &[retire],
        &[&beneficiary],
        &[reserve, beneficiary.pubkey()],
        None,
        (0, 1),
    );
    drop(beneficiary);
    env.svm.warp_to_slot(EXPIRY);
    assert_eq!(env.svm.get_account(&env.market), Some(parked.clone()));

    // Rent is paid by the keeper; the independent transaction payer covers signatures.
    let short = InstructionError::Custom(
        solana_sdk::system_instruction::SystemError::ResultWithNegativeLamports as u32,
    );
    send(
        &mut env,
        &[close.clone(), close.clone(), repair.clone(), payout.clone()],
        &[&admin, &keeper],
        &[],
        Some((4, short.clone())),
        (2, 1),
    );
    assert_eq!(env.svm.get_account(&env.market), Some(parked.clone()));
    assert_eq!(
        env.svm.get_account(&keeper.pubkey()).unwrap().lamports,
        rent - 1
    );
    send(
        &mut env,
        &[close.clone()],
        &[&admin],
        &market_only,
        None,
        (1, 0),
    );
    assert_eq!(env.market_state().0.terminal_slab_scan_progress, 0);
    assert_eq!(env.market_state().1.insurance, 0);
    assert_eq!(
        market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 0),
        market_engine_slot_bytes(&parked.data, 0),
    );
    send(
        &mut env,
        &[close.clone()],
        &[&admin],
        &market_only,
        None,
        (1, 0),
    );
    let (cfg, group) = env.market_state();
    assert_eq!(cfg.terminal_slab_scan_progress, 0);
    assert_eq!(
        (group.vault, group.insurance),
        (BACKING.into(), BACKING.into())
    );
    assert_eq!(group.insurance_domain_spent[0], u128::from(SPENT - BACKING));
    let mut expected_ledger = initial_ledger;
    expected_ledger.snapshot_residual += u128::from(BACKING);
    assert_eq!(group.resolved_payout_ledger, expected_ledger);
    assert_eq!(env.control_sequences(0), initial_sequences);
    let retry = [repair, payout];
    send(&mut env, &retry, &[&keeper], &[], Some((2, short)), (0, 1));
    send(
        &mut env,
        &[close],
        &[&admin],
        &[],
        Some((
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        )),
        (0, 0),
    );

    let fund = solana_sdk::system_instruction::transfer(&admin.pubkey(), &keeper.pubkey(), 1);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports -= 1;
    send(
        &mut env,
        &[fund],
        &[&admin],
        &[admin.pubkey(), keeper.pubkey()],
        None,
        (0, 0),
    );
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert_eq!(
        env.svm.get_account(&keeper.pubkey()).unwrap().lamports,
        rent
    );
    send(&mut env, &retry, &[&keeper], &payment, None, (1, 4));
    stocks_at_cursor(&env, 0, BACKING, true, BACKING, BACKING, tokens, reserve, 0);
    assert_eq!(env.market_state().1.resolved_payout_ledger, expected_ledger);
    let mut expected_sequences = initial_sequences;
    expected_sequences.authority_epoch += 1;
    assert_eq!(env.control_sequences(0), expected_sequences);
    let mut expected_reserve = empty_reserve;
    let mut token = TokenAccount::unpack(&expected_reserve.data).unwrap();
    token.amount = BACKING;
    TokenAccount::pack(token, &mut expected_reserve.data).unwrap();
    assert_eq!(env.svm.get_account(&reserve), Some(expected_reserve));
    assert!(env
        .svm
        .get_account(&keeper.pubkey())
        .is_none_or(|account| account.lamports == 0));

    let market = env.svm.get_account(&env.market).unwrap();
    let vault = env.svm.get_account(&env.vault).unwrap();
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    send(&mut env, &[final_close], &[&admin], &closing, None, (1, 1));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(
        tombstone.lamports,
        env.svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
    );
    expected_admin.lamports += market.lamports - tombstone.lamports + vault.lamports;
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|account| account.lamports == 0));
    assert_eq!(env.token_amount(reserve), BACKING);
    assert_eq!(env.token_amount(destination), 0);
    assert_eq!((commits, rollbacks), (7, 3));
    println!("INV-070 rent-short scanner repair: 1 history, {commits} commits, {rollbacks} exact rollbacks, peak={peak} CU");
}

#[test]
fn v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry() {
    let lock = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
    let stale = InstructionError::Custom(PercolatorError::EngineStale as u32);
    let mut peak = 0;
    let mut commits = 0;
    let mut rollbacks = 0;
    let mut rediscoveries = 0;
    let mut outcomes = Vec::new();
    for side in 0..2 {
        for backing in [61u64, 307] {
            for late in [false, true] {
                let mut ordered_outcomes = Vec::new();
                for scanner_first in [false, true] {
                    let RecreditFixture {
                        mut env,
                        admin,
                        beneficiary,
                        owners,
                        portfolios,
                        tokens,
                        reserve,
                        destination,
                        peak: fixture_peak,
                    } = fixture(side, backing);
                    peak = peak.max(fixture_peak);
                    let recovered = CAPITAL[1].min(SPENT).min(backing);
                    let partial = recovered / 3;
                    let initial_sequences = env.control_sequences(0);
                    let close_at = |authority_epoch| {
                        wrap(
                            &env,
                            ProgInstruction::CloseSlab { authority_epoch },
                            vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new(destination, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                                AccountMeta::new(env.mint, false),
                            ],
                        )
                    };
                    let close = close_at(initial_sequences.authority_epoch);
                    let close_after_first = close_at(initial_sequences.authority_epoch + 1);
                    let close_after_tail = close_at(initial_sequences.authority_epoch + 2);
                    let withdraw = |amount: u64, authority_epoch| {
                        wrap(
                            &env,
                            ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                authority_epoch,
                                amount: amount.into(),
                            },
                            vec![
                                AccountMeta::new_readonly(beneficiary.pubkey(), false),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(reserve, false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                        )
                    };
                    let first = withdraw(partial, initial_sequences.authority_epoch);
                    let tail = withdraw(recovered - partial, initial_sequences.authority_epoch + 1);
                    let mut tracked = vec![
                        env.market,
                        env.vault,
                        env.mint,
                        reserve,
                        destination,
                        admin.pubkey(),
                        beneficiary.pubkey(),
                        solana_sdk::sysvar::clock::ID,
                    ];
                    tracked.extend(tokens);
                    tracked.extend(portfolios);
                    tracked.extend(owners.each_ref().map(Signer::pubkey));
                    drop(beneficiary);
                    let market_only = [env.market];
                    let payment = [env.market, env.vault, reserve];
                    let initial_ledger = env.market_state().1.resolved_payout_ledger;
                    let mut send = |env: &mut V16CuEnv,
                                    ixs: &[Instruction],
                                    signers: &[&Keypair],
                                    changed: &[Pubkey],
                                    rejection: Option<(u8, InstructionError)>,
                                    successes| {
                        if rejection.is_some() {
                            rollbacks += 1;
                        } else {
                            commits += 1;
                        }
                        let cu = land(env, ixs, signers, &tracked, changed, rejection, successes);
                        peak = peak.max(cu);
                    };
                    let check = |env: &V16CuEnv, normalized, restored, paid, cursor| {
                        stocks_at_cursor(
                            env, side, backing, normalized, restored, paid, tokens, reserve, cursor,
                        );
                        let mut ledger = initial_ledger;
                        if normalized {
                            ledger.snapshot_residual += u128::from(backing);
                        }
                        assert_eq!(env.market_state().1.resolved_payout_ledger, ledger);
                        let mut sequences = initial_sequences;
                        sequences.authority_epoch += if paid == 0 {
                            0
                        } else if paid == partial {
                            1
                        } else {
                            assert_eq!(paid, recovered);
                            2
                        };
                        assert_eq!(env.control_sequences(0), sequences);
                        assert_eq!(
                            env.token_amount(destination),
                            0,
                            "market authority has no insurance entitlement"
                        );
                    };

                    send(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    check(&env, false, 0, 0, 1);
                    send(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                    send(
                        &mut env,
                        &[first.clone()],
                        &[],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                    let before_clock = env.svm.get_account(&env.market).unwrap();
                    env.svm.warp_to_slot(EXPIRY + u64::from(late));
                    assert_eq!(env.svm.get_account(&env.market), Some(before_clock.clone()));
                    check(&env, false, 0, 0, 1);

                    send(
                        &mut env,
                        &[close.clone(), first.clone(), close.clone()],
                        &[&admin],
                        &[],
                        Some((4, stale.clone())),
                        (2, 1),
                    );
                    check(&env, false, 0, 0, 1);
                    // A current-epoch suffix reaches the economic lock after the same paid prefix.
                    send(
                        &mut env,
                        &[close.clone(), first.clone(), close_after_first.clone()],
                        &[&admin],
                        &[],
                        Some((4, lock.clone())),
                        (2, 1),
                    );
                    check(&env, false, 0, 0, 1);
                    send(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    // The later source released residual. The earlier stored asset did not change,
                    // but its recoverable claim is now min(receivable, spent, released residual).
                    check(&env, true, 0, 0, 0);
                    let slot_start = MARKET_GROUP_OFF
                        + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
                    let slot_end = slot_start
                        + std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
                    assert_eq!(
                        &env.svm.get_account(&env.market).unwrap().data[slot_start..slot_end],
                        &before_clock.data[slot_start..slot_end]
                    );

                    if scanner_first {
                        send(
                            &mut env,
                            &[close.clone(), close.clone()],
                            &[&admin],
                            &[],
                            Some((3, lock.clone())),
                            (1, 0),
                        );
                        check(&env, true, 0, 0, 0);
                        send(
                            &mut env,
                            &[close.clone()],
                            &[&admin],
                            &market_only,
                            None,
                            (1, 0),
                        );
                        check(&env, true, recovered, 0, 0);
                        rediscoveries += 1;
                    }
                    send(
                        &mut env,
                        &[first.clone(), close.clone()],
                        &[&admin],
                        &[],
                        Some((3, stale.clone())),
                        (1, 1),
                    );
                    check(&env, true, if scanner_first { recovered } else { 0 }, 0, 0);
                    send(
                        &mut env,
                        &[first.clone(), close_after_first.clone()],
                        &[&admin],
                        &[],
                        Some((3, lock.clone())),
                        (1, 1),
                    );
                    check(&env, true, if scanner_first { recovered } else { 0 }, 0, 0);
                    send(&mut env, &[first.clone()], &[], &payment, None, (1, 1));
                    check(&env, true, recovered, partial, 0);
                    send(
                        &mut env,
                        &[first],
                        &[],
                        &[],
                        Some((2, stale.clone())),
                        (0, 0),
                    );
                    send(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &[],
                        Some((2, stale.clone())),
                        (0, 0),
                    );
                    check(&env, true, recovered, partial, 0);
                    send(
                        &mut env,
                        &[close_after_first],
                        &[&admin],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                    send(&mut env, &[tail], &[], &payment, None, (1, 1));
                    check(&env, true, recovered, recovered, 0);

                    let market = env.svm.get_account(&env.market).unwrap();
                    let vault = env.svm.get_account(&env.vault).unwrap();
                    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                    let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
                    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                    mint.supply -= backing - recovered;
                    Mint::pack(mint, &mut expected_mint.data).unwrap();
                    let bad_suffix = Instruction {
                        program_id: system_program::ID,
                        accounts: Vec::new(),
                        data: vec![255],
                    };
                    send(
                        &mut env,
                        &[close_after_tail.clone(), bad_suffix],
                        &[&admin],
                        &[],
                        Some((3, InstructionError::InvalidInstructionData)),
                        (1, 1 + usize::from(backing != recovered)),
                    );
                    check(&env, true, recovered, recovered, 0);
                    let closing = [env.market, env.vault, env.mint, admin.pubkey()];
                    send(
                        &mut env,
                        &[close_after_tail],
                        &[&admin],
                        &closing,
                        None,
                        (1, 1 + usize::from(backing != recovered)),
                    );
                    let tombstone = env.svm.get_account(&env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(
                        tombstone.lamports,
                        env.svm.minimum_balance_for_rent_exemption(
                            percolator_prog::constants::HEADER_LEN
                        )
                    );
                    expected_admin.lamports +=
                        market.lamports - tombstone.lamports + vault.lamports;
                    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                    assert!(env
                        .svm
                        .get_account(&env.vault)
                        .map_or(true, |a| a.lamports == 0));
                    assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
                    let outcome = (
                        tokens.map(|key| env.token_amount(key)),
                        env.token_amount(reserve),
                        env.token_amount(destination),
                        mint.supply,
                        backing - recovered,
                    );
                    assert_eq!(outcome.1, recovered);
                    assert_eq!(outcome.2, 0);
                    ordered_outcomes.push(outcome);
                    println!("INV-070 scan recredit: side={side} backing={backing} late={late} scanner_first={scanner_first} recovered={recovered}");
                }
                assert_eq!(
                    ordered_outcomes[0], ordered_outcomes[1],
                    "route order preserves entitlement and stock disposition"
                );
                outcomes.push(ordered_outcomes[0]);
            }
        }
    }
    assert_eq!(outcomes.len(), 8);
    assert_eq!((commits, rollbacks, rediscoveries), (88, 168, 8));
    println!("INV-070 scan recredit: 16 public histories, 8 order comparisons, {commits} commits, {rollbacks} exact rollbacks, {rediscoveries} scanner rediscoveries, peak={peak} CU");
}
