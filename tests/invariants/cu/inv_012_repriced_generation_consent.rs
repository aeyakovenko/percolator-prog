//! Row 414 / INV-012 (+ INV-002/007/019/089): retained batch permission across
//! used-slot replacement must bind both the generation and its economic bounds.
//! A repriced replacement doubles the rounded two-leg fee. The unchanged LP grant
//! cannot make an old-generation request current or enlarge its signed atom cap.
//! This is bounded request conformance, not standing-grant generation confinement.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const REPLACEMENT_PRICE: u64 = 2 * PRICE + 1;

fn fee(size: i128, price: u64) -> u128 {
    let notional = (size.unsigned_abs() * u128::from(price)).div_ceil(POS_SCALE);
    (notional * u128::from(FEE_CAP)).div_ceil(10_000)
}

fn batch(h: &History, sizes: [i128; 3], reverse: bool, cap: u128) -> ProgInstruction {
    let mut ix = h.instruction(Route::Batch, sizes, reverse);
    match &mut ix {
        ProgInstruction::BatchTradeCpi {
            max_fee_atoms,
            legs,
            ..
        } => {
            *max_fee_atoms = cap;
            for leg in legs {
                leg.fee_bps = u64::from(FEE_CAP);
                // Both price directions are admissible; only generation/fee consent changes.
                leg.limit_price = 0;
            }
        }
        _ => unreachable!(),
    }
    ix
}

fn reject(h: &mut History, tx: Transaction, after_cpi: bool) -> u64 {
    tx.verify().unwrap();
    let before = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]])
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let network_fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let frame = h.frame();
    let failure = h.env.svm.send_transaction(tx).expect_err(
        "retained authority needs the replacement generation and sufficient signed fee consent",
    );
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(if after_cpi {
                PercolatorError::InvalidInstruction as u32
            } else {
                PercolatorError::AssetGenerationMismatch as u32
            })
        )
    );
    for log in [
        format!("Program {} invoke [2]", h.matcher.0),
        format!("Program {} success", h.matcher.0),
    ] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == log)
                .count(),
            usize::from(after_cpi),
            "generation rejection precedes CPI; fee rejection follows real matcher execution"
        );
    }
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= network_fee;
        }
        assert_eq!(h.env.svm.get_account(&key), account, "rollback: {key}");
    }
    assert_eq!(h.frame(), frame);
    failure.meta.compute_units_consumed
}

fn check(h: &History, fees: [u128; 3], paid: [u128; 2], requests: u64) {
    let per_owner_fee = fees.iter().sum::<u128>();
    let (cfg, group) = h.env.market_state();
    let grant = h.env.portfolio_matcher_config(h.portfolios[1]);
    assert_eq!(grant.enabled(), 1);
    assert_eq!(grant.trade_fee_cap_bps(), FEE_CAP);
    assert_eq!(grant.matcher_program, h.matcher.0.to_bytes());
    assert_eq!(grant.matcher_context, h.matcher.1.to_bytes());
    assert_eq!(grant.matcher_delegate, h.matcher.2.to_bytes());
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        h.grant_sequence
    );
    assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), EXPIRY);
    assert_eq!(cfg.matcher_req_seq, requests);
    assert_eq!(cfg.trade_fee_base_bps, u64::from(FEE_CAP));
    assert_eq!(group.next_market_id, h.next_id);
    assert_eq!(
        group.c_tot,
        2 * (CAPITAL - per_owner_fee) - paid.iter().sum::<u128>()
    );
    assert_eq!(group.insurance, 2 * per_owner_fee);
    assert_eq!(group.vault, 2 * CAPITAL - paid.iter().sum::<u128>());
    assert_eq!(h.env.token_amount(h.env.vault) as u128, group.vault);
    assert_eq!(group.vault, group.c_tot + group.insurance);
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
    for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
        assert_eq!(*budget, fees.get(domain / 2).copied().unwrap_or(0));
    }
    for actor in 0..2 {
        let p = h.env.portfolio_state(h.portfolios[actor]);
        assert_eq!(p.capital.get(), CAPITAL - per_owner_fee - paid[actor]);
        assert_eq!(p.pnl.get(), 0);
        assert_eq!(h.env.portfolio_position_epoch(h.portfolios[actor]), h.epoch);
        assert_eq!(h.env.token_amount(h.tokens[actor]) as u128, paid[actor]);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&p)) as usize,
            h.positions.iter().filter(|q| **q != 0).count()
        );
        for asset in 1..3 {
            if h.positions[asset] != 0 {
                let leg = active_leg_for_asset(&p, asset);
                assert_eq!(leg.market_id, h.ids[asset]);
                assert_eq!(
                    leg.basis_pos_q,
                    h.positions[asset] * if actor == 0 { 1 } else { -1 }
                );
            }
        }
    }
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
}

