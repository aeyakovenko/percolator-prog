//! INV-073/077/082: terminal continuation at both portfolio caps after a partial
//! short-side B chunk. Compare interrupted and completed settlement, in both
//! claimant orders, using public construction and keeper-only terminal calls.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

fn land(
    env: &mut V16CuEnv,
    instruction: Instruction,
    tracked: &[Pubkey],
    rollback_prefix: bool,
) -> u64 {
    env.svm.expire_blockhash();
    let frame: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
    let payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let writable: Vec<_> = instruction
        .accounts
        .iter()
        .filter(|meta| meta.is_writable)
        .map(|meta| meta.pubkey)
        .collect();
    let mut instructions = vec![heap_ix(), cu_ix(), instruction];
    if rollback_prefix {
        instructions.push(Instruction {
            program_id: env.program_id,
            accounts: vec![],
            data: ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
            .encode(),
        });
    }
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let fee = solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let cu = if rollback_prefix {
        let error = env
            .svm
            .send_transaction(tx)
            .expect_err("late suffix rolls back progress");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(3, InstructionError::NotEnoughAccountKeys)
        );
        assert_eq!(
            tracked
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            frame
        );
        error.meta.compute_units_consumed
    } else {
        let result = env
            .svm
            .send_transaction(tx)
            .expect("bounded public progress");
        for (key, account) in tracked.iter().zip(&frame) {
            if !writable.contains(key) {
                assert_eq!(
                    &env.svm.get_account(key),
                    account,
                    "untouched account {key}"
                );
            }
        }
        result.compute_units_consumed
    };
    let mut expected_payer = payer_before;
    expected_payer.lamports -= fee;
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        expected_payer
    );
    assert!(cu < 1_400_000, "short B terminal transaction used {cu} CU");
    cu
}

