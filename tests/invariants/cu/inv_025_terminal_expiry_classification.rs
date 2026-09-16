//! INV-025/033/041/063/070: impaired backing, Recovery and terminal classification.
//! Public counterparty liens remain disjoint from funded insurance. Both source
//! sides, expiry/shutdown orders and independent owner forfeit/settlement orders
//! converge to exact payouts and burn.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const INSURANCE: u128 = 83;
const BACKING: [u128; 2] = [150, 17];
const EXPIRY: u64 = 3;
const CAPITAL: [u128; 3] = [313 - 50, 1_000 - 100, 0];
const CLAIMS: [u128; 3] = [20 * 5, 10 * 5, 0];
const PAYOUTS: [u64; 3] = [363, 950, 0];
const BOOKED: u128 = 313 + 1_000 + 150 + 17 + INSURANCE;
const RESIDUE: u128 = BACKING[0] + BACKING[1];
const LIMIT: u64 = 500_000;

#[derive(Default)]
struct Evidence {
    peak: u64,
    rollback_peak: u64,
    close_peak: u64,
    commits: usize,
    rollbacks: usize,
    waits: usize,
    payout_rollbacks: usize,
    closures: usize,
}

impl Evidence {
    fn record(&mut self, cu: u64) {
        assert_cu_within("INV-025 terminal lien classification", cu, LIMIT);
        self.peak = self.peak.max(cu);
    }
}

fn wrap(w: &World, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        data: ix.encode(),
        accounts,
    }
}

fn close_slab(w: &World, epoch: u64) -> Instruction {
    wrap(
        w,
        ProgInstruction::CloseSlab {
            authority_epoch: epoch,
        },
        vec![
            AccountMeta::new(w.env.admin.pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.env.vault, false),
            AccountMeta::new_readonly(w.env.vault_authority, false),
            AccountMeta::new(w.tokens[3], false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(w.env.mint, false),
        ],
    )
}

fn insurance(w: &World, amount: u128, epoch: u64) -> Instruction {
    wrap(
        w,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: w.env.asset_market_id(0),
            authority_epoch: epoch,
            amount,
        },
        vec![
            AccountMeta::new_readonly(w.env.admin.pubkey(), false),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.tokens[3], false),
            AccountMeta::new(w.env.vault, false),
            AccountMeta::new_readonly(w.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

// All failures restore complete accounts except the exact signature fee, even
// when an SPL payout already executed. The test never restores a snapshot.
fn submit(
    w: &mut World,
    ixs: &[Instruction],
    rejection: Option<(u8, PercolatorError, usize, usize)>,
    may_wait: bool,
    evidence: &mut Evidence,
) -> bool {
    w.env.svm.expire_blockhash();
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    instructions.extend_from_slice(ixs);
    let mut signers = vec![&w.env.payer];
    signers.extend(w.owners.iter().chain([&w.env.admin]).filter(|key| {
        ixs.iter().any(|ix| {
            ix.accounts
                .iter()
                .any(|m| m.is_signer && m.pubkey == key.pubkey())
        })
    }));
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&w.env.payer.pubkey()),
        &signers,
        w.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(w.portfolios);
    keys.extend(w.tokens);
    keys.extend(w.owners.each_ref().map(Signer::pubkey));
    keys.extend([w.env.market, w.env.vault, w.env.mint, w.env.admin.pubkey()]);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| w.env.svm.get_account(key)).collect();
    let (meta, committed) = match w.env.svm.send_transaction(tx) {
        Ok(meta) => {
            assert!(rejection.is_none(), "expected rejection committed");
            evidence.commits += 1;
            (meta, true)
        }
        Err(failure) => {
            evidence.waits += usize::from(rejection.is_none());
            let (index, error, wrapper_successes, spl_successes) = rejection.unwrap_or_else(|| {
                assert!(may_wait, "unexpected failure: {failure:?}");
                (2, PercolatorError::EngineNonProgress, 0, 0)
            });
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
            );
            for (program, count) in [
                (w.env.program_id, wrapper_successes),
                (spl_token::ID, spl_successes),
            ] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count
                );
            }
            evidence.rollbacks += 1;
            evidence.payout_rollbacks += usize::from(spl_successes > 0);
            evidence.rollback_peak = evidence
                .rollback_peak
                .max(failure.meta.compute_units_consumed);
            (failure.meta, false)
        }
    };
    for (key, mut frame) in keys.iter().zip(before) {
        if *key == w.env.payer.pubkey() {
            frame.as_mut().unwrap().lamports -= fee;
        }
        let writable = ixs.iter().any(|ix| {
            ix.accounts
                .iter()
                .any(|m| m.pubkey == *key && m.is_writable)
        });
        if !committed || !writable {
            assert_eq!(
                w.env.svm.get_account(key),
                frame,
                "complete Account frame: {key}"
            );
        }
    }
    evidence.record(meta.compute_units_consumed);
    committed
}

