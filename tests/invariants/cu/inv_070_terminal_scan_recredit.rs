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

#[test]
fn v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry() {
    let lock = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
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
                    let initial_epoch = env.control_sequences(0).authority_epoch;
                    let mut close = wrap(
                        &env,
                        ProgInstruction::CloseSlab {
                            authority_epoch: initial_epoch,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(destination, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(env.mint, false),
                        ],
                    );
                    let close_after_debit = |debits| Instruction {
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: initial_epoch + debits,
                        }
                        .encode(),
                        ..close.clone()
                    };
                    let close_after_first = close_after_debit(1);
                    let close_after_tail = close_after_debit(2);
                    let withdraw = |amount: u64, debits| {
                        wrap(
                            &env,
                            ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                authority_epoch: initial_epoch + debits,
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
                    let first = withdraw(partial, 0);
                    let tail = withdraw(recovered - partial, 1);
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
                        assert_eq!(
                            env.control_sequences(0).authority_epoch,
                            initial_epoch + u64::from(paid > 0) + u64::from(paid == recovered),
                            "only committed insurance debits consume the authority epoch"
                        );
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
                        Some((
                            4,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                        (2, 1),
                    );
                    check(&env, false, 0, 0, 1);
                    // The debit consumes an epoch even inside a bundle. A current
                    // suffix must reach the unpaid-entitlement guard, not stale auth.
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
                        &[first.clone(), close_after_first.clone()],
                        &[&admin],
                        &[],
                        Some((3, lock.clone())),
                        (1, 1),
                    );
                    check(&env, true, if scanner_first { recovered } else { 0 }, 0, 0);
                    send(&mut env, &[first], &[], &payment, None, (1, 1));
                    close = close_after_first;
                    check(&env, true, recovered, partial, 0);
                    send(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                    send(&mut env, &[tail], &[], &payment, None, (1, 1));
                    close = close_after_tail;
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
                        &[close.clone(), bad_suffix],
                        &[&admin],
                        &[],
                        Some((3, InstructionError::InvalidInstructionData)),
                        (1, 1 + usize::from(backing != recovered)),
                    );
                    check(&env, true, recovered, recovered, 0);
                    let closing = [env.market, env.vault, env.mint, admin.pubkey()];
                    send(
                        &mut env,
                        &[close],
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
    assert_eq!((commits, rollbacks, rediscoveries), (88, 120, 8));
    println!("INV-070 scan recredit: 16 public histories, 8 order comparisons, {commits} commits, {rollbacks} exact rollbacks, {rediscoveries} scanner rediscoveries, peak={peak} CU");
}
