//! INV-070/073: reserve custody recreation and native surplus classification
//! compose with exact dual-vault retirement. INV-018/021/025/069/077/078/081
//! own token integrity, rent, normalization, bounded continuation and validity.
//! Row 418 additionally owns native-primary expired booked principal after a
//! secondary-rail payment, including rollback of the complete retirement prefix.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const LIMIT: u64 = 300_000;

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    payer_cost: u64,
    reject: bool,
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        signing.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = signing.len() as u64 * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let total_before: u64 = before
        .iter()
        .flatten()
        .map(|account| account.lamports)
        .sum();
    let result = env.svm.send_transaction(tx);
    let meta = if reject {
        let failure = result.expect_err("unsigned administrative suffix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (ixs.len() + 1) as u8,
                InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
            )
        );
        for program in [env.program_id, associated_token_program_id()] {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                ixs[..ixs.len() - 1]
                    .iter()
                    .filter(|ix| ix.program_id == program)
                    .count()
            );
        }
        failure.meta
    } else {
        result.expect("public residue continuation")
    };
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + if reject { 0 } else { payer_cost };
        } else if !reject && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame {key}"
        );
    }
    assert_eq!(
        keys.iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|account| account.lamports)
            .sum::<u64>(),
        total_before - fee
    );
    assert_cu_within(
        "native residue continuation",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn absent(env: &V16CuEnv, key: Pubkey) {
    assert!(env
        .svm
        .get_account(&key)
        .is_none_or(|account| account.lamports == 0
            && account.data.is_empty()
            && account.owner == solana_sdk::system_program::ID
            && !account.executable));
}

fn run_native_residue_disposition(native_booked: bool) {
    let mut worlds = 0;
    let mut peak = 0;
    let mut payments = 0;
    let mut repairs = 0;
    let mut normalizations = 0;
    for native_rail in 0..2 {
        for expire in [false, true] {
            if (expire && native_rail == 0) != native_booked {
                continue;
            }
            for raw in [0, 19] {
                for sync_at in 0..3 {
                    for missing in [false, true] {
                        if native_booked && (raw != 19 || sync_at != 2 || missing) {
                            continue;
                        }
                        let (mut env, admin, holders, rails) = reserve_world(native_rail);
                        let wallets = holders.each_ref().map(|holder| holder.pubkey());
                        env.resolve();
                        let market_frame = env.svm.get_account(&env.market).unwrap();
                        let sequences = env.control_sequences(0);
                        let profile =
                            state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
                        let tracked = [env.market, env.vault_authority, admin.pubkey()]
                            .into_iter()
                            .chain(wallets)
                            .chain(rails.iter().flat_map(|rail| {
                                [
                                    rail.mint,
                                    rail.vault,
                                    rail.recipients[0],
                                    rail.recipients[1],
                                    rail.admin_token,
                                ]
                            }))
                            .collect::<Vec<_>>();
                        let native = &rails[native_rail];
                        let rent = native.empty[0].lamports;
                        let mut close = Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(rails[0].vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new(rails[0].admin_token, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                                AccountMeta::new(rails[1].vault, false),
                                AccountMeta::new(rails[1].admin_token, false),
                                AccountMeta::new(rails[0].mint, false),
                            ],
                            data: ProgInstruction::CloseSlab {
                                authority_epoch: sequences.authority_epoch,
                            }
                            .encode(),
                        };
                        if native_booked {
                            close
                                .accounts
                                .push(AccountMeta::new(native.recipients[1], false));
                        }
                        let mut unsigned_close = close.clone();
                        unsigned_close.accounts[0].is_signer = false;
                        let sync =
                            spl_token::instruction::sync_native(&spl_token::ID, &native.vault)
                                .unwrap();
                        let donation =
                            system_instruction::transfer(&env.payer.pubkey(), &native.vault, raw);
                        peak = peak.max(land(
                            &mut env,
                            &[donation],
                            &[],
                            &tracked,
                            &[native.vault],
                            raw,
                            false,
                        ));
                        let mut synced = false;
                        if sync_at == 1 {
                            peak = peak.max(land(
                                &mut env,
                                &[sync.clone()],
                                &[],
                                &tracked,
                                &[native.vault],
                                0,
                                false,
                            ));
                            synced = true;
                        }
                        let payout = |env: &V16CuEnv, kind: usize, rail: usize, amount: u64| {
                            let recipient = usize::from(kind == 2);
                            let instruction = if kind == 2 {
                                ProgInstruction::WithdrawInsuranceAsset {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    authority_epoch: env.control_sequences(0).authority_epoch,
                                    amount: amount.into(),
                                }
                            } else {
                                ProgInstruction::WithdrawBackingBucket {
                                    domain: kind as u16,
                                    market_id: env.asset_market_id(0),
                                    authority_epoch: env.control_sequences(0).authority_epoch,
                                    amount: amount.into(),
                                }
                            };
                            Instruction {
                                program_id: env.program_id,
                                accounts: vec![
                                    AccountMeta::new_readonly(wallets[recipient], false),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(rails[rail].recipients[recipient], false),
                                    AccountMeta::new(rails[rail].vault, false),
                                    AccountMeta::new_readonly(env.vault_authority, false),
                                    AccountMeta::new_readonly(spl_token::ID, false),
                                ],
                                data: instruction.encode(),
                            }
                        };
                        let check = |env: &V16CuEnv,
                                     paid: [[u64; 2]; 3],
                                     redeemed: [u64; 2],
                                     present: [bool; 2],
                                     insurance_debits: u64,
                                     expired: bool,
                                     synced: bool| {
                            let paid_by_rail: [u64; 2] = std::array::from_fn(|rail| {
                                paid.iter().map(|kind| kind[rail]).sum()
                            });
                            let remaining: [u64; 3] = std::array::from_fn(|kind| {
                                CLAIMS[kind] - paid[kind].iter().sum::<u64>()
                            });
                            let physical = [FUNDED - paid_by_rail[0], SECONDARY - paid_by_rail[1]];
                            let logical = remaining.iter().sum::<u64>();
                            let booked_residue = if expired { remaining[1] } else { 0 };
                            let claim_rank = logical - booked_residue;
                            assert_eq!(
                                physical.iter().sum::<u64>(),
                                claim_rank + booked_residue + SECONDARY
                            );
                            let market = env.svm.get_account(&env.market).unwrap();
                            let group = env.market_state().1;
                            assert_eq!(group.mode, MarketModeV16::Resolved);
                            assert_eq!(group.vault, u128::from(logical));
                            assert_eq!(
                                (
                                    group.c_tot,
                                    group.pnl_pos_tot,
                                    group.materialized_portfolio_count
                                ),
                                (0, 0, 0)
                            );
                            assert_eq!(group.source_claim_bound_total_num, 0);
                            assert_eq!(group.backing_provider_earnings_total, 0);
                            assert_eq!(group.insurance, u128::from(remaining[2]));
                            assert_eq!(
                                group.insurance_domain_budget_remaining_total,
                                group.insurance
                            );
                            assert_eq!(group.insurance_domain_budget[0], group.insurance);
                            assert!(group.insurance_domain_budget[1..]
                                .iter()
                                .all(|amount| *amount == 0));
                            assert!(group
                                .insurance_domain_spent
                                .iter()
                                .all(|amount| *amount == 0));
                            for domain in 0..2 {
                                let principal = if expired && domain == 1 {
                                    0
                                } else {
                                    remaining[domain]
                                };
                                let bucket = group.source_backing_buckets[domain];
                                assert_eq!(
                                    bucket.fresh_unliened_backing_num,
                                    u128::from(principal) * BOUND_SCALE
                                );
                                assert_eq!(
                                    group.source_credit[domain].fresh_reserved_backing_num,
                                    u128::from(principal) * BOUND_SCALE
                                );
                                assert_eq!(
                                    (
                                        bucket.valid_liened_backing_num,
                                        bucket.consumed_liened_backing_num
                                    ),
                                    (0, 0)
                                );
                            }
                            assert_eq!(physical[0], logical + paid_by_rail[1]);
                            for (rail, custody) in rails.iter().enumerate() {
                                assert_eq!(
                                    env.svm.get_account(&custody.mint),
                                    Some(custody.mint_frame.clone())
                                );
                                let amounts = [
                                    physical[rail],
                                    paid[0][rail] + paid[1][rail],
                                    paid[2][rail],
                                    0,
                                ];
                                for (index, key) in [
                                    custody.vault,
                                    custody.recipients[0],
                                    custody.recipients[1],
                                    custody.admin_token,
                                ]
                                .into_iter()
                                .enumerate()
                                {
                                    let is_recipient = index == 1 || index == 2;
                                    if rail == native_rail && is_recipient && !present[index - 1] {
                                        absent(env, key);
                                        continue;
                                    }
                                    let amount = amounts[index]
                                        - if rail == native_rail && is_recipient {
                                            redeemed[index - 1]
                                        } else {
                                            0
                                        };
                                    let mut expected = token_image(&custody.empty[index], amount);
                                    if rail == native_rail && index == 0 {
                                        expected.lamports += raw;
                                        if synced {
                                            let mut token =
                                                TokenAccount::unpack(&expected.data).unwrap();
                                            token.amount += raw;
                                            TokenAccount::pack(token, &mut expected.data).unwrap();
                                        }
                                    }
                                    assert_eq!(
                                        env.svm.get_account(&key),
                                        Some(expected),
                                        "rail {rail} custody {index}"
                                    );
                                }
                            }
                            assert_eq!(market.lamports, market_frame.lamports);
                            let mut expected_sequences = sequences;
                            expected_sequences.authority_epoch += insurance_debits;
                            assert_eq!(env.control_sequences(0), expected_sequences);
                            assert_eq!(
                                state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                                profile
                            );
                            assert_market_stock_census(
                                "native residue claims",
                                &group,
                                &market.data,
                                &[],
                                logical.into(),
                            )
                            .unwrap();
                            assert_reservation_encumbrance_census(
                                "native residue claims",
                                &group,
                                &[],
                            )
                            .unwrap();
                            state::market_view_mut(&mut market.data.clone())
                                .unwrap()
                                .1
                                .validate_shape()
                                .unwrap();
                            claim_rank
                        };
                        let mut paid = [[0; 2]; 3];
                        let mut redeemed = [0; 2];
                        let mut present = [true; 2];
                        let mut insurance_debits = 0;
                        let mut rank = check(
                            &env,
                            paid,
                            redeemed,
                            present,
                            insurance_debits,
                            false,
                            synced,
                        );
                        for kind in 0..3 {
                            let ix = payout(&env, kind, native_rail, PREFIX[kind]);
                            let recipient = usize::from(kind == 2);
                            let changed = [env.market, native.vault, native.recipients[recipient]];
                            peak =
                                peak.max(land(&mut env, &[ix], &[], &tracked, &changed, 0, false));
                            paid[kind][native_rail] += PREFIX[kind];
                            insurance_debits += u64::from(kind == 2);
                            payments += 1;
                            let next_rank = check(
                                &env,
                                paid,
                                redeemed,
                                present,
                                insurance_debits,
                                false,
                                synced,
                            );
                            assert_eq!(rank - next_rank, PREFIX[kind]);
                            rank = next_rank;
                        }
                        if missing {
                            redeemed = [PREFIX[0] + PREFIX[1], PREFIX[2]];
                            for recipient in 0..2 {
                                let before =
                                    env.svm.get_account(&wallets[recipient]).unwrap().lamports;
                                let ix = spl_token::instruction::close_account(
                                    &spl_token::ID,
                                    &native.recipients[recipient],
                                    &wallets[recipient],
                                    &wallets[recipient],
                                    &[],
                                )
                                .unwrap();
                                peak = peak.max(land(
                                    &mut env,
                                    &[ix],
                                    &[&holders[recipient]],
                                    &tracked,
                                    &[native.recipients[recipient], wallets[recipient]],
                                    0,
                                    false,
                                ));
                                assert_eq!(
                                    env.svm.get_account(&wallets[recipient]).unwrap().lamports,
                                    before + rent + redeemed[recipient]
                                );
                                present[recipient] = false;
                            }
                            for holder in &holders {
                                let balance =
                                    env.svm.get_account(&holder.pubkey()).unwrap().lamports;
                                let ix = system_instruction::transfer(
                                    &holder.pubkey(),
                                    &admin.pubkey(),
                                    balance,
                                );
                                peak = peak.max(land(
                                    &mut env,
                                    &[ix],
                                    &[holder],
                                    &tracked,
                                    &[holder.pubkey(), admin.pubkey()],
                                    0,
                                    false,
                                ));
                                absent(&env, holder.pubkey());
                            }
                        }
                        drop(holders);
                        let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
                        check(
                            &env,
                            paid,
                            redeemed,
                            present,
                            insurance_debits,
                            false,
                            synced,
                        );
                        let mut expired = false;
                        for kind in [0, 1, 2] {
                            if expire && kind == 1 {
                                env.svm.warp_to_slot(100);
                                let changed = [env.market];
                                let mut close = close.clone();
                                close.data = ProgInstruction::CloseSlab {
                                    authority_epoch: env.control_sequences(0).authority_epoch,
                                }
                                .encode();
                                peak = peak.max(land(
                                    &mut env,
                                    &[close],
                                    &[&admin],
                                    &tracked,
                                    &changed,
                                    0,
                                    false,
                                ));
                                expired = true;
                                normalizations += 1;
                                let next_rank = check(
                                    &env,
                                    paid,
                                    redeemed,
                                    present,
                                    insurance_debits,
                                    expired,
                                    synced,
                                );
                                assert_eq!(rank - next_rank, CLAIMS[1] - PREFIX[1]);
                                rank = next_rank;
                                continue;
                            }
                            let rail = if kind == 1 || (native_booked && kind == 0) {
                                1 - native_rail
                            } else {
                                native_rail
                            };
                            let recipient = usize::from(kind == 2);
                            let repair = rail == native_rail && !present[recipient];
                            let mut batch = Vec::new();
                            if repair {
                                batch.push(Instruction {
                                    program_id: associated_token_program_id(),
                                    accounts: vec![
                                        AccountMeta::new(env.payer.pubkey(), true),
                                        AccountMeta::new(native.recipients[recipient], false),
                                        AccountMeta::new_readonly(wallets[recipient], false),
                                        AccountMeta::new_readonly(native.mint, false),
                                        AccountMeta::new_readonly(
                                            solana_sdk::system_program::ID,
                                            false,
                                        ),
                                        AccountMeta::new_readonly(spl_token::ID, false),
                                    ],
                                    data: vec![1],
                                });
                            }
                            batch.push(payout(&env, kind, rail, CLAIMS[kind] - PREFIX[kind]));
                            let mut rejected = batch.clone();
                            rejected.push(unsigned_close.clone());
                            peak = peak.max(land(&mut env, &rejected, &[], &tracked, &[], 0, true));
                            check(
                                &env,
                                paid,
                                redeemed,
                                present,
                                insurance_debits,
                                expired,
                                synced,
                            );
                            let changed = [
                                env.market,
                                rails[rail].vault,
                                rails[rail].recipients[recipient],
                            ];
                            peak = peak.max(land(
                                &mut env,
                                &batch,
                                &[],
                                &tracked,
                                &changed,
                                if repair { rent } else { 0 },
                                false,
                            ));
                            present[recipient] = true;
                            repairs += usize::from(repair);
                            paid[kind][rail] += CLAIMS[kind] - PREFIX[kind];
                            insurance_debits += u64::from(kind == 2);
                            payments += 1;
                            let next_rank = check(
                                &env,
                                paid,
                                redeemed,
                                present,
                                insurance_debits,
                                expired,
                                synced,
                            );
                            assert_eq!(rank - next_rank, CLAIMS[kind] - PREFIX[kind]);
                            rank = next_rank;
                        }
                        if sync_at == 2 {
                            peak = peak.max(land(
                                &mut env,
                                &[sync],
                                &[],
                                &tracked,
                                &[native.vault],
                                0,
                                false,
                            ));
                            synced = true;
                            check(
                                &env,
                                paid,
                                redeemed,
                                present,
                                insurance_debits,
                                expired,
                                synced,
                            );
                        }
                        let retired = if expire { CLAIMS[1] - PREFIX[1] } else { 0 };
                        assert_eq!(rank, 0);
                        assert_eq!(env.market_state().1.vault, u128::from(retired));
                        assert_eq!(env.market_state().1.insurance, 0);
                        let paid_by_rail: [u64; 2] =
                            std::array::from_fn(|rail| paid.iter().map(|kind| kind[rail]).sum());
                        let mut sweep = [
                            FUNDED - paid_by_rail[0] - retired,
                            SECONDARY - paid_by_rail[1],
                        ];
                        sweep[native_rail] += if synced { raw } else { 0 };
                        let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
                        let tombstone_rent = env.svm.minimum_balance_for_rent_exemption(
                            percolator_prog::constants::HEADER_LEN,
                        );
                        let refund = market_frame.lamports - tombstone_rent
                            + rails.iter().map(|rail| rail.empty[0].lamports).sum::<u64>()
                            + if synced { 0 } else { raw };
                        let mut changed = vec![
                            env.market,
                            admin.pubkey(),
                            rails[0].vault,
                            rails[1].vault,
                            rails[0].admin_token,
                            rails[1].admin_token,
                            rails[0].mint,
                        ];
                        if native_booked {
                            changed.push(native.recipients[1]);
                        }
                        let mut close = close;
                        close.data = ProgInstruction::CloseSlab {
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        }
                        .encode();
                        if native_booked {
                            assert_eq!((retired, sweep), (248, [319, 697]));
                            let mut denied = unsigned_close.clone();
                            denied.accounts[0] = AccountMeta::new(wallets[1], false);
                            peak = peak.max(land(
                                &mut env,
                                &[close.clone(), denied],
                                &[&admin],
                                &tracked,
                                &[],
                                0,
                                true,
                            ));
                            check(
                                &env,
                                paid,
                                redeemed,
                                present,
                                insurance_debits,
                                expired,
                                synced,
                            );
                        }
                        peak = peak.max(land(
                            &mut env,
                            &[close],
                            &[&admin],
                            &tracked,
                            &changed,
                            0,
                            false,
                        ));
                        let tombstone = env.svm.get_account(&env.market).unwrap();
                        assert_closed_market_tombstone(&tombstone);
                        assert_eq!(tombstone.lamports, tombstone_rent);
                        let mut expected_admin = admin_before;
                        expected_admin.lamports += refund;
                        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                        if native_booked {
                            assert_eq!(
                                env.svm.get_account(&native.recipients[1]),
                                Some(token_image(&native.empty[2], CLAIMS[2] + retired))
                            );
                        }
                        for (rail, custody) in rails.iter().enumerate() {
                            absent(&env, custody.vault);
                            assert_eq!(
                                env.svm.get_account(&custody.admin_token),
                                Some(token_image(&custody.empty[3], sweep[rail]))
                            );
                            let mut expected_mint = custody.mint_frame.clone();
                            if rail == 0 && retired != 0 && !native_booked {
                                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                                mint.supply -= retired;
                                Mint::pack(mint, &mut expected_mint.data).unwrap();
                            }
                            assert_eq!(env.svm.get_account(&custody.mint), Some(expected_mint));
                            let custody_total = custody
                                .recipients
                                .iter()
                                .map(|key| env.token_amount(*key))
                                .sum::<u64>()
                                + env.token_amount(custody.admin_token);
                            assert_eq!(
                                custody_total
                                    + if rail == native_rail {
                                        redeemed.iter().sum::<u64>()
                                    } else {
                                        0
                                    }
                                    + if rail == 0 && !native_booked {
                                        retired
                                    } else {
                                        0
                                    },
                                if rail == 0 { FUNDED } else { SECONDARY }
                                    + if rail == native_rail && synced {
                                        raw
                                    } else {
                                        0
                                    }
                            );
                        }
                        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
                        // Redeem the administrator's native sweep. Unsynced raw SOL
                        // already arrived in the vault-close refund above.
                        let before = env.svm.get_account(&admin.pubkey()).unwrap().lamports;
                        let redeem = spl_token::instruction::close_account(
                            &spl_token::ID,
                            &native.admin_token,
                            &admin.pubkey(),
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap();
                        peak = peak.max(land(
                            &mut env,
                            &[redeem],
                            &[&admin],
                            &tracked,
                            &[native.admin_token, admin.pubkey()],
                            0,
                            false,
                        ));
                        absent(&env, native.admin_token);
                        assert_eq!(
                            env.svm.get_account(&admin.pubkey()).unwrap().lamports,
                            before + rent + sweep[native_rail]
                        );
                        assert_eq!(
                            refund + sweep[native_rail],
                            market_frame.lamports - tombstone_rent
                                + rails.iter().map(|rail| rail.empty[0].lamports).sum::<u64>()
                                + if native_rail == 0 {
                                    FUNDED - paid_by_rail[0] - retired
                                } else {
                                    SECONDARY - paid_by_rail[1]
                                }
                                + raw
                        );
                        assert_eq!(env.svm.get_account(&env.market), Some(tombstone));
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(
        (worlds, payments, repairs, normalizations),
        if native_booked {
            (1, 5, 0, 1)
        } else {
            (36, 204, 36, 12)
        }
    );
    eprintln!("native residue disposition: native_booked={native_booked}, worlds={worlds}, payments={payments}, repairs={repairs}, normalizations={normalizations}, retirements={worlds}, rollbacks={}, peak={peak} CU", payments - worlds * 3 + usize::from(native_booked));
}

#[test]
fn v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement() {
    run_native_residue_disposition(false);
}

#[test]
fn v16_program_native_primary_booked_residue_survives_dual_quote_retirement_retry() {
    run_native_residue_disposition(true);
}
