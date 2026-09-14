//! INV-070/071/088, row 424: external custody after a saved prefix is not insurance backing.
//! Eight public worlds compare donated/control custody across both source sides and
//! residual-limited/full recovery. A retained payout must wait for booked expiry,
//! recompute its exact local entitlement, and preserve surplus for final disposition.

use super::*;
use inv_071_crank_progress::terminal_prefix_recredit::{
    fixture, land, stocks_with_custody_transfer, wrap, RecreditFixture, CAPITAL, EXPIRY, PAYOUTS,
    SPENT,
};
use solana_sdk::{instruction::InstructionError, system_program};

const DONATION: u64 = 89;

fn rank(env: &V16CuEnv, side: usize, recovered: u64, reserve: Pubkey) -> [u128; 5] {
    let (cfg, group) = env.market_state();
    [
        group
            .source_backing_buckets
            .iter()
            .filter(|bucket| bucket.status == BackingBucketStatusV16::Fresh)
            .count() as u128,
        group.insurance_domain_spent[side] - u128::from(SPENT - recovered),
        u128::from(recovered - env.token_amount(reserve)),
        group.assets.len() as u128 - cfg.terminal_slab_scan_progress,
        1,
    ]
}

#[test]
fn v16_program_cached_prefix_custody_surplus_cannot_capitalize_spent_insurance() {
    let lock = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
    let mut peak = 0;
    let mut commits = 0;
    let mut rollbacks = 0;
    let mut comparisons = 0;
    for side in 0..2 {
        for backing in [61u64, 307] {
            let recovered = CAPITAL[1].min(SPENT).min(backing);
            let partial = recovered / 3;
            let mut outcomes = Vec::new();
            for donated in [false, true] {
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
                let close = wrap(
                    &env,
                    ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
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
                let withdrawal = |amount: u64| {
                    wrap(
                        &env,
                        ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            authority_epoch: env.control_sequences(0).authority_epoch,
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
                let first = withdrawal(partial);
                let tail = withdrawal(recovered - partial);
                let excess = withdrawal(recovered + 1);
                let transfer = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &tokens[0],
                    &env.vault,
                    &owners[0].pubkey(),
                    &[],
                    DONATION,
                )
                .unwrap();
                let bad_suffix = Instruction {
                    program_id: system_program::ID,
                    accounts: Vec::new(),
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
                    solana_sdk::sysvar::clock::ID,
                ];
                tracked.extend(tokens);
                tracked.extend(portfolios);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                drop(beneficiary);
                let market_only = [env.market];
                let payment = [env.market, env.vault, reserve];
                let custody_transfer = [tokens[0], env.vault];
                let closing = [env.market, env.vault, env.mint, destination, admin.pubkey()];
                let ledger = env.market_state().1.resolved_payout_ledger;
                let check = |env: &V16CuEnv, normalized, restored, paid, cursor, transferred| {
                    stocks_with_custody_transfer(
                        env,
                        side,
                        backing,
                        normalized,
                        restored,
                        paid,
                        tokens,
                        reserve,
                        cursor,
                        transferred,
                    );
                    let mut expected = ledger;
                    if normalized {
                        expected.snapshot_residual += u128::from(backing);
                    }
                    assert_eq!(env.market_state().1.resolved_payout_ledger, expected);
                    assert_eq!(env.token_amount(destination), 0);
                };
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

                check(&env, false, 0, 0, 0, 0);
                let initial_rank = rank(&env, side, recovered, reserve);
                send(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &market_only,
                    None,
                    (1, 0),
                );
                check(&env, false, 0, 0, 1, 0);
                let prefix_rank = rank(&env, side, recovered, reserve);
                assert!(prefix_rank < initial_rank);
                send(
                    &mut env,
                    &[first.clone()],
                    &[],
                    &[],
                    Some((2, lock.clone())),
                    (0, 0),
                );

                let transferred = if donated { DONATION } else { 0 };
                if donated {
                    send(
                        &mut env,
                        &[transfer.clone(), first.clone()],
                        &[&owners[0]],
                        &[],
                        Some((3, lock.clone())),
                        (0, 1),
                    );
                    check(&env, false, 0, 0, 1, 0);
                    send(
                        &mut env,
                        &[transfer],
                        &[&owners[0]],
                        &custody_transfer,
                        None,
                        (0, 1),
                    );
                    check(&env, false, 0, 0, 1, transferred);
                    assert!(env.token_amount(env.vault) > recovered);
                    send(
                        &mut env,
                        &[first.clone()],
                        &[],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                }
                assert_eq!(rank(&env, side, recovered, reserve), prefix_rank);
                let market_before_clock = env.svm.get_account(&env.market).unwrap();
                env.svm.warp_to_slot(EXPIRY);
                assert_eq!(env.svm.get_account(&env.market), Some(market_before_clock));
                check(&env, false, 0, 0, 1, transferred);

                // Both expiry and the retained insurance payment execute before rollback.
                send(
                    &mut env,
                    &[close.clone(), first.clone(), bad_suffix.clone()],
                    &[&admin],
                    &[],
                    Some((4, InstructionError::InvalidInstructionData)),
                    (2, 1),
                );
                check(&env, false, 0, 0, 1, transferred);
                assert_eq!(rank(&env, side, recovered, reserve), prefix_rank);
                send(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &market_only,
                    None,
                    (1, 0),
                );
                check(&env, true, 0, 0, 0, transferred);
                let expired_rank = rank(&env, side, recovered, reserve);
                assert!(expired_rank < prefix_rank);

                if donated {
                    // Raw custody can fund this amount; the current local entitlement cannot.
                    assert!(env.token_amount(env.vault) > recovered);
                    send(
                        &mut env,
                        &[excess],
                        &[],
                        &[],
                        Some((2, lock.clone())),
                        (0, 0),
                    );
                    check(&env, true, 0, 0, 0, transferred);
                }
                send(&mut env, &[first], &[], &payment, None, (1, 1));
                check(&env, true, recovered, partial, 0, transferred);
                let partial_rank = rank(&env, side, recovered, reserve);
                assert!(partial_rank < expired_rank);
                send(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &[],
                    Some((2, lock.clone())),
                    (0, 0),
                );
                send(&mut env, &[tail], &[], &payment, None, (1, 1));
                check(&env, true, recovered, recovered, 0, transferred);
                let paid_rank = rank(&env, side, recovered, reserve);
                assert!(paid_rank < partial_rank);

                let market = env.svm.get_account(&env.market).unwrap();
                let vault = env.svm.get_account(&env.vault).unwrap();
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                let burned = backing - recovered;
                mint.supply -= burned;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                let token_calls = 1 + usize::from(burned != 0) + usize::from(donated);
                send(
                    &mut env,
                    &[close.clone(), bad_suffix],
                    &[&admin],
                    &[],
                    Some((3, InstructionError::InvalidInstructionData)),
                    (1, token_calls),
                );
                check(&env, true, recovered, recovered, 0, transferred);
                assert_eq!(rank(&env, side, recovered, reserve), paid_rank);
                send(
                    &mut env,
                    &[close],
                    &[&admin],
                    &closing,
                    None,
                    (1, token_calls),
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
                    .is_none_or(|a| a.lamports == 0));
                assert_eq!(env.token_amount(reserve), recovered);
                assert_eq!(env.token_amount(destination), transferred);
                let mut final_users = PAYOUTS;
                final_users[0] -= transferred;
                assert_eq!(tokens.map(|key| env.token_amount(key)), final_users);
                assert_eq!(
                    final_users.iter().sum::<u64>() + recovered + transferred,
                    mint.supply
                );
                assert!([0u128; 5] < paid_rank);
                outcomes.push((recovered, burned, mint.supply));
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "external custody cannot enlarge insurance recovery"
            );
            comparisons += 1;
        }
    }
    assert_eq!((commits, rollbacks, comparisons), (44, 44, 4));
    println!("INV-070/071/088 prefix custody actionability: 8 worlds, {commits} commits, {rollbacks} exact rollbacks, {comparisons} donation/control comparisons, peak={peak} CU");
}