fn census(w: &World, paid_insurance: u128, portfolios_present: bool) {
    let env = &w.env;
    let market = env.svm.get_account(&env.market).unwrap();
    let g = env.market_state().1;
    let accounts: Vec<_> = if portfolios_present {
        w.portfolios
            .map(|p| env.portfolio_state(p))
            .into_iter()
            .collect()
    } else {
        vec![]
    };
    assert_market_stock_census(
        "terminal lien classes",
        &g,
        &market.data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("terminal lien classes", &g, &accounts).unwrap();
    assert_eq!(env.svm.get_account(&env.mint).unwrap(), w.mint_frame);
    assert_eq!(g.insurance, INSURANCE - paid_insurance);
    assert_eq!(g.backing_provider_earnings_total, 0);
    assert_eq!(g.source_insurance_credit_reserved_total_atoms, 0);
    assert_eq!(g.insurance_domain_spent.iter().sum::<u128>(), 0);
    assert_eq!(
        g.insurance_domain_budget.iter().sum::<u128>(),
        INSURANCE - paid_insurance
    );
    for (source, bucket) in g.source_credit.iter().zip(&g.source_backing_buckets) {
        assert_eq!(
            [
                source.insurance_credit_reserved_num,
                source.valid_liened_insurance_num,
                source.impaired_liened_insurance_num,
                source.spent_backing_num,
                source.provider_receivable_num,
                bucket.consumed_liened_backing_num,
                bucket.utilization_fee_earnings
            ],
            [0; 7]
        );
    }
    for account in &accounts {
        for source in account.source_domains {
            assert_eq!(source.source_claim_insurance_liened_num.get(), 0);
            assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
        }
    }
    let cash = w.tokens.map(|key| u128::from(env.token_amount(key)));
    assert_eq!(cash[2], u128::from(ENDOWMENTS[2]));
    assert_eq!(
        cash[3],
        u128::from(ENDOWMENTS[3]) - RESIDUE - INSURANCE + paid_insurance
    );
    assert_eq!(
        g.vault + cash.iter().sum::<u128>(),
        ENDOWMENTS.iter().map(|n| u128::from(*n)).sum::<u128>()
    );
    for actor in 0..2 {
        assert!(
            cash[actor] <= u128::from(PAYOUTS[actor]),
            "owner entitlement cap"
        );
    }
}

fn classified(w: &World, winning_domain: usize, lien: u128, impaired: bool) {
    census(w, 0, true);
    let g = w.env.market_state().1;
    assert_eq!(
        (g.vault, g.c_tot, g.pnl_pos_tot),
        (BOOKED, CAPITAL.iter().sum(), CLAIMS.iter().sum())
    );
    let source = g.source_credit[winning_domain];
    let bucket = g.source_backing_buckets[winning_domain];
    let total = BACKING[0] + CLAIMS[0];
    assert_eq!(
        bucket.status,
        if impaired {
            BackingBucketStatusV16::Impaired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(source.positive_claim_bound_num, CLAIMS[0] * SCALE);
    assert_eq!(
        source.fresh_reserved_backing_num,
        if impaired { 0 } else { total * SCALE }
    );
    assert_eq!(
        bucket.fresh_unliened_backing_num,
        if impaired { 0 } else { (total - lien) * SCALE }
    );
    assert_eq!(
        [
            source.valid_liened_backing_num,
            bucket.valid_liened_backing_num
        ],
        [if impaired { 0 } else { lien * SCALE }; 2]
    );
    assert_eq!(
        [
            source.impaired_liened_backing_num,
            bucket.impaired_liened_backing_num
        ],
        [if impaired { lien * SCALE } else { 0 }; 2]
    );
    for actor in 0..3 {
        let a = w.env.portfolio_state(w.portfolios[actor]);
        assert_eq!(
            (a.capital.get(), a.pnl.get()),
            (CAPITAL[actor], CLAIMS[actor] as i128)
        );
    }
    let a = w.env.portfolio_state(w.portfolios[0]);
    let local = state::portfolio_source_domain(&a, winning_domain);
    assert_eq!(
        local.source_claim_counterparty_liened_num.get(),
        lien * SCALE
    );
    assert_eq!(
        local.source_lien_counterparty_backing_num.get(),
        lien * SCALE
    );
    // PnL faces and liens label these stocks; neither is an additional asset.
    let fresh = g
        .source_credit
        .iter()
        .map(|s| s.fresh_reserved_backing_num / SCALE)
        .sum::<u128>();
    let impaired_stock = g
        .source_credit
        .iter()
        .map(|s| s.impaired_liened_backing_num / SCALE)
        .sum::<u128>();
    assert_eq!(
        fresh,
        if impaired {
            BACKING[1] + CLAIMS[1]
        } else {
            RESIDUE + CLAIMS.iter().sum::<u128>()
        }
    );
    assert_eq!(impaired_stock, if impaired { lien } else { 0 });
    let released = if impaired { total - lien } else { 0 };
    assert_eq!(
        BOOKED,
        g.c_tot + INSURANCE + fresh + impaired_stock + released
    );
}

#[test]
fn v16_program_expired_lien_recovery_classifies_terminal_atoms_once_across_orders() {
    let mut evidence = Evidence::default();
    let mut baseline = None;
    for direction in [-1i128, 1] {
        for recovery_first in [false, true] {
            for (reverse_forfeit, reverse_settle) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut w = World::new();
                let winning_domain = usize::from(direction > 0);
                let adverse_domain = 2 + usize::from(direction < 0);
                let winning_mark = (100 + 5 * direction) as u64;
                let adverse_mark = (100 - 5 * direction) as u64;
                for asset in 0..2 {
                    evidence.record(w.env.configure_auth_mark_for_asset_as_admin(asset, 1, 100));
                }
                evidence.record(w.env.configure_permissionless_resolve_with_cu(1_000, 1));
                for (actor, amount) in [(0, 313), (1, 1_000)] {
                    evidence.record(w.send(w.deposit(actor, amount)));
                }
                for (domain, amount) in [(winning_domain, BACKING[0]), (adverse_domain, BACKING[1])]
                {
                    let mut ix = w.reserve(domain as u16, amount, true);
                    ix.data = ProgInstruction::TopUpBackingBucket {
                        domain: domain as u16,
                        market_id: w.env.asset_market_id(domain as u16 / 2),
                        authority_epoch: w.env.control_sequences(domain / 2).authority_epoch,
                        intent_id: next_control_sequence(
                            w.env.control_sequences(domain / 2).backing_top_up,
                        ),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount,
                        expiry_slot: EXPIRY,
                    }
                    .encode();
                    evidence.record(w.send(ix));
                }
                evidence.record(w.send(w.reserve(winning_domain as u16, INSURANCE, false)));
                for (asset, lots) in [(0, 20), (1, 10)] {
                    evidence.record(w.send(w.trade(asset, lots * direction, 100)));
                }
                w.env.svm.warp_to_slot(2);
                evidence.record(w.env.push_auth_mark_for_asset_as_admin(0, 2, winning_mark));
                evidence.record(w.env.push_auth_mark_for_asset_as_admin(1, 2, adverse_mark));
                for (actor, asset) in [(1, 0), (0, 0), (1, 1)] {
                    evidence.record(w.env.crank(
                        w.portfolios[actor],
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 2,
                            observations: crank_observations_for_assets(&[asset, 1 - asset]),
                        },
                    ));
                }
                evidence.record(w.send(w.trade(1, 2 * direction, adverse_mark)));
                let lien = (20 * u128::from(winning_mark) + 12 * u128::from(adverse_mark))
                    .div_ceil(10)
                    - CAPITAL[0];
                assert_eq!(lien, if direction > 0 { 61 } else { 53 });
                classified(&w, winning_domain, lien, false);
                let admin = w.env.admin.insecure_clone();
                if recovery_first {
                    for asset in 0..2 {
                        evidence.record(
                            w.env
                                .try_shutdown_asset_with_authority(&admin, asset, 2)
                                .unwrap(),
                        );
                    }
                    classified(&w, winning_domain, lien, false);
                }
                w.env.svm.warp_to_slot(EXPIRY);
                if !recovery_first {
                    for asset in 0..2 {
                        evidence.record(w.env.push_auth_mark_for_asset_as_admin(
                            asset,
                            EXPIRY,
                            [winning_mark, adverse_mark][asset as usize],
                        ));
                    }
                }
                for _ in 0..8 {
                    if w.env.market_state().1.source_backing_buckets[winning_domain].status
                        != BackingBucketStatusV16::Fresh
                    {
                        break;
                    }
                    evidence.record(w.env.crank(
                        w.portfolios[0],
                        ProgInstruction::PermissionlessCrank {
                            now_slot: EXPIRY,
                            observations: if recovery_first {
                                vec![]
                            } else {
                                crank_observations_for_assets(&[0, 1])
                            },
                        },
                    ));
                    census(&w, 0, true);
                }
                classified(&w, winning_domain, lien, true);
                if !recovery_first {
                    for asset in 0..2 {
                        evidence.record(
                            w.env
                                .try_shutdown_asset_with_authority(&admin, asset, EXPIRY)
                                .unwrap(),
                        );
                    }
                }
                for asset in 0..2 {
                    assert_eq!(
                        w.env.market_state().1.assets[asset].lifecycle,
                        AssetLifecycleV16::Recovery
                    );
                }
                let order = if reverse_forfeit { [1, 0] } else { [0, 1] };
                for actor in order {
                    for asset in 0..2 {
                        evidence.record(w.env.forfeit_recovery_leg_with_cu(
                            &w.owners[actor],
                            w.portfolios[actor],
                            asset,
                            u128::MAX,
                        ));
                        classified(&w, winning_domain, lien, true);
                    }
                }
                for p in w.portfolios {
                    let a = w.env.portfolio_state(p);
                    for asset in 0..2 {
                        if has_active_leg_for_asset(&a, asset) {
                            assert_eq!(active_leg_for_asset(&a, asset).basis_pos_q, 0,
                                "Recovery may retain a zero-basis obligation until terminal cleanup");
                        }
                    }
                }
                evidence.record(w.env.resolve());
                classified(&w, winning_domain, lien, true);
                assert_eq!(w.env.market_state().1.mode, MarketModeV16::Resolved);

                let early = close_slab(&w, w.env.control_sequences(0).authority_epoch);
                submit(
                    &mut w,
                    &[early],
                    Some((2, PercolatorError::EngineLockActive, 0, 0)),
                    false,
                    &mut evidence,
                );
                let settlement = if reverse_settle { [1, 0, 2] } else { [0, 1, 2] };
                for _ in 0..32 {
                    if w.portfolios
                        .iter()
                        .all(|p| resolved_portfolio_is_terminal(&w.env, *p))
                    {
                        break;
                    }
                    let mut progressed = false;
                    for actor in settlement {
                        if resolved_portfolio_is_terminal(&w.env, w.portfolios[actor]) {
                            continue;
                        }
                        let ix = wrap(
                            &w,
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            },
                            vec![
                                AccountMeta::new_readonly(w.owners[actor].pubkey(), true),
                                AccountMeta::new(w.env.market, false),
                                AccountMeta::new(w.portfolios[actor], false),
                                AccountMeta::new(w.tokens[actor], false),
                                AccountMeta::new(w.env.vault, false),
                                AccountMeta::new_readonly(w.env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                        );
                        let before = [w.env.market, w.portfolios[actor], w.env.vault]
                            .map(|k| w.env.svm.get_account(&k));
                        if submit(&mut w, &[ix], None, true, &mut evidence) {
                            assert_ne!(
                                [w.env.market, w.portfolios[actor], w.env.vault]
                                    .map(|k| w.env.svm.get_account(&k)),
                                before,
                                "successful resolved step must progress"
                            );
                            progressed = true;
                        }
                        census(&w, 0, true);
                    }
                    assert!(
                        progressed,
                        "funded cohort cannot reach a nonterminal fixed point"
                    );
                }
                assert!(w
                    .portfolios
                    .iter()
                    .all(|p| resolved_portfolio_is_terminal(&w.env, *p)));
                let slab_before_deletion = w.env.svm.get_account(&w.env.market).unwrap().lamports;
                let portfolio_rent: u64 = w
                    .portfolios
                    .iter()
                    .map(|key| w.env.svm.get_account(key).unwrap().lamports)
                    .sum();
                for actor in 0..3 {
                    assert_eq!(
                        w.env.token_amount(w.tokens[actor]),
                        PAYOUTS[actor] + if actor == 2 { ENDOWMENTS[2] } else { 0 }
                    );
                    evidence.record(
                        w.env
                            .close_portfolio_with_cu(&w.owners[actor], w.portfolios[actor]),
                    );
                }
                assert_eq!(
                    w.env.svm.get_account(&w.env.market).unwrap().lamports,
                    slab_before_deletion + portfolio_rent
                );
                for actor in 0..3 {
                    assert!(w
                        .env
                        .svm
                        .get_account(&w.portfolios[actor])
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    assert_eq!(
                        w.env.svm.get_account(&w.owners[actor].pubkey()).as_ref(),
                        Some(&w.authority_frames[actor])
                    );
                }
                census(&w, 0, false);
                let g = w.env.market_state().1;
                assert_eq!(
                    (
                        g.c_tot,
                        g.pnl_pos_tot,
                        g.source_claim_bound_total_num,
                        g.materialized_portfolio_count
                    ),
                    (0, 0, 0, 0)
                );
                for domain in [winning_domain, adverse_domain] {
                    assert_eq!(
                        g.source_backing_buckets[domain].status,
                        BackingBucketStatusV16::Expired
                    );
                    assert_eq!(g.source_credit[domain].impaired_liened_backing_num, 0);
                    assert_eq!(g.source_credit[domain].fresh_reserved_backing_num, 0);
                }
                assert_eq!(g.vault, RESIDUE + INSURANCE);
                assert_eq!(
                    g.resolved_payout_ledger.snapshot_residual,
                    RESIDUE + CLAIMS.iter().sum::<u128>()
                );
                assert_eq!(
                    g.resolved_payout_ledger.terminal_claim_exact_receipts_num,
                    CLAIMS.iter().sum::<u128>() * SCALE
                );
                let epoch = w.env.control_sequences(0).authority_epoch;
                let partial = insurance(&w, 40, epoch);
                let premature_close = close_slab(&w, epoch + 1);
                submit(
                    &mut w,
                    &[partial.clone(), premature_close],
                    Some((3, PercolatorError::EngineLockActive, 1, 1)),
                    false,
                    &mut evidence,
                );
                census(&w, 0, false);
                submit(&mut w, &[partial], None, false, &mut evidence);
                census(&w, 40, false);
                let tail = insurance(&w, INSURANCE - 40, epoch + 1);
                submit(&mut w, &[tail], None, false, &mut evidence);
                census(&w, INSURANCE, false);
                assert_eq!(w.env.market_state().1.vault, RESIDUE);
                let late_claim = insurance(&w, 1, epoch + 2);
                submit(
                    &mut w,
                    &[late_claim],
                    Some((2, PercolatorError::EngineLockActive, 0, 0)),
                    false,
                    &mut evidence,
                );
                census(&w, INSURANCE, false);

                let market_rent = w.env.svm.get_account(&w.env.market).unwrap().lamports;
                let vault_rent = w.env.svm.get_account(&w.env.vault).unwrap().lamports;
                let admin_before = w
                    .env
                    .svm
                    .get_account(&w.env.admin.pubkey())
                    .unwrap()
                    .lamports;
                let token_frames = w.tokens.map(|k| w.env.svm.get_account(&k));
                let tombstone_rent = w
                    .env
                    .svm
                    .get_sysvar::<solana_sdk::rent::Rent>()
                    .minimum_balance(percolator_prog::constants::HEADER_LEN);
                for _ in 0..8 {
                    if w.env.svm.get_account(&w.env.market).unwrap().data.len()
                        == percolator_prog::constants::HEADER_LEN
                    {
                        break;
                    }
                    let ix = close_slab(&w, w.env.control_sequences(0).authority_epoch);
                    let cu = w.send(ix);
                    evidence.record(cu);
                    evidence.close_peak = evidence.close_peak.max(cu);
                    w.env.svm.expire_blockhash();
                }
                let tombstone = w.env.svm.get_account(&w.env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(tombstone.lamports, tombstone_rent);
                assert!(w
                    .env
                    .svm
                    .get_account(&w.env.vault)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                assert_eq!(
                    w.env
                        .svm
                        .get_account(&w.env.admin.pubkey())
                        .unwrap()
                        .lamports
                        - admin_before,
                    market_rent + vault_rent - tombstone_rent
                );
                assert_eq!(
                    w.tokens.map(|k| w.env.svm.get_account(&k)),
                    token_frames,
                    "booked residue is burned, never swept to the admin"
                );
                let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
                assert_eq!(
                    u128::from(mint.supply),
                    ENDOWMENTS.iter().map(|n| u128::from(*n)).sum::<u128>() - RESIDUE
                );
                assert_eq!(mint.mint_authority, COption::None);
                let mut expected_mint = w.mint_frame.clone();
                let mut expected = Mint::unpack(&expected_mint.data).unwrap();
                expected.supply -= RESIDUE as u64;
                Mint::pack(expected, &mut expected_mint.data).unwrap();
                assert_eq!(w.env.svm.get_account(&w.env.mint).unwrap(), expected_mint);
                let cash = w.tokens.map(|key| w.env.token_amount(key));
                assert_eq!(
                    cash,
                    [
                        PAYOUTS[0],
                        PAYOUTS[1],
                        ENDOWMENTS[2],
                        ENDOWMENTS[3] - RESIDUE as u64
                    ]
                );
                assert_eq!(cash.iter().sum::<u64>(), mint.supply);
                let outcome = (cash, mint.supply, tombstone.lamports);
                if let Some(expected) = baseline {
                    assert_eq!(
                        outcome, expected,
                        "economic outcome must commute across order dimensions"
                    );
                } else {
                    baseline = Some(outcome);
                }
                evidence.closures += 1;
            }
        }
    }
    assert_eq!(evidence.closures, 16);
    assert_eq!(evidence.payout_rollbacks, 16);
    assert_eq!(evidence.waits, 8);
    assert_eq!(evidence.rollbacks, 56);
    println!("terminal lien classification: worlds={}, checked commits={}, exact rollbacks={}, waiting rollbacks={}, SPL-prefix rollbacks={}, peak={} CU, rollback peak={} CU, slab peak={} CU; payouts={PAYOUTS:?}, insurance={INSURANCE}, burned={RESIDUE}", evidence.closures, evidence.commits, evidence.rollbacks, evidence.waits, evidence.payout_rollbacks, evidence.peak, evidence.rollback_peak, evidence.close_peak);
}