#[test]
fn v16_program_repriced_asset_reuse_requires_generation_and_fee_consent() {
    let mut peak = 0;
    for reverse in [false, true] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let mut evidence = Evidence::default();
            let sizes = [
                0,
                direction * 100 * POS_SCALE as i128,
                -direction * 2 * POS_SCALE as i128,
            ];
            // Retire a genuinely used slot, with a genuine single-CPI close record retained.
            for delta in [sizes[1], -sizes[1]] {
                h.fill(Route::Single(1), [0, delta, 0], false, &mut evidence);
            }
            let seq = h.env.control_sequences(0);
            send_tx(
                &mut h.env.svm,
                h.env.program_id,
                &h.env.payer,
                ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: u64::from(FEE_CAP),
                    policy_sequence: seq.trade_fee + 1,
                    authority_epoch: seq.authority_epoch,
                },
                vec![
                    AccountMeta::new(h.env.admin.pubkey(), true),
                    AccountMeta::new(h.env.market, false),
                ],
                &[&h.env.admin],
            )
            .expect("establish the fixed fee policy before retaining consent");
            let old_cap = fee(sizes[1], PRICE) + fee(sizes[2], PRICE);
            let new_fees = [0, fee(sizes[1], REPLACEMENT_PRICE), fee(sizes[2], PRICE)];
            let new_cap = new_fees.iter().sum::<u128>();
            assert_eq!((old_cap, new_cap, new_fees), (38, 76, [0, 75, 1]));
            let retained_ix = batch(&h, sizes, reverse, old_cap);
            let retained = h.sign(&retained_ix);
            retained.verify().unwrap();
            h.simulate(&retained, 0, &mut evidence);
            let identities = h.portfolios.map(|p| h.env.portfolio_id(p));
            let portfolio_accounts = h.portfolios.map(|p| h.env.svm.get_account(&p));
            let context = h.env.svm.get_account(&h.matcher.1);
            let blockhash = h.env.svm.latest_blockhash();

            for activate in [false, true] {
                if activate {
                    h.slot += 1;
                    h.env.svm.warp_to_slot(h.slot);
                }
                send_tx(
                    &mut h.env.svm,
                    h.env.program_id,
                    &h.env.payer,
                    ProgInstruction::UpdateAssetLifecycle {
                        action: if activate {
                            processor::ASSET_ACTION_ACTIVATE
                        } else {
                            processor::ASSET_ACTION_RETIRE
                        },
                        asset_index: 1,
                        market_id: if activate { h.next_id } else { h.ids[1] },
                        authority_epoch: seq.authority_epoch,
                        now_slot: h.slot,
                        initial_price: if activate { REPLACEMENT_PRICE } else { 0 },
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
                .expect("publicly retire and reactivate the used slot at its replacement price");
            }
            h.ids[1] = 4;
            h.next_id = 5;
            for asset in 0..3 {
                h.env.configure_auth_mark_for_asset_as_admin(
                    asset,
                    h.slot,
                    if asset == 1 { REPLACEMENT_PRICE } else { PRICE },
                );
            }
            assert_eq!(h.env.svm.latest_blockhash(), blockhash);
            assert!(h.slot < EXPIRY);
            assert_eq!(
                h.portfolios.map(|p| h.env.svm.get_account(&p)),
                portfolio_accounts
            );
            assert_eq!(h.env.svm.get_account(&h.matcher.1), context);
            check(&h, [0; 3], [0; 2], 2);

            peak = peak.max(reject(&mut h, retained, false));
            let generation_only = repair(&retained_ix, &h, 1);
            let tx = h.sign(&generation_only);
            peak = peak.max(reject(&mut h, tx, true));
            // The one-atom boundary proves the failed generation-only retry was economic.
            let tx = h.sign(&batch(&h, sizes, reverse, new_cap - 1));
            peak = peak.max(reject(&mut h, tx, true));
            let fresh_ix = batch(&h, sizes, reverse, new_cap);
            let mut fee_only = fresh_ix.clone();
            if let ProgInstruction::BatchTradeCpi { legs, .. } = &mut fee_only {
                legs.iter_mut()
                    .find(|leg| leg.asset_index == 1)
                    .unwrap()
                    .market_id = 2;
            }
            let tx = h.sign(&fee_only);
            peak = peak.max(reject(&mut h, tx, false));
            check(&h, [0; 3], [0; 2], 2);
            let mut fully_repaired = generation_only;
            if let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut fully_repaired {
                *max_fee_atoms = new_cap;
            }
            assert_eq!(
                fully_repaired.encode(),
                fresh_ix.encode(),
                "only generation and atom cap change"
            );
            let tx = h.sign(&fresh_ix);
            let meta = h
                .env
                .svm
                .send_transaction(tx)
                .expect("fresh bounded consent authorizes replacement exposure and fees");
            assert!(meta
                .logs
                .iter()
                .any(|line| line == &format!("Program {} success", h.matcher.0)));
            peak = peak.max(meta.compute_units_consumed);
            h.positions = sizes;
            h.epoch += 1;
            let mut requests = 3;
            let mut fees = new_fees;
            check(&h, fees, [0; 2], requests);

            for asset in if reverse { [2usize, 1] } else { [1usize, 2] } {
                let ix = h.env.trade_cpi_ix(
                    h.portfolios[0],
                    h.portfolios[1],
                    asset as u16,
                    -sizes[asset],
                    u64::from(FEE_CAP),
                    0,
                );
                let tx = h.sign(&ix);
                let meta = h
                    .env
                    .svm
                    .send_transaction(tx)
                    .expect("current single-CPI consent exits each replacement/sibling leg");
                peak = peak.max(meta.compute_units_consumed);
                h.positions[asset] = 0;
                h.epoch += 1;
                requests += 1;
                fees[asset] += new_fees[asset];
                check(&h, fees, [0; 2], requests);
            }
            assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), identities);
            let entitlement = CAPITAL - 2 * new_cap;
            let mut paid = [0; 2];
            for actor in 0..2 {
                let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
                h.env
                    .send(
                        h.env.withdraw_ix(h.portfolios[actor], entitlement),
                        vec![
                            AccountMeta::new(h.owners[actor].pubkey(), true),
                            AccountMeta::new(h.env.market, false),
                            AccountMeta::new(h.portfolios[actor], false),
                            AccountMeta::new(h.tokens[actor], false),
                            AccountMeta::new(h.env.vault, false),
                            AccountMeta::new_readonly(h.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&h.owners[actor]],
                    )
                    .expect("full input-derived owner SPL entitlement");
                paid[actor] = entitlement;
                h.grant_sequence += u64::from(actor == 1);
                assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
                check(&h, fees, paid, requests);
            }
            assert_eq!(h.env.token_amount(h.env.vault), 304);
        }
    }
    assert_cu_within(
        "row414 repriced generation consent",
        peak,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    println!("row414 repriced generation: worlds=4, retained_live_controls=4, replacements=4, exact_rollbacks=16, fresh_batch_fills=4, single_exits=8, owner_payouts=8, peak_cu={peak}");
}
