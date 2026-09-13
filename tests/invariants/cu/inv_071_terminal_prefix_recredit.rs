//! Row 424: later-slot expiry makes spent insurance on a scanned asset payable.
//! The asset-local withdrawal must recompute from current stocks despite cursor 1.
//! Public construction only; this does not certify the scanner's own rediscovery.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 3] = [1_000, 100, 137];
const GAIN: u64 = 10 * 20;
const SPENT: u64 = GAIN - CAPITAL[1];
const PAYOUTS: [u64; 3] = [CAPITAL[0] + GAIN, 0, CAPITAL[2]];
const EXPIRY: u64 = 44;
const LIMIT: u64 = 400_000;

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rejection: Option<u8>,
    successes: (usize, usize),
) -> u64 {
    env.svm.expire_blockhash();
    let mut batch = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    batch.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + signers.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let payer = keys
        .iter()
        .position(|key| *key == env.payer.pubkey())
        .unwrap();
    before[payer].as_mut().unwrap().lamports -=
        u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = rejection {
        assert!(changed.is_empty());
        let failure = result.expect_err("unpaid insurance or unnormalized backing remains");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32)
            )
        );
        failure.meta
    } else {
        result.expect("public earlier-asset insurance continuation")
    };
    for (key, account) in keys.iter().zip(before) {
        if !changed.contains(key) {
            assert_eq!(
                env.svm.get_account(key),
                account,
                "complete Account frame: {key}"
            );
        }
    }
    for (program, count) in [(env.program_id, successes.0), (spl_token::ID, successes.1)] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count
        );
    }
    assert_cu_within(
        "INV-071 earlier-asset recredit",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn stocks(
    env: &V16CuEnv,
    side: usize,
    backing: u64,
    normalized: bool,
    restored: u64,
    paid: u64,
    tokens: [Pubkey; 3],
    beneficiary_token: Pubkey,
) {
    let (cfg, group) = env.market_state();
    let market = env.svm.get_account(&env.market).unwrap();
    let header = market_group_header_bytes(&market.data);
    let fresh = if normalized {
        0
    } else {
        u128::from(backing) * BOUND_SCALE
    };
    let insurance = u128::from(restored - paid);
    assert_eq!(cfg.terminal_slab_scan_progress, 1);
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.materialized_portfolio_count
        ),
        (0, 0, 0)
    );
    assert!(group
        .assets
        .iter()
        .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
    assert_eq!(group.vault, u128::from(backing - paid));
    assert_eq!(group.insurance, insurance);
    assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
    assert_eq!(env.token_amount(beneficiary_token), paid);
    assert_eq!(env.token_amount(env.vault), backing - paid);
    let supply = CAPITAL.iter().sum::<u64>() + SPENT + backing;
    assert_eq!(
        PAYOUTS.iter().sum::<u64>() + paid + env.token_amount(env.vault),
        supply
    );
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, supply);
    assert_eq!(mint.mint_authority, COption::None);

    for domain in 0..4 {
        let bucket = group.source_backing_buckets[domain];
        let source = group.source_credit[domain];
        let expected_fresh = if domain == 2 + side { fresh } else { 0 };
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
    assert_eq!(
        group.source_backing_buckets[2 + side].status,
        if normalized {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(group.source_backing_buckets[2 + side].expiry_slot, EXPIRY);
    assert!(group.source_backing_buckets[..2]
        .iter()
        .all(|bucket| bucket.status != BackingBucketStatusV16::Fresh));
    let decoded_fresh = group
        .source_backing_buckets
        .iter()
        .map(|bucket| bucket.fresh_unliened_backing_num)
        .sum::<u128>();
    let decoded_remaining = group
        .insurance_domain_budget
        .iter()
        .zip(&group.insurance_domain_spent)
        .map(|(budget, spent)| budget.checked_sub(*spent).unwrap())
        .sum::<u128>();
    assert_eq!(decoded_fresh, fresh);
    assert_eq!(decoded_remaining, insurance);
    assert_eq!(header.source_fresh_backing_total_num.get(), decoded_fresh);
    assert_eq!(
        header.insurance_domain_budget_remaining_total.get(),
        decoded_remaining
    );

    // Recompute the earlier asset's current entitlement from decoded stocks, never cursor/summary.
    let residual = group
        .vault
        .checked_sub(group.insurance + decoded_fresh / BOUND_SCALE)
        .unwrap();
    let local_overlap = (group.source_credit[1 - side].provider_receivable_num / BOUND_SCALE)
        .min(group.insurance_domain_spent[side])
        .min(residual);
    assert_eq!(
        insurance + local_overlap,
        if normalized {
            u128::from(CAPITAL[1].min(SPENT).min(backing) - paid)
        } else {
            0
        }
    );
    crate::support::fuzz_model::assert_market_stock_census(
        "earlier-asset recredit",
        &group,
        &market.data,
        &[],
        group.vault,
    )
    .unwrap();
    crate::support::fuzz_model::assert_reservation_encumbrance_census(
        "earlier-asset recredit",
        &group,
        &[],
    )
    .unwrap();
}

#[test]
fn v16_program_later_expiry_recomputes_scanned_asset_insurance_entitlement() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    let mut peak = 0;
    for side in 0..2 {
        for backing in [61u64, 307] {
            for late in [false, true] {
                for bundled in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            maintenance_margin_bps: 1_000,
                            initial_margin_bps: 1_000,
                            max_price_move_bps_per_slot: 500,
                            ..V16CuMarketParams::default()
                        },
                    );
                    let admin = env.admin.insecure_clone();
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
                    env.svm.warp_to_slot(1);
                    for asset in [0, 1] {
                        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
                    }
                    let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
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
                        .unwrap();
                        env.portfolios.push(key.pubkey());
                        key.pubkey()
                    });
                    let tokens = owners.each_ref().map(|owner| {
                        create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
                    });
                    let reserve = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        beneficiary.pubkey(),
                        env.mint,
                    );
                    let destination =
                        create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
                    for (token, amount) in tokens
                        .into_iter()
                        .zip(CAPITAL)
                        .chain([(reserve, SPENT), (destination, backing)])
                    {
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &token,
                                &admin.pubkey(),
                                &[],
                                amount,
                            )
                            .unwrap(),
                            &[&admin],
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
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                    for actor in 0..3 {
                        env.send(
                            env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
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
                        .unwrap();
                    }
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: side as u16,
                            market_id: env.asset_market_id(0),
                            authority_epoch: env.control_sequences(0).authority_epoch,
                            intent_id: 0,
                            amount: SPENT.into(),
                        },
                        vec![
                            AccountMeta::new(beneficiary.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(reserve, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&beneficiary],
                    )
                    .unwrap();
                    env.top_up_backing_bucket_from_admin_token_with_cu(
                        destination,
                        (2 + side) as u16,
                        backing.into(),
                        EXPIRY,
                    );
                    env.trade_asset_with_cu(
                        0,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        (10 * POS_SCALE) as i128 * if side == 0 { 1 } else { -1 },
                        100,
                        0,
                    );
                    for offset in 0..5 {
                        let slot = offset + 2;
                        let mark = if side == 0 {
                            100 + 5 * (offset + 1).min(4)
                        } else {
                            100 - 5 * (offset + 1).min(4)
                        };
                        env.svm.warp_to_slot(slot);
                        env.push_auth_mark_for_asset_as_admin(0, slot, mark);
                        env.crank(
                            portfolios[2],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        );
                    }
                    for actor in [0, 1] {
                        env.crank(
                            portfolios[actor],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 6,
                                observations: crank_observations(0),
                            },
                        );
                    }
                    assert_eq!(
                        env.portfolio_state(portfolios[0]).pnl.get(),
                        i128::from(GAIN)
                    );
                    assert_eq!(
                        env.market_state().1.assets[0].effective_price,
                        if side == 0 { 120 } else { 80 }
                    );
                    assert_eq!(
                        env.portfolio_state(portfolios[1]).pnl.get(),
                        -i128::from(SPENT)
                    );
                    env.svm.warp_to_slot(40);
                    env.resolve();
                    env.svm.warp_to_slot(43);
                    for actor in [1, 0, 2] {
                        for _ in 0..8 {
                            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                                break;
                            }
                            env.svm.expire_blockhash();
                            let cu = env
                                .send(
                                    ProgInstruction::CloseResolved {
                                        fee_rate_per_slot: 0,
                                    },
                                    vec![
                                        AccountMeta::new_readonly(owners[actor].pubkey(), false),
                                        AccountMeta::new(env.market, false),
                                        AccountMeta::new(portfolios[actor], false),
                                        AccountMeta::new(tokens[actor], false),
                                        AccountMeta::new(env.vault, false),
                                        AccountMeta::new_readonly(env.vault_authority, false),
                                        AccountMeta::new_readonly(spl_token::ID, false),
                                    ],
                                    &[],
                                )
                                .unwrap();
                            assert_cu_within("earlier-asset user payout", cu, LIMIT);
                            peak = peak.max(cu);
                        }
                        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
                        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
                        env.send(
                            env.close_portfolio_ix(portfolios[actor]),
                            vec![
                                AccountMeta::new(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[actor], false),
                            ],
                            &[&owners[actor]],
                        )
                        .unwrap();
                    }
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
                    let recovered = CAPITAL[1].min(SPENT).min(backing);
                    let partial = recovered / 3;
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
                    let mut tracked = vec![
                        env.market,
                        env.vault,
                        env.mint,
                        reserve,
                        destination,
                        admin.pubkey(),
                        beneficiary.pubkey(),
                    ];
                    tracked.extend(tokens);
                    tracked.extend(portfolios);
                    tracked.extend(owners.each_ref().map(Signer::pubkey));
                    drop(beneficiary);
                    let changed = [env.market, env.vault, reserve];
                    peak = peak.max(land(
                        &mut env,
                        &[close.clone()],
                        &[&admin],
                        &tracked,
                        &changed[..1],
                        None,
                        (1, 0),
                    ));
                    stocks(&env, side, backing, false, 0, 0, tokens, reserve);
                    let earlier = env.market_state().1.assets[0];
                    let slot_start = MARKET_GROUP_OFF
                        + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
                    let slot_end = slot_start
                        + std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
                    let earlier_bytes = env.svm.get_account(&env.market).unwrap().data
                        [slot_start..slot_end]
                        .to_vec();
                    let ledger = env.market_state().1.resolved_payout_ledger;
                    peak = peak.max(land(
                        &mut env,
                        &[first.clone()],
                        &[],
                        &tracked,
                        &[],
                        Some(2),
                        (0, 0),
                    ));
                    let before_clock = env.svm.get_account(&env.market);
                    env.svm.warp_to_slot(EXPIRY + u64::from(late));
                    assert_eq!(env.svm.get_account(&env.market), before_clock);
                    peak = peak.max(land(
                        &mut env,
                        &[first.clone()],
                        &[],
                        &tracked,
                        &[],
                        Some(2),
                        (0, 0),
                    ));

                    // Expiry, earlier-slot recredit and SPL payment all execute before the unpaid-claim suffix.
                    peak = peak.max(land(
                        &mut env,
                        &[close.clone(), first.clone(), close.clone()],
                        &[&admin],
                        &tracked,
                        &[],
                        Some(4),
                        (2, 1),
                    ));
                    stocks(&env, side, backing, false, 0, 0, tokens, reserve);
                    if bundled {
                        peak = peak.max(land(
                            &mut env,
                            &[close.clone(), first.clone()],
                            &[&admin],
                            &tracked,
                            &changed,
                            None,
                            (2, 1),
                        ));
                    } else {
                        peak = peak.max(land(
                            &mut env,
                            &[close.clone()],
                            &[&admin],
                            &tracked,
                            &changed[..1],
                            None,
                            (1, 0),
                        ));
                        stocks(&env, side, backing, true, 0, 0, tokens, reserve);
                        assert_eq!(&env.svm.get_account(&env.market).unwrap().data[slot_start..slot_end], earlier_bytes.as_slice(),
                            "later expiry changes earlier actionability without writing its scanned slot");
                        peak = peak.max(land(
                            &mut env,
                            &[first.clone(), close.clone()],
                            &[&admin],
                            &tracked,
                            &[],
                            Some(3),
                            (1, 1),
                        ));
                        peak = peak.max(land(
                            &mut env,
                            &[first],
                            &[],
                            &tracked,
                            &changed,
                            None,
                            (1, 1),
                        ));
                    }
                    stocks(
                        &env, side, backing, true, recovered, partial, tokens, reserve,
                    );
                    assert_eq!(env.market_state().1.assets[0], earlier);
                    let mut expected_ledger = ledger;
                    expected_ledger.snapshot_residual += u128::from(backing);
                    assert_eq!(env.market_state().1.resolved_payout_ledger, expected_ledger);
                    peak = peak.max(land(
                        &mut env,
                        &[tail],
                        &[],
                        &tracked,
                        &changed,
                        None,
                        (1, 1),
                    ));
                    stocks(
                        &env, side, backing, true, recovered, recovered, tokens, reserve,
                    );

                    let market = env.svm.get_account(&env.market).unwrap();
                    let vault = env.svm.get_account(&env.vault).unwrap();
                    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                    let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
                    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                    mint.supply -= backing - recovered;
                    Mint::pack(mint, &mut expected_mint.data).unwrap();
                    let closing = [env.market, env.vault, env.mint, admin.pubkey()];
                    peak = peak.max(land(
                        &mut env,
                        &[close],
                        &[&admin],
                        &tracked,
                        &closing,
                        None,
                        (1, 1 + usize::from(backing != recovered)),
                    ));
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
                        .map_or(true, |account| account.lamports == 0));
                    assert_eq!(env.token_amount(destination), 0);
                    println!("earlier-asset recredit: side={side} backing={backing} late={late} bundled={bundled} recovered={recovered} burned={}", backing - recovered);
                }
            }
        }
    }
    println!("INV-071 earlier-asset recredit: 16 public histories, peak={peak} CU");
}
