//! INV-012/002/004/007/019/024/081/089: matcher synchronization is context-local.
//! A former LP signs as taker through its peer's context of the same program.
//! Returning to flat and recycling a previously traded asset cannot revive the
//! former LP's grant. Generation, episode and fresh grant remain separate gates.

use super::*;
use percolator_prog::matcher_abi::read_matcher_return;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

type Matcher = (Pubkey, Pubkey, Pubkey);

fn trade(h: &History, batch: bool, taker: usize, asset: u16, size: i128) -> ProgInstruction {
    let a = h.portfolios[taker];
    let b = h.portfolios[1 - taker];
    if batch {
        h.env.batch_trade_cpi_ix_with_caps(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: asset,
                market_id: h.ids[asset as usize],
                size_q: size,
                fee_bps: 0,
                limit_price: PRICE,
            }],
            0,
            0,
        )
    } else {
        h.env.trade_cpi_ix(a, b, asset, size, 0, PRICE)
    }
}

fn sign(
    h: &History,
    matcher: Matcher,
    taker: usize,
    ix: &ProgInstruction,
    lane: u32,
) -> Transaction {
    // Distinct budgets distinguish failed deliveries without changing signed economic fields.
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(750_000 - lane),
            Instruction {
                program_id: h.env.program_id,
                accounts: vec![
                    AccountMeta::new(h.owners[taker].pubkey(), true),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(h.portfolios[taker], false),
                    AccountMeta::new(h.portfolios[1 - taker], false),
                    AccountMeta::new_readonly(matcher.0, false),
                    AccountMeta::new(matcher.1, false),
                    AccountMeta::new_readonly(matcher.2, false),
                ],
                data: ix.encode(),
            },
        ],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.owners[taker]],
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        tx.signatures.len(),
        2,
        "only payer and the acting taker sign"
    );
    assert!(!tx.message.account_keys[..2].contains(&h.owners[1 - taker].pubkey()));
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn check(h: &History, matchers: [Matcher; 2], sequences: [u64; 2], enabled: [bool; 2]) {
    let group = h.env.market_state().1;
    assert_eq!(group.next_market_id, h.next_id);
    for asset in 0..3 {
        assert_eq!(group.assets[asset].market_id, h.ids[asset]);
        assert_eq!(
            group.assets[asset].oi_eff_long_q,
            h.positions[asset].unsigned_abs()
        );
        assert_eq!(
            group.assets[asset].oi_eff_short_q,
            h.positions[asset].unsigned_abs()
        );
    }
    for actor in 0..2 {
        let p = h.portfolios[actor];
        let state = h.env.portfolio_state(p);
        assert_eq!(state.capital.get(), CAPITAL);
        assert_eq!(state.pnl.get(), 0);
        assert_eq!(h.env.portfolio_position_epoch(p), h.epoch);
        for asset in 0..3 {
            assert_eq!(
                has_active_leg_for_asset(&state, asset),
                h.positions[asset] != 0
            );
            if h.positions[asset] != 0 {
                let leg = active_leg_for_asset(&state, asset);
                assert_eq!(leg.market_id, h.ids[asset]);
                assert_eq!(
                    leg.basis_pos_q,
                    h.positions[asset] * if actor == 0 { 1 } else { -1 }
                );
            }
        }
        let config = h.env.portfolio_matcher_config(p);
        assert_eq!(config.enabled(), u64::from(enabled[actor]));
        assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
        assert_eq!(config.matcher_program, matchers[actor].0.to_bytes());
        assert_eq!(config.matcher_context, matchers[actor].1.to_bytes());
        assert_eq!(config.matcher_delegate, matchers[actor].2.to_bytes());
        assert_eq!(h.env.portfolio_matcher_sequence(p), sequences[actor]);
        assert_eq!(
            h.env.portfolio_matcher_expiry(p),
            if enabled[actor] { EXPIRY } else { 0 }
        );
        assert_eq!(h.env.token_amount(h.tokens[actor]), 0);
    }
    assert_eq!(group.c_tot, 2 * CAPITAL);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 2 * CAPITAL);
    assert_eq!(h.env.token_amount(h.env.vault) as u128, 2 * CAPITAL);
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
    assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, h.slot);
    assert!(h.slot < EXPIRY);
}

