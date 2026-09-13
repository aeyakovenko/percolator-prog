//! Row 424: successful owner-local insurance withdrawals behind a persisted scan.
//! The mutations preserve residual and do not require cursor invalidation. No receipts,
//! pending losses, insurance spend/recredit, or injected economic state are used.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const INSURANCE: [u64; 2] = [37, 53];
const BACKING: u64 = 61;
const SUPPLY: u64 = 151;
const EXPIRY: u64 = 20;
const LIMIT: u64 = 300_000;

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
    successes: (usize, usize),
) -> u64 {
    env.svm.expire_blockhash();
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
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
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("the specified suffix must reject atomically");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, error)
        );
        assert!(allowed.is_empty());
        failure.meta
    } else {
        result.expect("public terminal continuation")
    };
    for (key, account) in keys.iter().zip(before) {
        if !allowed.contains(key) {
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
            count,
            "the intended wrapper and SPL prefixes must execute"
        );
    }
    assert_cu_within(
        "terminal prefix insurance",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn check(env: &V16CuEnv, tokens: [Pubkey; 2], paid: [bool; 2], domain: usize, expired: bool) {
    let (cfg, group) = env.market_state();
    assert_eq!(cfg.terminal_slab_scan_progress, 3);
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(
        (
            group.materialized_portfolio_count,
            group.c_tot,
            group.pnl_pos_tot
        ),
        (0, 0, 0)
    );
    let remaining: [u128; 2] =
        std::array::from_fn(|i| if paid[i] { 0 } else { INSURANCE[i].into() });
    let insurance = remaining.iter().sum::<u128>();
    assert_eq!(group.insurance, insurance);
    assert_eq!(group.vault, insurance + u128::from(BACKING));
    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
    for i in 0..2 {
        assert_eq!(
            env.token_amount(tokens[i]),
            if paid[i] { INSURANCE[i] } else { 0 }
        );
        assert_eq!(
            group.insurance_domain_budget[2 * (i + 1)]
                + group.insurance_domain_budget[2 * (i + 1) + 1],
            remaining[i]
        );
    }
    assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
    assert_eq!(
        group.insurance_domain_budget.iter().sum::<u128>(),
        insurance
    );
    let fresh = if expired {
        0
    } else {
        u128::from(BACKING) * BOUND_SCALE
    };
    assert_eq!(
        group.source_backing_buckets[domain].fresh_unliened_backing_num,
        fresh
    );
    assert_eq!(
        group.source_credit[domain].fresh_reserved_backing_num,
        fresh
    );
    assert_eq!(
        group.source_backing_buckets[domain].status,
        if expired {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert!(group.source_backing_buckets[..6]
        .iter()
        .all(|bucket| bucket.status != BackingBucketStatusV16::Fresh));
    assert!(group
        .source_credit
        .iter()
        .all(|source| source.positive_claim_bound_num == 0 && source.provider_receivable_num == 0));
    let market = env.svm.get_account(&env.market).unwrap();
    let header = market_group_header_bytes(&market.data);
    assert_eq!(
        header.insurance_domain_budget_remaining_total.get(),
        insurance
    );
    assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        SUPPLY
    );
    crate::support::fuzz_model::assert_market_stock_census(
        "prefix insurance",
        &group,
        &market.data,
        &[],
        group.vault,
    )
    .unwrap();
    crate::support::fuzz_model::assert_reservation_encumbrance_census(
        "prefix insurance",
        &group,
        &[],
    )
    .unwrap();
}

#[test]
fn v16_program_scanned_insurance_withdrawals_preserve_peer_entitlements_across_late_expiry() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    let mut peak = 0;
    for backing_side in 0..2 {
        for first in 0..2 {
            for late in [false, true] {
                let mut env =
                    inv018_public_spl_market_with_capacity(0, V16CuMarketParams::default(), 4);
                let admin = env.admin.insecure_clone();
                let owners = [Keypair::new(), Keypair::new()];
                for asset in 1..4 {
                    env.activate_asset(asset, u64::from(asset), 100);
                }
                let tokens = owners.each_ref().map(|owner| {
                    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
                });
                let destination =
                    create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
                for (i, owner) in owners.iter().enumerate() {
                    let asset = (i + 1) as u16;
                    env.send(
                        ProgInstruction::UpdateAssetAuthority {
                            asset_index: asset,
                            market_id: env.asset_market_id(asset),
                            authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                            kind: processor::ASSET_AUTH_INSURANCE,
                            new_pubkey: owner.pubkey().to_bytes(),
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new_readonly(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                        ],
                        &[&admin, owner],
                    )
                    .unwrap();
                }
                for (token, amount) in [
                    (tokens[0], INSURANCE[0]),
                    (tokens[1], INSURANCE[1]),
                    (destination, BACKING),
                ] {
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
                for i in 0..2 {
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: [2, 5][i],
                            market_id: env.asset_market_id((i + 1) as u16),
                            authority_epoch: 0,
                            intent_id: 0,
                            amount: INSURANCE[i].into(),
                        },
                        vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                }
                let domain = 6 + backing_side;
                env.top_up_backing_bucket_from_admin_token_with_cu(
                    destination,
                    domain as u16,
                    BACKING.into(),
                    EXPIRY,
                );
                env.svm.warp_to_slot(10);
                env.resolve();
                let close = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                    data: ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    }
                    .encode(),
                };
                let withdrawals: [Instruction; 2] = std::array::from_fn(|i| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[i].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env
                        .withdraw_insurance_asset_instruction(
                            owners[i].pubkey(),
                            (i + 1) as u16,
                            INSURANCE[i].into(),
                        )
                        .encode(),
                });
                let tracked = [
                    env.market,
                    env.mint,
                    env.vault,
                    destination,
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    admin.pubkey(),
                    solana_sdk::sysvar::clock::ID,
                ];
                let market = env.market;
                let vault = env.vault;
                peak = peak.max(land(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &tracked,
                    &[market],
                    None,
                    (1, 0),
                ));
                check(&env, tokens, [false; 2], domain, false);
                let asset_zero =
                    market_engine_slot_bytes(&env.svm.get_account(&market).unwrap().data, 0)
                        .to_vec();
                let second = 1 - first;
                let peer = market_engine_slot_bytes(
                    &env.svm.get_account(&market).unwrap().data,
                    second + 1,
                )
                .to_vec();

                // A real earlier-slot payout followed by a same-cursor wait is atomic.
                env.svm.warp_to_slot(EXPIRY - 1);
                let locked = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
                peak = peak.max(land(
                    &mut env,
                    &[withdrawals[first].clone(), close.clone()],
                    &[&owners[first], &admin],
                    &tracked,
                    &[],
                    Some((3, locked.clone())),
                    (1, 1),
                ));
                check(&env, tokens, [false; 2], domain, false);
                peak = peak.max(land(
                    &mut env,
                    &[withdrawals[first].clone()],
                    &[&owners[first]],
                    &tracked,
                    &[market, vault, tokens[first]],
                    None,
                    (1, 1),
                ));
                let mut paid = [false; 2];
                paid[first] = true;
                check(&env, tokens, paid, domain, false);
                assert_eq!(
                    market_engine_slot_bytes(
                        &env.svm.get_account(&market).unwrap().data,
                        second + 1
                    ),
                    peer
                );
                peak = peak.max(land(
                    &mut env,
                    &[withdrawals[first].clone()],
                    &[&owners[first]],
                    &tracked,
                    &[],
                    Some((2, locked.clone())),
                    (0, 0),
                ));
                let paid_slot = market_engine_slot_bytes(
                    &env.svm.get_account(&market).unwrap().data,
                    first + 1,
                )
                .to_vec();

                // Late reserve release cannot replenish the already withdrawn owner's allowance.
                env.svm.warp_to_slot(EXPIRY + u64::from(late));
                peak = peak.max(land(
                    &mut env,
                    &[close.clone(), withdrawals[first].clone()],
                    &[&admin, &owners[first]],
                    &tracked,
                    &[],
                    Some((3, locked.clone())),
                    (1, 0),
                ));
                check(&env, tokens, paid, domain, false);
                let invalid = Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                };
                peak = peak.max(land(
                    &mut env,
                    &[close.clone(), withdrawals[second].clone(), invalid],
                    &[&admin, &owners[second]],
                    &tracked,
                    &[],
                    Some((4, InstructionError::InvalidInstructionData)),
                    (2, 1),
                ));
                check(&env, tokens, paid, domain, false);
                peak = peak.max(land(
                    &mut env,
                    &[close.clone(), withdrawals[second].clone()],
                    &[&admin, &owners[second]],
                    &tracked,
                    &[market, vault, tokens[second]],
                    None,
                    (2, 1),
                ));
                check(&env, tokens, [true; 2], domain, true);
                assert_eq!(
                    market_engine_slot_bytes(
                        &env.svm.get_account(&market).unwrap().data,
                        first + 1,
                    ),
                    paid_slot,
                    "expiry and the peer's withdrawal preserve the earlier paid asset"
                );
                assert_eq!(env.market_state().1.current_slot, EXPIRY + u64::from(late));
                assert_eq!(
                    market_engine_slot_bytes(&env.svm.get_account(&market).unwrap().data, 0),
                    asset_zero
                );
                for i in 0..2 {
                    peak = peak.max(land(
                        &mut env,
                        &[withdrawals[i].clone()],
                        &[&owners[i]],
                        &tracked,
                        &[],
                        Some((2, locked.clone())),
                        (0, 0),
                    ));
                }

                let old_market = env.svm.get_account(&market).unwrap();
                let old_vault = env.svm.get_account(&vault).unwrap();
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
                let mint_key = env.mint;
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                mint.supply -= BACKING;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                peak = peak.max(land(
                    &mut env,
                    &[close],
                    &[&admin],
                    &tracked,
                    &[market, vault, mint_key, admin.pubkey()],
                    None,
                    (1, 2),
                ));
                let tombstone = env.svm.get_account(&market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(
                    tombstone.lamports,
                    env.svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
                );
                expected_admin.lamports +=
                    old_market.lamports + old_vault.lamports - tombstone.lamports;
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                assert_eq!(env.svm.get_account(&mint_key), Some(expected_mint));
                assert!(env
                    .svm
                    .get_account(&vault)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
                assert_eq!(tokens.map(|token| env.token_amount(token)), INSURANCE);
                assert_eq!(env.token_amount(destination), 0);
                println!("prefix insurance: side={backing_side} first={first} late={late}, cursor=0->3->tombstone; paid=37/53, burn=61");
            }
        }
    }
    println!("prefix insurance: 8 worlds, 48 exact rollbacks, 16 owner payouts, 8 slab retirements; peak {peak} CU");
}
