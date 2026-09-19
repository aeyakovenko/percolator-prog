//! INV-070: provider withdrawals around a cached prefix change the later expiry's
//! recreditable stock. Full withdrawal removes the wait without creating residual.
//! INV-024/025/033/041/063/069/071/086/088 receive bounded stock and ordering evidence.

use super::*;

#[test]
fn v16_program_terminal_scan_recomputes_residual_after_provider_withdrawal_order() {
    const BACKING: u64 = 307;
    let mut peak = 0;
    let mut commits = 0;
    let mut rollbacks = 0;
    for side in 0..2 {
        for withdrawn in [107, 246, BACKING] {
            let remaining = BACKING - withdrawn;
            let recovered = remaining.min(SPENT).min(CAPITAL[1]);
            let residue = remaining - recovered;
            let mut outcomes = Vec::new();
            for after_prefix in [false, true] {
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
                } = fixture(side, BACKING);
                peak = peak.max(fixture_peak);
                let initial_ledger = env.market_state().1.resolved_payout_ledger;
                let initial_sequences = [0, 1].map(|asset| env.control_sequences(asset));
                let close = |env: &V16CuEnv| {
                    wrap(
                        env,
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
                    )
                };
                let principal = wrap(
                    &env,
                    ProgInstruction::WithdrawBackingBucket {
                        domain: (2 + side) as u16,
                        market_id: env.asset_market_id(1),
                        authority_epoch: initial_sequences[1].authority_epoch,
                        amount: withdrawn.into(),
                    },
                    vec![
                        AccountMeta::new_readonly(admin.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                let deadline_probe = Instruction {
                    data: ProgInstruction::WithdrawBackingBucket {
                        domain: (2 + side) as u16,
                        market_id: env.asset_market_id(1),
                        authority_epoch: initial_sequences[1].authority_epoch,
                        amount: 1,
                    }
                    .encode(),
                    ..principal.clone()
                };
                let payout = wrap(
                    &env,
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: initial_sequences[0].authority_epoch,
                        amount: recovered.max(1).into(),
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
                    solana_sdk::sysvar::clock::ID,
                ];
                tracked.extend(tokens);
                tracked.extend(portfolios);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                drop(beneficiary);
                let market_only = [env.market];
                let principal_accounts = [env.market, env.vault, destination];
                let payment = [env.market, env.vault, reserve];
                let closing = [env.market, env.vault, env.mint, admin.pubkey()];
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
                // Expected amounts come only from fixture inputs and committed public actions.
                let check = |env: &V16CuEnv,
                             drained: bool,
                             expired: bool,
                             restored: u64,
                             paid: u64,
                             cursor: u128| {
                    let principal_paid = if drained { withdrawn } else { 0 };
                    let retained = BACKING - principal_paid;
                    let fresh = if expired { 0 } else { retained };
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg.terminal_slab_scan_progress, cursor);
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(
                        (
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(group.vault, u128::from(retained - paid));
                    assert_eq!(group.insurance, u128::from(restored - paid));
                    assert_eq!(group.backing_provider_earnings_total, 0);
                    assert_eq!(env.token_amount(env.vault), retained - paid);
                    assert_eq!(env.token_amount(destination), principal_paid);
                    assert_eq!(env.token_amount(reserve), paid);
                    assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
                    let supply = CAPITAL.iter().sum::<u64>() + SPENT + BACKING;
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(mint.supply, supply);
                    assert_eq!(mint.mint_authority, COption::None);
                    assert_eq!(
                        PAYOUTS.iter().sum::<u64>()
                            + principal_paid
                            + paid
                            + env.token_amount(env.vault),
                        supply
                    );
                    for domain in 0..4 {
                        let bucket = group.source_backing_buckets[domain];
                        let source = group.source_credit[domain];
                        let expected_fresh = if domain == 2 + side {
                            u128::from(fresh) * BOUND_SCALE
                        } else {
                            0
                        };
                        assert_eq!(bucket.fresh_unliened_backing_num, expected_fresh);
                        assert_eq!(source.fresh_reserved_backing_num, expected_fresh);
                        assert_eq!(
                            (
                                bucket.valid_liened_backing_num,
                                bucket.utilization_fee_earnings
                            ),
                            (0, 0)
                        );
                        assert_eq!(
                            (
                                source.valid_liened_backing_num,
                                source.positive_claim_bound_num
                            ),
                            (0, 0)
                        );
                        assert_eq!(
                            source.provider_receivable_num,
                            if domain == 1 - side {
                                u128::from(CAPITAL[1]) * BOUND_SCALE
                            } else {
                                0
                            }
                        );
                        assert_eq!(
                            group.insurance_domain_budget[domain],
                            if domain == side {
                                u128::from(SPENT - paid)
                            } else {
                                0
                            }
                        );
                        assert_eq!(
                            group.insurance_domain_spent[domain],
                            if domain == side {
                                u128::from(SPENT - restored)
                            } else {
                                0
                            }
                        );
                    }
                    let bucket = group.source_backing_buckets[2 + side];
                    assert_eq!(
                        bucket.status,
                        if retained == 0 {
                            BackingBucketStatusV16::Empty
                        } else if expired {
                            BackingBucketStatusV16::Expired
                        } else {
                            BackingBucketStatusV16::Fresh
                        }
                    );
                    assert_eq!(bucket.expiry_slot, if retained == 0 { 0 } else { EXPIRY });
                    let mut ledger = initial_ledger;
                    if expired {
                        ledger.snapshot_residual += u128::from(retained);
                    }
                    assert_eq!(group.resolved_payout_ledger, ledger);
                    let mut sequences = initial_sequences;
                    sequences[0].authority_epoch += u64::from(paid != 0);
                    assert_eq!([0, 1].map(|asset| env.control_sequences(asset)), sequences);
                    let market = env.svm.get_account(&env.market).unwrap();
                    crate::support::fuzz_model::assert_market_stock_census(
                        "provider scan order",
                        &group,
                        &market.data,
                        &[],
                        u128::from(retained - paid),
                    )
                    .unwrap();
                    crate::support::fuzz_model::assert_reservation_encumbrance_census(
                        "provider scan order",
                        &group,
                        &[],
                    )
                    .unwrap();
                    // Fresh backing, unrecredited recovery, unpaid recovery, unscanned slots,
                    // and the live slab form a finite lexicographic rank for this continuation.
                    [
                        u128::from(fresh),
                        group.insurance_domain_spent[side] - u128::from(SPENT - recovered),
                        u128::from(recovered - paid),
                        2 - cursor,
                        1,
                    ]
                };
                let mut rank = check(&env, false, false, 0, 0, 0);
                let scan = close(&env);
                if after_prefix {
                    send(
                        &mut env,
                        &[scan.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    let next = check(&env, false, false, 0, 0, 1);
                    assert!(next < rank);
                    rank = next;
                }
                let cursor = u128::from(after_prefix);
                let earlier_slot =
                    market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 0)
                        .to_vec();
                let bad = Instruction {
                    program_id: system_program::ID,
                    accounts: vec![],
                    data: vec![255],
                };
                send(
                    &mut env,
                    &[principal.clone(), bad.clone()],
                    &[],
                    &[],
                    Some((3, InstructionError::InvalidInstructionData)),
                    (1, 1),
                );
                assert_eq!(check(&env, false, false, 0, 0, cursor), rank);
                send(
                    &mut env,
                    &[principal.clone()],
                    &[],
                    &principal_accounts,
                    None,
                    (1, 1),
                );
                assert_eq!(
                    market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 0),
                    earlier_slot
                );
                let next = check(&env, true, false, 0, 0, cursor);
                assert!(next < rank);
                rank = next;
                if !after_prefix && remaining != 0 {
                    send(
                        &mut env,
                        &[scan.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    let next = check(&env, true, false, 0, 0, 1);
                    assert!(next < rank);
                    rank = next;
                }
                let cursor = if remaining != 0 { 1 } else { cursor };
                send(
                    &mut env,
                    &[payout.clone()],
                    &[],
                    &[],
                    Some((
                        2,
                        InstructionError::Custom(if remaining == 0 {
                            PercolatorError::InvalidTokenAccount as u32
                        } else {
                            PercolatorError::EngineLockActive as u32
                        }),
                    )),
                    (0, 0),
                );
                let parked = env.svm.get_account(&env.market);
                env.svm.warp_to_slot(EXPIRY);
                assert_eq!(env.svm.get_account(&env.market), parked);
                assert_eq!(check(&env, true, false, 0, 0, cursor), rank);
                // A retained one-atom request reaches the deadline guard when custody remains.
                // Fully drained custody rejects earlier in token preflight.
                send(
                    &mut env,
                    &[deadline_probe],
                    &[],
                    &[],
                    Some((
                        2,
                        InstructionError::Custom(if remaining == 0 {
                            PercolatorError::InvalidTokenAccount as u32
                        } else {
                            PercolatorError::EngineStale as u32
                        }),
                    )),
                    (0, 0),
                );
                if remaining != 0 {
                    send(
                        &mut env,
                        &[scan.clone(), scan.clone(), bad.clone()],
                        &[&admin],
                        &[],
                        Some((4, InstructionError::InvalidInstructionData)),
                        (2, 0),
                    );
                    assert_eq!(check(&env, true, false, 0, 0, cursor), rank);
                    send(
                        &mut env,
                        &[scan.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    let next = check(&env, true, true, 0, 0, 0);
                    assert!(next < rank);
                    rank = next;
                    assert_eq!(
                        market_engine_slot_bytes(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0
                        ),
                        earlier_slot
                    );
                    send(
                        &mut env,
                        &[scan.clone()],
                        &[&admin],
                        &market_only,
                        None,
                        (1, 0),
                    );
                    let next = check(&env, true, true, recovered, 0, 0);
                    assert!(next < rank);
                    rank = next;
                    send(
                        &mut env,
                        &[scan],
                        &[&admin],
                        &[],
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                        )),
                        (0, 0),
                    );
                    send(&mut env, &[payout], &[], &payment, None, (1, 1));
                    let next = check(&env, true, true, recovered, recovered, 0);
                    assert!(next < rank);
                    rank = next;
                }
                let market = env.svm.get_account(&env.market).unwrap();
                let vault = env.svm.get_account(&env.vault).unwrap();
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                mint.supply -= residue;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                let final_close = close(&env);
                let token_calls = 1 + usize::from(residue != 0);
                send(
                    &mut env,
                    &[final_close.clone(), bad],
                    &[&admin],
                    &[],
                    Some((3, InstructionError::InvalidInstructionData)),
                    (1, token_calls),
                );
                assert_eq!(
                    check(
                        &env,
                        true,
                        remaining != 0,
                        recovered,
                        recovered,
                        if remaining != 0 { 0 } else { cursor }
                    ),
                    rank
                );
                send(
                    &mut env,
                    &[final_close],
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
                assert_eq!(env.token_amount(destination), withdrawn);
                assert_eq!(env.token_amount(reserve), recovered);
                assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
                assert_eq!(
                    PAYOUTS.iter().sum::<u64>() + withdrawn + recovered,
                    mint.supply
                );
                assert!([0u128; 5] < rank);
                outcomes.push((withdrawn, recovered, residue, mint.supply));
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "scan/provider ordering preserves final attribution"
            );
        }
    }
    assert_eq!((commits, rollbacks), (58, 64));
    println!("INV-070 provider scan order: 12 histories, {commits} commits, {rollbacks} rollbacks, peak={peak} CU");
}