fn recycle(h: &mut History, target: u16, evidence: &mut Evidence) {
    assert_eq!(h.positions, [0; 3]);
    let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
    for activate in [false, true] {
        if activate {
            h.slot += 1;
            h.env.svm.warp_to_slot(h.slot);
        }
        let authority_epoch = h.env.control_sequences(0).authority_epoch;
        let cu = send_tx(
            &mut h.env.svm,
            h.env.program_id,
            &h.env.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: if activate {
                    processor::ASSET_ACTION_ACTIVATE
                } else {
                    processor::ASSET_ACTION_RETIRE
                },
                asset_index: target,
                market_id: if activate {
                    h.next_id
                } else {
                    h.ids[target as usize]
                },
                authority_epoch,
                now_slot: h.slot,
                initial_price: if activate { PRICE } else { 0 },
                max_init_fee: 0,
                insurance_authority: h.env.admin.pubkey().to_bytes(),
                insurance_operator: h.env.admin.pubkey().to_bytes(),
                backing_bucket_authority: h.env.admin.pubkey().to_bytes(),
                oracle_authority: h.env.admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(h.env.admin.pubkey(), true),
                AccountMeta::new(h.env.market, false),
            ],
            &[&h.env.admin],
        )
        .expect("public retirement/reactivation of the previously traded flat asset");
        evidence.writer_cu = evidence.writer_cu.max(cu);
        if activate {
            h.ids[target as usize] = h.next_id;
            h.next_id += 1;
        }
        assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), portfolios);
    }
    for asset in 0..3 {
        let cu = h
            .env
            .configure_auth_mark_for_asset_as_admin(asset, h.slot, PRICE);
        evidence.writer_cu = evidence.writer_cu.max(cu);
    }
}

fn reject(
    h: &mut History,
    matchers: [Matcher; 2],
    tx: Transaction,
    error: PercolatorError,
    evidence: &mut Evidence,
) {
    let keys = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.mint,
            h.env.vault,
            h.tokens[0],
            h.tokens[1],
            h.owners[0].pubkey(),
            h.owners[1].pubkey(),
            h.env.admin.pubkey(),
            matchers[0].1,
            matchers[0].2,
            matchers[1].1,
            matchers[1].2,
        ])
        .collect::<std::collections::BTreeSet<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let mut before = keys
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let failed = h
        .env
        .svm
        .send_transaction(tx)
        .expect_err("obsolete or revoked consent rejects before matcher CPI");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(2, InstructionError::Custom(error as u32))
    );
    assert!(!failed
        .meta
        .logs
        .iter()
        .any(|line| line.starts_with(&format!("Program {} invoke", h.matcher.0))));
    for (key, account) in &mut before {
        if *key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            h.env.svm.get_account(key),
            *account,
            "exact Account rollback: {key}"
        );
    }
    evidence.rejections += 1;
    evidence.rejection_cu = evidence
        .rejection_cu
        .max(failed.meta.compute_units_consumed);
}