#[test]
fn v16_program_partial_short_b_at_capacity_has_bounded_terminal_progress() {
    const TOTAL: u64 = 2 * CAPITAL as u64 + 2 * ASSETS as u64;
    const RESOLVE_SLOT: u64 = SETTLE_SLOT + 100;
    const EXIT_SLOT: u64 = RESOLVE_SLOT + 5;
    let mut peak = [0u64; 3];
    let mut progress_calls = 0;
    let mut rollbacks = 0;
    for b_calls in [1, 2 * ASSETS] {
        for reverse in [false, true] {
            let ShortBWorld {
                mut env,
                owner,
                target,
                peer_owner,
                peer,
                checkpoint_owner,
                checkpoint,
                counterparties,
            } = public_short_b_world();
            let actors: Vec<_> = [
                (owner.pubkey(), target),
                (peer_owner.pubkey(), peer),
                (checkpoint_owner.pubkey(), checkpoint),
            ]
            .into_iter()
            .chain(
                counterparties
                    .iter()
                    .map(|(owner, key)| (owner.pubkey(), *key)),
            )
            .map(|(owner, portfolio)| (owner, portfolio, canonical_vault_ata(owner, env.mint)))
            .collect();
            drop((owner, peer_owner, checkpoint_owner, counterparties));
            let tracked: Vec<_> = [
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                env.admin.pubkey(),
                solana_sdk::sysvar::clock::id(),
            ]
            .into_iter()
            .chain(
                actors
                    .iter()
                    .flat_map(|&(owner, portfolio, token)| [owner, portfolio, token]),
            )
            .collect();
            let owner_frames: Vec<_> = actors
                .iter()
                .map(|&(owner, _, _)| env.svm.get_account(&owner))
                .collect();
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            assert_eq!(Mint::unpack(&mint_frame.data).unwrap().supply, TOTAL);
            let initial = env.portfolio_state(target);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&initial)),
                u32::from(ASSETS)
            );
            assert_eq!(
                initial
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied() && s.source_claim_bound_num.get() > 0)
                    .count(),
                SOURCES
            );
            assert_eq!(initial.pnl.get(), 9 * i128::from(ASSETS));
            let b_rank = |env: &V16CuEnv| -> u128 {
                let market = env.market_state().1;
                let account = env.portfolio_state(target);
                (0..ASSETS)
                    .map(|asset| {
                        if !has_active_leg_for_asset(&account, usize::from(asset)) {
                            return 0;
                        }
                        let leg = active_leg_for_asset(&account, usize::from(asset));
                        let num = (market.assets[usize::from(asset)].b_short_num - leg.b_snap)
                            .checked_mul(leg.loss_weight)
                            .unwrap()
                            .checked_add(leg.b_rem)
                            .unwrap();
                        assert_eq!(num % percolator::SOCIAL_LOSS_DEN, 0);
                        num / percolator::SOCIAL_LOSS_DEN
                    })
                    .sum()
            };
            let check_custody = |env: &V16CuEnv| {
                let group = env.market_state().1;
                let accounts: Vec<_> = actors
                    .iter()
                    .map(|&(_, portfolio, _)| env.portfolio_state(portfolio))
                    .collect();
                assert_market_stock_census(
                    "short B terminal",
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    u128::from(env.token_amount(env.vault)),
                )
                .unwrap();
                assert_reservation_encumbrance_census("short B terminal", &group, &accounts)
                    .unwrap();
                let paid: u64 = actors
                    .iter()
                    .map(|&(_, _, token)| env.token_amount(token))
                    .sum();
                assert_eq!(env.token_amount(env.vault) + paid, TOTAL);
                assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                assert_eq!(group.insurance, 0);
                assert!(env.token_amount(actors[0].2) <= CAPITAL as u64 + 3 * u64::from(ASSETS));
                assert!(env.token_amount(actors[1].2) <= CAPITAL as u64 - u64::from(ASSETS));
                assert!(actors[2..]
                    .iter()
                    .all(|&(_, _, token)| env.token_amount(token) == 0));
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                assert_eq!(
                    actors
                        .iter()
                        .map(|&(owner, _, _)| env.svm.get_account(&owner))
                        .collect::<Vec<_>>(),
                    owner_frames
                );
            };
            assert_eq!(b_rank(&env), LOSS_PER_LEG * u128::from(ASSETS));
            for _ in 0..b_calls {
                let rank = b_rank(&env);
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(env.payer.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(target, false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: SETTLE_SLOT,
                        observations: vec![],
                    }
                    .encode(),
                };
                peak[2] = peak[2].max(land(&mut env, ix.clone(), &tracked, true));
                rollbacks += 1;
                peak[0] = peak[0].max(land(&mut env, ix, &tracked, false));
                assert!(b_rank(&env) < rank);
                check_custody(&env);
            }
            let remaining = b_rank(&env);
            let checkpoint_state = env.portfolio_state(target);
            assert_eq!(remaining, if b_calls == 1 { 80 } else { 0 });
            assert_eq!(checkpoint_state.pnl.get(), 42 + remaining as i128);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&checkpoint_state)),
                u32::from(ASSETS)
            );
            assert_eq!(
                checkpoint_state
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied() && s.source_claim_bound_num.get() > 0)
                    .count(),
                SOURCES
            );
            assert_eq!(
                (0..ASSETS)
                    .filter(
                        |&asset| active_leg_for_asset(&checkpoint_state, usize::from(asset)).b_rem
                            > 0
                    )
                    .count(),
                usize::from(b_calls == 1)
            );

            env.svm.warp_to_slot(RESOLVE_SLOT);
            let resolve = Instruction {
                program_id: env.program_id,
                accounts: vec![AccountMeta::new(env.market, false)],
                data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
            };
            peak[2] = peak[2].max(land(&mut env, resolve.clone(), &tracked, true));
            rollbacks += 1;
            peak[1] = peak[1].max(land(&mut env, resolve, &tracked, false));
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(env.portfolio_state(target), checkpoint_state);
            check_custody(&env);
            env.svm.warp_to_slot(EXIT_SLOT);

            let rank = |env: &V16CuEnv| {
                let group = env.market_state().1;
                let lapsed = group
                    .source_backing_buckets
                    .iter()
                    .filter(|bucket| {
                        bucket.status == BackingBucketStatusV16::Fresh
                            && bucket.expiry_slot <= env.svm.get_sysvar::<Clock>().slot
                    })
                    .count();
                let mut legs = 0;
                let mut sources = 0;
                let mut unfinished = 0;
                let mut unpaid = TOTAL;
                for &(_, portfolio, token) in &actors {
                    let account = env.portfolio_state(portfolio);
                    legs += percolator::active_bitmap_count_ones(active_bitmap(&account));
                    sources += account
                        .source_domains
                        .iter()
                        .filter(|s| s.is_occupied())
                        .count();
                    unfinished += usize::from(!resolved_portfolio_is_terminal(env, portfolio));
                    unpaid -= env.token_amount(token);
                }
                (lapsed, b_rank(env), legs, sources, unfinished, unpaid)
            };
            let mut order: Vec<_> = (0..actors.len()).collect();
            if reverse {
                order.reverse();
            }
            let mut calls = 0;
            let initial_rank = rank(&env);
            let call_bound = initial_rank.0
                + initial_rank.1 as usize
                + initial_rank.2 as usize
                + initial_rank.3
                + initial_rank.4;
            for _ in 0..call_bound {
                if actors
                    .iter()
                    .all(|&(_, portfolio, _)| resolved_portfolio_is_terminal(&env, portfolio))
                {
                    break;
                }
                let before_sweep = rank(&env);
                for &actor in &order {
                    let (owner, portfolio, token) = actors[actor];
                    if resolved_portfolio_is_terminal(&env, portfolio) {
                        continue;
                    }
                    let before = rank(&env);
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(owner, false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(token, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                        .encode(),
                    };
                    peak[2] = peak[2].max(land(&mut env, ix.clone(), &tracked, true));
                    rollbacks += 1;
                    peak[1] = peak[1].max(land(&mut env, ix, &tracked, false));
                    assert!(
                        rank(&env) < before,
                        "actor {actor}: terminal work {before:?} -> {:?}",
                        rank(&env)
                    );
                    check_custody(&env);
                    calls += 1;
                    assert!(calls <= call_bound, "state-derived terminal work bound");
                }
                assert!(rank(&env) < before_sweep);
            }
            assert_eq!(rank(&env), (0, 0, 0, 0, 0, 0));
            let payouts: Vec<_> = actors
                .iter()
                .map(|&(_, _, token)| env.token_amount(token))
                .collect();
            assert_eq!(payouts[0], CAPITAL as u64 + 3 * u64::from(ASSETS));
            assert_eq!(payouts[1], CAPITAL as u64 - u64::from(ASSETS));
            assert!(payouts[2..].iter().all(|&paid| paid == 0));
            assert_eq!(env.market_state().1.c_tot, 0);
            assert_eq!(
                env.market_state().1.materialized_portfolio_count,
                actors.len() as u64
            );
            progress_calls += calls;
            eprintln!("short B terminal: B_calls={b_calls}, reverse={reverse}, remaining_B={remaining}, terminal_calls={calls}/{call_bound}, payouts={payouts:?}, peak_CU={peak:?}");
        }
    }
    eprintln!("short B terminal: worlds=4, terminal_calls={progress_calls}, exact_rollbacks={rollbacks}, peak_CU[B,terminal,rollback]={peak:?}");
}
