//! Three equal Live claims share scarce replacement backing after public expiry normalization.
//! Cross every claim permutation with both close directions; a rejected caller-cap bundle must
//! roll back its successful conversion/SPL-withdrawal prefix before the exact-cap retry.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use percolator::CREDIT_RATE_SCALE;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_equal_claim_refill_permutations_preserve_allocation_and_atomic_caps() {
    const CAPITAL: u128 = 1_000;
    const OPEN: u64 = 100;
    const MARK: u64 = 105;
    const LOTS: u128 = 10;
    const FACE: u128 = LOTS * (MARK - OPEN) as u128;
    const BACKING: u128 = 17;
    const INSURANCE: u128 = 19;
    const REFILL: u128 = 75;
    const SHARE: u128 = REFILL / 3;
    const DOMAIN: usize = 1;
    const EXPIRY: u64 = 4;
    const SUPPLY: u128 = 4 * CAPITAL + BACKING + INSURANCE + REFILL;
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    assert_eq!(FACE, 50);
    assert_eq!(SHARE * 3, REFILL);
    assert!(SHARE > 0 && SHARE < FACE);
    let mut baseline = None;
    let mut worlds = 0;
    let mut peak_reject_cu = 0;
    let mut peak_exit_cu = 0;

    for close_order in [ORDERS[0], ORDERS[5]] {
        for claim_order in ORDERS {
            let label = format!("close={close_order:?} claim={claim_order:?}");
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN);
            let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
            let portfolios = owners.each_ref().map(|owner| {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[owner],
                )
                .expect("public portfolio initialization");
                env.portfolios.push(key.pubkey());
                key.pubkey()
            });
            let tokens = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            let provider =
                create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
            for (token, amount) in tokens
                .map(|token| (token, CAPITAL))
                .into_iter()
                .chain([(provider, BACKING + INSURANCE + REFILL)])
            {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &env.admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &env.admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            for actor in 0..4 {
                env.send(
                    env.deposit_ix(portfolios[actor], CAPITAL),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .expect("deposit finite SPL funds");
            }
            env.top_up_insurance_from_admin_token_with_cu(provider, INSURANCE);
            env.top_up_backing_bucket_from_admin_token_with_cu(
                provider,
                DOMAIN as u16,
                BACKING,
                EXPIRY,
            );
            for actor in 0..3 {
                env.trade_with_cu(
                    &owners[actor],
                    portfolios[actor],
                    &owners[3],
                    portfolios[3],
                    (LOTS * POS_SCALE) as i128,
                    OPEN,
                    0,
                );
            }
            env.svm.warp_to_slot(2);
            env.push_auth_mark_for_asset_as_admin(0, 2, MARK);
            for actor in [3, 0, 1, 2] {
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                );
            }
            for actor in close_order {
                let cu = env.trade_with_cu(
                    &owners[actor],
                    portfolios[actor],
                    &owners[3],
                    portfolios[3],
                    -((LOTS * POS_SCALE) as i128),
                    MARK,
                    0,
                );
                assert_cu_within(&label, cu, TRADE_CU_LIMIT);
            }
            let settled = env.market_state().1;
            assert_eq!(
                settled.source_credit[DOMAIN].positive_claim_bound_num,
                3 * FACE * BOUND_SCALE
            );
            assert_eq!(
                settled.source_backing_buckets[DOMAIN].fresh_unliened_backing_num,
                (BACKING + 3 * FACE) * BOUND_SCALE
            );
            assert_eq!(
                env.portfolio_state(portfolios[3]).capital.get(),
                CAPITAL - 3 * FACE
            );

            // Expiry removes unused support, not the three owner-attributed faces. The replacement
            // restores exactly half their aggregate support; insurance must not fill the haircut.
            env.svm.warp_to_slot(EXPIRY);
            let mut normalization_steps = 0;
            while env.market_state().1.source_backing_buckets[DOMAIN].status
                == BackingBucketStatusV16::Fresh
            {
                assert!(
                    normalization_steps < 3,
                    "{label}: bounded expiry normalization"
                );
                env.svm.expire_blockhash();
                let cu = env.crank(
                    portfolios[0],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(0),
                    },
                );
                assert_cu_within(&label, cu, CRANK_CU_LIMIT);
                normalization_steps += 1;
            }
            let expired = env.market_state().1;
            assert_eq!(
                expired.source_backing_buckets[DOMAIN].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(expired.source_credit[DOMAIN].fresh_reserved_backing_num, 0);
            assert_eq!(
                expired.source_credit[DOMAIN].positive_claim_bound_num,
                3 * FACE * BOUND_SCALE
            );
            assert_eq!(expired.source_credit[DOMAIN].credit_rate_num, 0);
            env.top_up_backing_bucket_from_admin_token_with_cu(provider, DOMAIN as u16, REFILL, 20);
            for actor in 0..3 {
                env.svm.expire_blockhash();
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(0),
                    },
                );
            }

            let check = |env: &V16CuEnv, converted: [bool; 3], paid: [u128; 4]| {
                let count = converted.into_iter().filter(|done| *done).count() as u128;
                let face = (3 - count) * FACE;
                let spent = count * SHARE;
                let fresh = REFILL - spent;
                let group = env.market_state().1;
                let source = group.source_credit[DOMAIN];
                let bucket = group.source_backing_buckets[DOMAIN];
                assert_eq!(
                    source.positive_claim_bound_num,
                    face * BOUND_SCALE,
                    "{label}"
                );
                assert_eq!(source.exact_positive_claim_num, face * BOUND_SCALE);
                assert_eq!(source.fresh_reserved_backing_num, fresh * BOUND_SCALE);
                assert_eq!(source.spent_backing_num, spent * BOUND_SCALE);
                assert_eq!(source.provider_receivable_num, spent * BOUND_SCALE);
                assert_eq!(
                    source.credit_rate_num,
                    if face == 0 {
                        CREDIT_RATE_SCALE
                    } else {
                        CREDIT_RATE_SCALE / 2
                    }
                );
                assert_eq!(bucket.fresh_unliened_backing_num, fresh * BOUND_SCALE);
                assert_eq!(bucket.consumed_liened_backing_num, spent * BOUND_SCALE);
                assert_eq!(
                    [
                        source.valid_liened_backing_num,
                        source.impaired_liened_backing_num,
                        source.insurance_credit_reserved_num,
                        source.valid_liened_insurance_num,
                        source.impaired_liened_insurance_num,
                        bucket.valid_liened_backing_num,
                        bucket.impaired_liened_backing_num,
                        bucket.utilization_fee_earnings,
                    ],
                    [0; 8]
                );
                let mut census = 0;
                for actor in 0..4 {
                    let account = env.portfolio_state(portfolios[actor]);
                    let expected_face = if actor < 3 && !converted[actor] {
                        FACE
                    } else {
                        0
                    };
                    let capital = if actor == 3 {
                        CAPITAL - 3 * FACE
                    } else {
                        CAPITAL + u128::from(converted[actor]) * SHARE
                    };
                    assert_eq!(account.capital.get(), capital - paid[actor]);
                    assert_eq!(account.pnl.get(), expected_face as i128);
                    assert_eq!(account.reserved_pnl.get(), 0);
                    assert_eq!(account.fee_credits.get(), 0);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    let mut local_face = 0;
                    for local in account
                        .source_domains
                        .iter()
                        .filter(|local| local.is_occupied())
                    {
                        let bound = local.source_claim_bound_num.get();
                        if bound != 0 {
                            assert_eq!(local.domain.get() as usize, DOMAIN);
                        }
                        local_face += bound;
                        assert_eq!(local.source_claim_liened_num.get(), 0);
                        assert_eq!(local.source_claim_impaired_num.get(), 0);
                        assert_eq!(local.source_lien_counterparty_backing_num.get(), 0);
                    }
                    assert_eq!(local_face, expected_face * BOUND_SCALE);
                    census += local_face;
                    assert_eq!(u128::from(env.token_amount(tokens[actor])), paid[actor]);
                }
                assert_eq!(census, group.source_claim_bound_total_num);
                assert_eq!(group.source_credit[0], settled.source_credit[0]);
                assert_eq!(
                    group.source_backing_buckets[0],
                    settled.source_backing_buckets[0]
                );
                assert_eq!(
                    group.insurance_domain_budget,
                    settled.insurance_domain_budget
                );
                assert_eq!(group.insurance, INSURANCE);
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::Active);
                assert_eq!(group.assets[0].oi_eff_long_q, 0);
                assert_eq!(group.assets[0].oi_eff_short_q, 0);
                assert_eq!(
                    group.c_tot,
                    4 * CAPITAL - 3 * FACE + spent - paid.iter().sum::<u128>()
                );
                assert_eq!(group.vault, SUPPLY - paid.iter().sum::<u128>());
                assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                assert_eq!(env.token_amount(provider), 0);
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(u128::from(mint.supply), SUPPLY);
                assert_eq!(mint.mint_authority, COption::None);
            };
            check(&env, [false; 3], [0; 4]);
            let convert_ix = |env: &V16CuEnv, actor: usize, cap| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                data: env.convert_released_pnl_ix(portfolios[actor], cap).encode(),
            };
            let withdraw_ix = |env: &V16CuEnv, actor: usize, amount| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(portfolios[actor], amount).encode(),
            };

            let [first, second, _] = claim_order;
            let mut prefix = vec![
                heap_ix(),
                cu_ix(),
                convert_ix(&env, first, SHARE),
                withdraw_ix(&env, first, CAPITAL + SHARE),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(env.payer.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[second], false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(0),
                    }
                    .encode(),
                },
                convert_ix(&env, second, SHARE),
            ];
            env.svm
                .simulate_transaction(
                    Transaction::new_signed_with_payer(
                        &prefix,
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &owners[first], &owners[second]],
                        env.svm.latest_blockhash(),
                    )
                    .into(),
                )
                .expect("positive control: both conversions and the SPL withdrawal fit exact caps");
            *prefix.last_mut().unwrap() = convert_ix(&env, second, SHARE - 1);
            let tx = Transaction::new_signed_with_payer(
                &prefix,
                Some(&env.payer.pubkey()),
                &[&env.payer, &owners[first], &owners[second]],
                env.svm.latest_blockhash(),
            );
            let mut keys = vec![
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                provider,
                env.admin.pubkey(),
            ];
            keys.extend(portfolios);
            keys.extend(tokens);
            keys.extend(owners.each_ref().map(|owner| owner.pubkey()));
            keys.extend(tx.message.account_keys.iter().copied());
            keys.sort_unstable();
            keys.dedup();
            keys.retain(|key| *key != env.payer.pubkey());
            let frame = |env: &V16CuEnv| {
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let before = frame(&env);
            let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
            payer_before.lamports -= u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
            let rejected = env
                .svm
                .send_transaction(tx)
                .expect_err("second caller cap is one atom too low");
            assert_eq!(
                rejected.err,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                ),
                "{label}"
            );
            assert_eq!(
                frame(&env),
                before,
                "{label}: rollback includes the SPL-paying prefix"
            );
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()).unwrap(),
                payer_before
            );
            peak_reject_cu = peak_reject_cu.max(rejected.meta.compute_units_consumed);
            assert_cu_within(
                &label,
                rejected.meta.compute_units_consumed,
                3 * CUSTODY_CU_LIMIT + CRANK_CU_LIMIT,
            );
            check(&env, [false; 3], [0; 4]);

            let mut converted = [false; 3];
            let mut paid = [0; 4];
            for actor in claim_order {
                // A prior claimant changes the shared credit epoch; refresh this owner's certificate
                // through a bounded public crank before applying the same input-derived 25-atom cap.
                env.svm.expire_blockhash();
                if let Some(cu) = env.crank_if_actionable(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(0),
                    },
                ) {
                    assert_cu_within(&label, cu, CRANK_CU_LIMIT);
                }
                check(&env, converted, paid);
                let siblings = portfolios
                    .iter()
                    .copied()
                    .filter(|key| *key != portfolios[actor])
                    .map(|key| (key, env.svm.get_account(&key)))
                    .collect::<Vec<_>>();
                let cu = env.convert_released_pnl_with_cu(&owners[actor], portfolios[actor], SHARE);
                assert_cu_within(&label, cu, CUSTODY_CU_LIMIT);
                converted[actor] = true;
                check(&env, converted, paid);
                let ix = withdraw_ix(&env, actor, CAPITAL + SHARE);
                let cu = send_raw_tx(&mut env.svm, &env.payer, ix, &[&owners[actor]]).unwrap();
                assert_cu_within(&label, cu, CUSTODY_CU_LIMIT);
                peak_exit_cu = peak_exit_cu.max(cu);
                paid[actor] = CAPITAL + SHARE;
                check(&env, converted, paid);
                for (key, before) in siblings {
                    assert_eq!(env.svm.get_account(&key), before);
                }
            }
            let ix = withdraw_ix(&env, 3, CAPITAL - 3 * FACE);
            send_raw_tx(&mut env.svm, &env.payer, ix, &[&owners[3]]).unwrap();
            paid[3] = CAPITAL - 3 * FACE;
            check(&env, converted, paid);
            assert_eq!(paid, [1_025, 1_025, 1_025, 850]);
            let mut group = env.market_state().1;
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.vault, BACKING + 3 * FACE + INSURANCE);
            // Only random market identity is normalized; no source, insurance, epoch or custody
            // field is removed from the cross-world market comparison.
            group.market_group_id = [0; 32];
            let outcome = (
                group,
                paid,
                tokens.map(|key| env.token_amount(key)),
                env.token_amount(env.vault),
            );
            if let Some(expected) = &baseline {
                assert_eq!(&outcome, expected, "{label}");
            } else {
                baseline = Some(outcome);
            }
            worlds += 1;
        }
    }
    assert_eq!(worlds, 12);
    println!("INV-041: {worlds} worlds / 12 atomic cap rejections; payouts=[1025,1025,1025,850]; residual=186; peak reject/withdraw CU={peak_reject_cu}/{peak_exit_cu}");
}