#[test]
fn v16_program_same_program_role_switch_revocation_survives_used_asset_reuse() {
    let mut evidence = Evidence::default();
    for target in [1u16, 2] {
        for batch in [false, true] {
            for reuse_first in [false, true] {
                for direction in [-1i128, 1] {
                    let mut h = History::new();
                    let route = if batch {
                        Route::Batch
                    } else {
                        Route::Single(target)
                    };
                    let size = direction * POS_SCALE as i128;
                    let mut sizes = [0; 3];
                    sizes[target as usize] = size;
                    for delta in [sizes, sizes.map(|q| -q)] {
                        h.fill(Route::Single(target), delta, false, &mut evidence);
                    }
                    // The original context contains an actual response for the old generation.
                    let original_context = h.env.svm.get_account(&h.matcher.1).unwrap();
                    let old_return = read_matcher_return(&original_context.data).unwrap();
                    assert_eq!(old_return.req_id, 2);
                    let (context, delegate, _) = h.env.init_auth_matcher_context_via_system_create(
                        h.matcher.0,
                        &h.owners[0],
                        h.portfolios[0],
                    );
                    h.env
                        .try_set_matcher_config_with_trade_fee_cap_and_expiry(
                            h.matcher.0,
                            &h.owners[0],
                            h.portfolios[0],
                            context,
                            delegate,
                            1,
                            FEE_CAP,
                            EXPIRY,
                        )
                        .expect("peer authorizes its own context of the identical program");
                    let matchers = [(h.matcher.0, context, delegate), h.matcher];
                    assert_ne!(matchers[0].1, matchers[1].1);
                    assert_ne!(matchers[0].2, matchers[1].2);
                    let mut sequences = [3, 3]; // Deposit and two explicit owner grants each.
                    let identities = h.portfolios.map(|p| h.env.portfolio_id(p));
                    check(&h, matchers, sequences, [true, true]);
                    let original = trade(&h, batch, 0, target, size);
                    let retained = sign(&h, matchers[1], 0, &original, 0);
                    let retained_bytes = bincode::serialize(&retained).unwrap();
                    h.simulate(&retained, 0, &mut evidence);
                    let old_generation = h.ids[target as usize];

                    if reuse_first {
                        recycle(&mut h, target, &mut evidence);
                        check(&h, matchers, sequences, [true, true]);
                    }
                    let sibling = 3 - target;
                    for delta in [size, -size] {
                        let ix = trade(&h, !batch, 1, sibling, delta);
                        let tx = sign(&h, matchers[0], 1, &ix, 1);
                        let meta = h
                            .env
                            .svm
                            .send_transaction(tx)
                            .expect("former LP signs as taker through peer context");
                        assert!(meta
                            .logs
                            .iter()
                            .any(|line| line == &format!("Program {} success", h.matcher.0)));
                        h.positions[sibling as usize] -= delta;
                        h.epoch += 1;
                        evidence.fills += 1;
                        evidence.fill_cu = evidence.fill_cu.max(meta.compute_units_consumed);
                        check(&h, matchers, sequences, [true, false]);
                        assert_eq!(
                            h.env.svm.get_account(&h.matcher.1).unwrap(),
                            original_context,
                            "same program invocation did not synchronize the former LP's context"
                        );
                    }
                    if !reuse_first {
                        recycle(&mut h, target, &mut evidence);
                    }
                    check(&h, matchers, sequences, [true, false]);
                    assert_eq!(h.ids[target as usize], 4);
                    assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), identities);
                    assert_eq!(h.epoch, 4);
                    assert_eq!(h.positions, [0; 3]);
                    assert_eq!(
                        h.env.svm.latest_blockhash(),
                        retained.message.recent_blockhash
                    );
                    assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                    reject(
                        &mut h,
                        matchers,
                        retained,
                        PercolatorError::EngineStale,
                        &mut evidence,
                    );

                    // Repairing all request identities cannot substitute for owner authorization.
                    let current = trade(&h, batch, 0, target, size);
                    let denied = sign(&h, matchers[1], 0, &current, 2);
                    reject(
                        &mut h,
                        matchers,
                        denied,
                        PercolatorError::Unauthorized,
                        &mut evidence,
                    );
                    check(&h, matchers, sequences, [true, false]);
                    let peer = trade(&h, !batch, 1, sibling, size);
                    let peer_tx = sign(&h, matchers[0], 1, &peer, 3);
                    let before = h.frame();
                    let peer_before = h.env.svm.get_account(&context);
                    let meta = h
                        .env
                        .svm
                        .simulate_transaction(peer_tx.into())
                        .expect("synchronized peer capability remains usable");
                    assert!(meta
                        .logs
                        .iter()
                        .any(|line| line == &format!("Program {} success", h.matcher.0)));
                    assert_eq!(h.frame(), before);
                    assert_eq!(h.env.svm.get_account(&context), peer_before);
                    evidence.live_simulations += 1;

                    h.replace(GRANT, &mut evidence);
                    sequences[1] += 1;
                    check(&h, matchers, sequences, [true, true]);
                    let old_grant = sign(&h, matchers[1], 0, &current, 4);
                    reject(
                        &mut h,
                        matchers,
                        old_grant,
                        PercolatorError::EngineStale,
                        &mut evidence,
                    );
                    let mut old_asset = trade(&h, batch, 0, target, size);
                    match &mut old_asset {
                        ProgInstruction::TradeCpi { market_id, .. } => *market_id = old_generation,
                        ProgInstruction::BatchTradeCpi { legs, .. } => {
                            legs[0].market_id = old_generation
                        }
                        _ => unreachable!(),
                    }
                    let old_asset = sign(&h, matchers[1], 0, &old_asset, 5);
                    reject(
                        &mut h,
                        matchers,
                        old_asset,
                        PercolatorError::AssetGenerationMismatch,
                        &mut evidence,
                    );
                    check(&h, matchers, sequences, [true, true]);

                    h.fill(route, sizes, false, &mut evidence);
                    check(&h, matchers, sequences, [false, true]);
                    let exit = if batch {
                        Route::Single(target)
                    } else {
                        Route::Batch
                    };
                    h.fill(exit, sizes.map(|q| -q), false, &mut evidence);
                    check(&h, matchers, sequences, [false, true]);
                    assert_eq!(h.epoch, 6);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, 6);
                    h.withdraw_all(reuse_first, &mut evidence);
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(evidence.worlds, 16);
    assert_eq!(evidence.rejections, 64);
    assert_eq!(evidence.fills, 96);
    assert_eq!(evidence.live_simulations, 32);
    assert!(evidence.fill_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    assert!(evidence.rejection_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    assert!(evidence.writer_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    assert!(evidence.custody_cu <= CUSTODY_CU_LIMIT);
    println!("same-program role switch and used-generation evidence: {evidence:?}");
}
