//! INV-073 / row 420: consumed then replenished principal remains publicly payable.
//! The 100-atom consumed-lien/receivable history is neither another withdrawal
//! allowance nor a signature dependency. Both user orders reach unsigned provider
//! payment and signed mechanical retirement before expiry. No production mutation.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PROFIT: u64 = 20 * (105 - 100);
const PAYOUTS: [u64; 2] = [CAPITAL + PROFIT, CAPITAL - PROFIT];
const LIMIT: u64 = 500_000;

#[test]
fn v16_program_absent_provider_replenished_principal_reaches_terminal_close() {
    run_terminal_provider_and_insurance(ProviderExit::Replenished);
}

pub(super) fn verify(
    env: &mut V16CuEnv,
    owners: &[Keypair; 2],
    absent: [Pubkey; 3],
    tokens: [Pubkey; 5],
    winner_first: bool,
) {
    let admin = env.admin.insecure_clone();
    let portfolios = [env.portfolios[0], env.portfolios[1]];
    let wallets = [
        owners[0].pubkey(),
        owners[1].pubkey(),
        absent[0],
        absent[1],
        admin.pubkey(),
    ];
    assert!(!absent.contains(&env.payer.pubkey()));
    assert!(!absent.contains(&admin.pubkey()));
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let mint = Mint::unpack(&mint_frame.data).unwrap();
    assert_eq!((mint.supply, mint.mint_authority), (FUNDED, COption::None));
    let custody_frames = tokens
        .into_iter()
        .chain([env.vault])
        .map(|key| (key, env.svm.get_account(&key).unwrap()))
        .collect::<Vec<_>>();
    let tracked = [env.market, env.vault, env.mint, env.vault_authority]
        .into_iter()
        .chain(portfolios)
        .chain(wallets)
        .chain(tokens)
        .chain(absent)
        .collect::<Vec<_>>();
    let land = |env: &mut V16CuEnv,
                ixs: &[Instruction],
                signers: &[&Keypair],
                allowed: &[Pubkey],
                refund: Option<(Pubkey, u64)>,
                rejection: Option<(u8, PercolatorError)>| {
        env.svm.expire_blockhash();
        let instructions = [
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ]
        .into_iter()
        .chain(ixs.iter().cloned())
        .collect::<Vec<_>>();
        let all_signers = [&env.payer]
            .into_iter()
            .chain(signers.iter().copied())
            .collect::<Vec<_>>();
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &all_signers,
            env.svm.latest_blockhash(),
        );
        let required = usize::from(tx.message.header.num_required_signatures);
        assert_eq!(required, 1 + signers.len());
        assert!(tx.message.account_keys[..required]
            .iter()
            .all(|key| !absent.contains(key)));
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let fee = required as u64 * FeeStructure::default().lamports_per_signature;
        let mut keys = tx.message.account_keys.clone();
        keys.extend_from_slice(&tracked);
        keys.sort_unstable();
        keys.dedup();
        let before = keys
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        let rejected = rejection.is_some();
        let result = env.svm.send_transaction(tx);
        let meta = if let Some((index, error)) = rejection {
            let failure =
                result.expect_err("overclaim must reject after the real provider payment");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32),)
            );
            for program in [env.program_id, spl_token::ID] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    1
                );
            }
            failure.meta
        } else {
            result.expect("bounded public provider continuation")
        };
        for (key, mut expected) in keys.into_iter().zip(before) {
            if key == env.payer.pubkey() {
                expected.as_mut().unwrap().lamports -= fee;
            }
            if !rejected {
                if let Some((recipient, lamports)) = refund {
                    if key == recipient {
                        expected.as_mut().unwrap().lamports += lamports;
                    }
                }
            }
            if rejected || !allowed.contains(&key) {
                assert_eq!(
                    env.svm.get_account(&key),
                    expected,
                    "complete Account frame {key}"
                );
            }
        }
        assert_cu_within(
            "row420 replenished provider",
            meta.compute_units_consumed,
            LIMIT,
        );
        meta.compute_units_consumed
    };
    let check_stock = |env: &V16CuEnv, active: &[Pubkey]| {
        let group = env.market_state().1;
        let image = env.svm.get_account(&env.market).unwrap();
        let states = active
            .iter()
            .map(|key| env.portfolio_state(*key))
            .collect::<Vec<_>>();
        assert_market_stock_census(
            "replenished provider",
            &group,
            &image.data,
            &states,
            env.token_amount(env.vault).into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("replenished provider", &group, &states).unwrap();
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                + env.token_amount(env.vault),
            FUNDED
        );
        assert_eq!(group.backing_provider_earnings_total, 0);
        for (key, original) in &custody_frames {
            let mut expected = original.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = env.token_amount(*key);
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(key), Some(expected));
        }
    };
    let wrap = |env: &V16CuEnv, ix: ProgInstruction, accounts| Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    };
    let order = if winner_first { [0, 1] } else { [1, 0] };
    let rank = |env: &V16CuEnv| {
        let legs: u32 = portfolios
            .iter()
            .map(|key| {
                percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(*key)))
            })
            .sum();
        (
            legs,
            2 * CAPITAL - env.token_amount(tokens[0]) - env.token_amount(tokens[1]),
        )
    };
    let mut peak = 0;
    let mut calls = 0;
    check_stock(env, &portfolios);
    env.svm.warp_to_slot(7);
    for _ in 0..8 {
        for actor in order {
            if resolved_portfolio_is_terminal(env, portfolios[actor]) {
                continue;
            }
            let ix = wrap(
                env,
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(wallets[actor], false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let before = rank(env);
            let allowed = [env.market, portfolios[actor], tokens[actor], env.vault];
            peak = peak.max(land(env, &[ix], &[], &allowed, None, None));
            assert!(
                rank(env) < before,
                "each user continuation decreases (legs, unpaid)"
            );
            calls += 1;
            check_stock(env, &portfolios);
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(env, *key))
        {
            break;
        }
    }
    assert_eq!(rank(env), (0, 0));
    assert_eq!(
        tokens.map(|key| env.token_amount(key)),
        [PAYOUTS[0], PAYOUTS[1], 0, 0, 0]
    );
    let settled = env.market_state().1;
    assert_eq!(settled.source_claim_bound_total_num, 0);
    let bucket = settled.source_backing_buckets[3];
    let source = settled.source_credit[3];
    // Input-derived trading gain, not a sum copied from the withdrawal implementation.
    assert_eq!(
        bucket.consumed_liened_backing_num,
        u128::from(PROFIT) * BOUND_SCALE
    );
    assert_eq!(
        source.provider_receivable_num,
        u128::from(PROFIT) * BOUND_SCALE
    );
    assert_eq!(
        bucket.fresh_unliened_backing_num,
        u128::from(BACKING) * BOUND_SCALE
    );
    assert_eq!(
        source.fresh_reserved_backing_num,
        u128::from(BACKING) * BOUND_SCALE
    );
    assert_eq!(
        (
            bucket.valid_liened_backing_num,
            bucket.impaired_liened_backing_num
        ),
        (0, 0)
    );
    assert_eq!(bucket.expiry_slot, 1_000);
    assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
    for (position, actor) in order.into_iter().enumerate() {
        let ix = wrap(
            env,
            env.close_portfolio_ix(portfolios[actor]),
            vec![
                AccountMeta::new(wallets[actor], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
            ],
        );
        let refund = env.svm.get_account(&portfolios[actor]).unwrap().lamports;
        let slab_lamports = env.svm.get_account(&env.market).unwrap().lamports;
        let allowed = [env.market, portfolios[actor]];
        peak = peak.max(land(env, &[ix], &[&owners[actor]], &allowed, None, None));
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            slab_lamports + refund
        );
        assert!(env
            .svm
            .get_account(&portfolios[actor])
            .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
        check_stock(
            env,
            &order[position + 1..]
                .iter()
                .map(|i| portfolios[*i])
                .collect::<Vec<_>>(),
        );
    }
    let payout = |env: &V16CuEnv, amount: u64| {
        wrap(
            env,
            ProgInstruction::WithdrawBackingBucket {
                domain: 3,
                market_id: env.asset_market_id(1),
                authority_epoch: env.control_sequences(1).authority_epoch,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new_readonly(absent[0], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    };
    let check_principal = |env: &V16CuEnv, paid: u64| {
        check_stock(env, &[]);
        let group = env.market_state().1;
        let mut expected_bucket = bucket;
        expected_bucket.fresh_unliened_backing_num = u128::from(BACKING - paid) * BOUND_SCALE;
        if paid == BACKING {
            expected_bucket.status = BackingBucketStatusV16::Expired;
        }
        let mut expected_source = source;
        expected_source.fresh_reserved_backing_num = u128::from(BACKING - paid) * BOUND_SCALE;
        expected_source.credit_epoch += if paid == 0 {
            0
        } else if paid == BACKING {
            2
        } else {
            1
        };
        assert_eq!(group.source_backing_buckets[3], expected_bucket);
        assert_eq!(group.source_credit[3], expected_source);
        assert_eq!(
            &group.source_backing_buckets[..3],
            &settled.source_backing_buckets[..3]
        );
        assert_eq!(&group.source_credit[..3], &settled.source_credit[..3]);
        assert_eq!(
            group.insurance_domain_budget,
            settled.insurance_domain_budget
        );
        assert_eq!(group.insurance, u128::from(INSURANCE + UNRELATED_INSURANCE));
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [PAYOUTS[0], PAYOUTS[1], paid, 0, 0]
        );
        assert_eq!(
            group.vault,
            u128::from(BACKING + INSURANCE + UNRELATED_INSURANCE - paid)
        );
    };
    check_principal(env, 0);
    // The consumed marker cannot authorize a second payment. Unrelated insurance
    // keeps custody liquid enough for this to reach the provider's stock bound.
    let first = payout(env, BACKING - 1);
    let overclaim = payout(env, PROFIT + 1);
    peak = peak.max(land(
        env,
        &[first.clone(), overclaim],
        &[],
        &[],
        None,
        Some((3, PercolatorError::EngineLockActive)),
    ));
    check_principal(env, 0);
    let allowed = [env.market, env.vault, tokens[2]];
    peak = peak.max(land(env, &[first], &[], &allowed, None, None));
    check_principal(env, BACKING - 1);
    let last = payout(env, 1);
    peak = peak.max(land(env, &[last], &[], &allowed, None, None));
    check_principal(env, BACKING);
    assert_eq!(env.svm.get_sysvar::<solana_sdk::clock::Clock>().slot, 7);

    // Existing insurance exits are cleanup only; the new claim is provider progress.
    for (asset_index, actor, amount) in [(1, 3, INSURANCE), (0, 4, UNRELATED_INSURANCE)] {
        let ix = wrap(
            env,
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id: env.asset_market_id(asset_index),
                authority_epoch: env.control_sequences(asset_index as usize).authority_epoch,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new_readonly(wallets[actor], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        let allowed = [env.market, env.vault, tokens[actor]];
        peak = peak.max(land(env, &[ix], &[], &allowed, None, None));
        check_stock(env, &[]);
    }
    let terminal = env.market_state().1;
    assert_eq!(
        (
            terminal.vault,
            terminal.c_tot,
            terminal.insurance,
            terminal.materialized_portfolio_count
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(
        terminal.source_backing_buckets[3].consumed_liened_backing_num,
        u128::from(PROFIT) * BOUND_SCALE
    );
    assert_eq!(
        tokens.map(|key| env.token_amount(key)),
        [
            PAYOUTS[0],
            PAYOUTS[1],
            BACKING,
            INSURANCE,
            UNRELATED_INSURANCE
        ]
    );
    let close = wrap(
        env,
        ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(tokens[4], false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
    );
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let refund = env.svm.get_account(&env.market).unwrap().lamports
        + env.svm.get_account(&env.vault).unwrap().lamports
        - rent;
    let allowed = [env.market, env.vault];
    peak = peak.max(land(
        env,
        &[close],
        &[&admin],
        &allowed,
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
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    println!("row420 replenished provider: winner_first={winner_first}, user_calls={calls}/16, consumed={PROFIT}, principal_paid={BACKING}, rollback=1, provider_payments=2, slab_close=1, peak_CU={peak}, limit={LIMIT}");
}
