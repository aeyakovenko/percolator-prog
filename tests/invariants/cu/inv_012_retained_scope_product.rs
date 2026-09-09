//! Coverage-only INV-012 products, derived from the invariant and public ABI.
//! Reuse the parent's System/SPL/wrapper fixture, never injected engine state.
//! Portfolio reuse joins asset generation and grant incarnation; scope/expiry
//! checks additionally distinguish market-admin epochs from owner delegation.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Consent {
    portfolios: [u64; 2],
    episodes: [u64; 2],
    grant: u64,
    market_id: u64,
}

impl Consent {
    fn capture(h: &History) -> Self {
        Self {
            portfolios: h.portfolios.map(|p| h.env.portfolio_id(p)),
            episodes: h.portfolios.map(|p| h.env.portfolio_position_epoch(p)),
            grant: h.env.portfolio_matcher_sequence(h.portfolios[1]),
            market_id: h.env.asset_market_id(1),
        }
    }

    fn instruction(self, batch: bool, size: i128) -> ProgInstruction {
        if batch {
            ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id: self.portfolios[0],
                account_a_position_epoch: self.episodes[0],
                account_b_portfolio_id: self.portfolios[1],
                account_b_position_epoch: self.episodes[1],
                account_b_matcher_sequence: self.grant,
                max_slippage_atoms: 0,
                max_fee_atoms: 0,
                legs: vec![BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: self.market_id,
                    size_q: size,
                    fee_bps: 0,
                    limit_price: PRICE,
                }],
            }
        } else {
            ProgInstruction::TradeCpi {
                account_a_portfolio_id: self.portfolios[0],
                account_a_position_epoch: self.episodes[0],
                account_b_portfolio_id: self.portfolios[1],
                account_b_position_epoch: self.episodes[1],
                account_b_matcher_sequence: self.grant,
                asset_index: 1,
                market_id: self.market_id,
                size_q: size,
                fee_bps: 0,
                limit_price: PRICE,
                backing_fee_cap_bps: 0,
            }
        }
    }
}

fn trade_accounts(h: &History) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(h.owners[0].pubkey(), true),
        AccountMeta::new(h.env.market, false),
        AccountMeta::new(h.portfolios[0], false),
        AccountMeta::new(h.portfolios[1], false),
        AccountMeta::new_readonly(h.matcher.0, false),
        AccountMeta::new(h.matcher.1, false),
        AccountMeta::new_readonly(h.matcher.2, false),
    ]
}

fn sign(
    h: &History,
    ix: ProgInstruction,
    accounts: Vec<AccountMeta>,
    signers: &[&Keypair],
    transport: u64,
) -> Transaction {
    let mut all_signers = vec![&h.env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: h.env.program_id,
                accounts,
                data: ix.encode(),
            },
            ComputeBudgetInstruction::set_compute_unit_price(transport),
        ],
        Some(&h.env.payer.pubkey()),
        &all_signers,
        h.env.svm.latest_blockhash(),
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn trade(h: &History, consent: Consent, batch: bool, size: i128, transport: u64) -> Transaction {
    let tx = sign(
        h,
        consent.instruction(batch, size),
        trade_accounts(h),
        &[&h.owners[0]],
        transport,
    );
    assert_eq!(
        tx.signatures.len(),
        2,
        "LP never signs capability consumption"
    );
    tx
}

fn frame(h: &History, extra: &[Pubkey]) -> Vec<Option<Account>> {
    let mut result = h.frame();
    result.push(h.env.svm.get_account(&h.matcher.0));
    result.extend(extra.iter().map(|key| h.env.svm.get_account(key)));
    result
}

fn live(h: &mut History, tx: &Transaction, extra: &[Pubkey], evidence: &mut Evidence) {
    let before = frame(h, extra);
    let meta = h
        .env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect("live consent control");
    assert!(meta
        .logs
        .iter()
        .any(|line| line == &format!("Program {} success", h.matcher.0)));
    assert!(meta.compute_units_consumed <= crate::support::v16_svm::TX_CU_LIMIT);
    assert_eq!(frame(h, extra), before, "simulation is read-only");
    evidence.live_simulations += 1;
}

fn reject(
    h: &mut History,
    tx: Transaction,
    expected: InstructionError,
    extra: &[Pubkey],
    evidence: &mut Evidence,
) {
    let before = frame(h, extra);
    let failed = h
        .env
        .svm
        .send_transaction(tx)
        .expect_err("out-of-scope consent must reject");
    assert_eq!(failed.err, TransactionError::InstructionError(2, expected));
    assert!(
        failed
            .meta
            .logs
            .iter()
            .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))),
        "scope rejection must precede matcher CPI"
    );
    assert_eq!(
        frame(h, extra),
        before,
        "exact account/lamport/SPL rollback, excluding only network payer"
    );
    evidence.rejections += 1;
    evidence.rejection_cu = evidence
        .rejection_cu
        .max(failed.meta.compute_units_consumed);
}

fn custom(error: PercolatorError) -> InstructionError {
    InstructionError::Custom(error as u32)
}

fn grant(h: &mut History, enabled: bool, expiry: u64) {
    let before = h.env.portfolio_matcher_sequence(h.portfolios[1]);
    let cap = if enabled { FEE_CAP } else { 0 };
    h.env
        .try_set_matcher_config_with_trade_fee_cap_and_expiry(
            h.matcher.0,
            &h.owners[1],
            h.portfolios[1],
            h.matcher.1,
            h.matcher.2,
            u8::from(enabled),
            cap,
            if enabled { expiry } else { 0 },
        )
        .expect("public owner grant transition");
    h.grant_sequence = before + 1;
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        h.grant_sequence
    );
    let config = h.env.portfolio_matcher_config(h.portfolios[1]);
    assert_eq!(config.enabled(), u64::from(enabled));
    assert_eq!(config.trade_fee_cap_bps(), cap);
    assert_eq!(
        h.env.portfolio_matcher_expiry(h.portfolios[1]),
        if enabled { expiry } else { 0 }
    );
}

fn recycle_portfolio(h: &mut History, actor: usize) {
    let id = h.env.portfolio_id(h.portfolios[actor]);
    let next_id = h.env.market_state().0.next_portfolio_id;
    h.env
        .send(
            h.env.withdraw_ix(h.portfolios[actor], CAPITAL),
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
        .expect("public withdrawal before flat close");
    h.env
        .close_portfolio_with_cu(&h.owners[actor], h.portfolios[actor]);
    send_raw_tx(
        &mut h.env.svm,
        &h.env.payer,
        system_instruction::transfer(&h.env.payer.pubkey(), &h.portfolios[actor], 1_000_000_000),
        &[],
    )
    .expect("System re-funds the closed address");
    h.env.svm.expire_blockhash();
    h.env
        .send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(h.owners[actor].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(h.portfolios[actor], false),
            ],
            &[&h.owners[actor]],
        )
        .expect("same owner reopens the same address");
    assert_eq!(h.env.portfolio_id(h.portfolios[actor]), next_id);
    assert_ne!(id, next_id);
    h.env
        .send(
            h.env.deposit_ix(h.portfolios[actor], CAPITAL),
            vec![
                AccountMeta::new(h.owners[actor].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(h.portfolios[actor], false),
                AccountMeta::new(h.tokens[actor], false),
                AccountMeta::new(h.env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&h.owners[actor]],
        )
        .expect("restore the same collateral through SPL custody");
    if actor == 1 {
        grant(h, true, EXPIRY);
    }
    h.assert_state();
}

fn finish(h: &mut History, batch: bool, size: i128, evidence: &mut Evidence) {
    let route = |batch| {
        if batch {
            Route::Batch
        } else {
            Route::Single(1)
        }
    };
    h.fill(route(batch), [0, size, 0], false, evidence);
    h.fill(route(!batch), [0, -size, 0], false, evidence);
    h.withdraw_all(false, evidence);
}

#[test]
fn v16_program_retained_scope_product_requires_portfolio_asset_and_grant() {
    let mut evidence = Evidence::default();
    for actor in 0..2 {
        for portfolio_first in [false, true] {
            for retained_batch in [false, true] {
                for signum in [-1, 1] {
                    let mut h = History::new();
                    let size = signum * POS_SCALE as i128;
                    let old = Consent::capture(&h);
                    let retained = trade(&h, old, retained_batch, size, 1);
                    live(&mut h, &retained, &[], &mut evidence);
                    if portfolio_first {
                        recycle_portfolio(&mut h, actor);
                    }
                    h.replace(1, &mut evidence);
                    if !portfolio_first {
                        recycle_portfolio(&mut h, actor);
                    }
                    grant(&mut h, false, 0);
                    grant(&mut h, true, EXPIRY);
                    h.assert_state();
                    let current = Consent::capture(&h);
                    assert_ne!(old.portfolios[actor], current.portfolios[actor]);
                    assert_ne!(old.market_id, current.market_id);
                    assert_ne!(old.grant, current.grant);
                    assert_eq!(
                        old.episodes, current.episodes,
                        "no stale episode can mask a scope check"
                    );
                    reject(
                        &mut h,
                        retained,
                        custom(PercolatorError::EngineProvenanceMismatch),
                        &[],
                        &mut evidence,
                    );

                    // Every proper repair subset, on both the retained and substituted route.
                    // In particular masks 3/5/6 leave exactly one stale authorization binding.
                    for batch in [retained_batch, !retained_batch] {
                        for repaired in 0u8..8 {
                            let consent = Consent {
                                portfolios: if repaired & 1 != 0 {
                                    current.portfolios
                                } else {
                                    old.portfolios
                                },
                                market_id: if repaired & 2 != 0 {
                                    current.market_id
                                } else {
                                    old.market_id
                                },
                                grant: if repaired & 4 != 0 {
                                    current.grant
                                } else {
                                    old.grant
                                },
                                episodes: old.episodes,
                            };
                            let tx = trade(&h, consent, batch, size, 10 + u64::from(repaired));
                            if repaired == 7 {
                                live(&mut h, &tx, &[], &mut evidence);
                            } else {
                                let error = if repaired & 1 == 0 {
                                    PercolatorError::EngineProvenanceMismatch
                                } else if repaired & 2 == 0 {
                                    PercolatorError::AssetGenerationMismatch
                                } else {
                                    PercolatorError::EngineStale
                                };
                                reject(&mut h, tx, custom(error), &[], &mut evidence);
                            }
                        }
                    }
                    finish(&mut h, retained_batch, size, &mut evidence);
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (
            evidence.worlds,
            evidence.live_simulations,
            evidence.rejections,
            evidence.fills
        ),
        (16, 48, 240, 32)
    );
    eprintln!("INV-012 retained scope product: {evidence:?}");
}

#[test]
fn v16_program_retained_scope_product_separates_admin_epoch_tuple_and_expiry() {
    let mut evidence = Evidence::default();
    for batch in [false, true] {
        for signum in [-1, 1] {
            let mut h = History::new();
            let size = signum * POS_SCALE as i128;
            let original = h.matcher;
            let (alternate_context, alternate_delegate, _) =
                h.env.init_auth_matcher_context_via_system_create(
                    h.matcher.0,
                    &h.owners[1],
                    h.portfolios[1],
                );
            // Return to A after a genuinely admitted same-program B context/delegate.
            grant(&mut h, true, 4);
            let foreign = Keypair::new();
            system_create_account_for_test(
                &mut h.env.svm,
                &h.env.payer,
                &foreign,
                state::market_account_len_for_capacity(3).unwrap(),
                h.env.program_id,
            );
            send_tx(
                &mut h.env.svm,
                h.env.program_id,
                &h.env.payer,
                init_market_instruction(&V16CuMarketParams {
                    max_portfolio_assets: 3,
                    ..V16CuMarketParams::default()
                }),
                vec![
                    AccountMeta::new(h.env.admin.pubkey(), true),
                    AccountMeta::new(foreign.pubkey(), false),
                    AccountMeta::new_readonly(h.env.mint, false),
                ],
                &[&h.env.admin],
            )
            .expect("public second slab initialization");
            let extra = [alternate_context, alternate_delegate, foreign.pubkey()];
            let consent = Consent::capture(&h);
            let retained = trade(&h, consent, batch, size, 1);
            live(&mut h, &retained, &extra, &mut evidence);

            let epoch = h.env.control_sequences(0).authority_epoch;
            let handoff = |h: &History, from: &Keypair, to: &Keypair, epoch, nonce| {
                sign(
                    h,
                    ProgInstruction::UpdateAuthority {
                        authority_epoch: epoch,
                        new_pubkey: to.pubkey().to_bytes(),
                    },
                    vec![
                        AccountMeta::new(from.pubkey(), true),
                        AccountMeta::new(to.pubkey(), true),
                        AccountMeta::new(h.env.market, false),
                    ],
                    &[from, to],
                    nonce,
                )
            };
            let old_admin = handoff(&h, &h.env.admin, &h.owners[0], epoch, 2);
            h.env
                .svm
                .simulate_transaction(old_admin.clone().into())
                .expect("retained admin control was admissible");
            let owner_before = h.portfolios.map(|key| h.env.svm.get_account(&key));
            let matcher_before =
                [original.1, alternate_context].map(|key| h.env.svm.get_account(&key));
            let forward = handoff(&h, &h.env.admin, &h.owners[0], epoch, 3);
            h.env.svm.send_transaction(forward).expect("admin handoff");
            let back = handoff(&h, &h.owners[0], &h.env.admin, epoch + 1, 4);
            h.env
                .svm
                .send_transaction(back)
                .expect("admin returns at a distinct authority epoch");
            assert_eq!(h.env.control_sequences(0).authority_epoch, epoch + 2);
            assert_eq!(
                Consent::capture(&h),
                consent,
                "admin epoch is not owner matcher consent"
            );
            assert_eq!(
                h.portfolios.map(|key| h.env.svm.get_account(&key)),
                owner_before
            );
            assert_eq!(
                [original.1, alternate_context].map(|key| h.env.svm.get_account(&key)),
                matcher_before
            );
            reject(
                &mut h,
                old_admin,
                custom(PercolatorError::EngineStale),
                &extra,
                &mut evidence,
            );
            live(&mut h, &retained, &extra, &mut evidence);

            // Each single-field substitution has otherwise current consent; the
            // displaced canonical pair separately reaches the grant tuple guard.
            for route in [batch, !batch] {
                let wrong_instrument = Consent {
                    market_id: h.env.asset_market_id(0),
                    ..consent
                };
                assert_ne!(wrong_instrument.market_id, consent.market_id);
                let tx = trade(&h, wrong_instrument, route, size, 9);
                reject(
                    &mut h,
                    tx,
                    custom(PercolatorError::AssetGenerationMismatch),
                    &extra,
                    &mut evidence,
                );
                for substitution in 0..4 {
                    let mut accounts = trade_accounts(&h);
                    let expected = match substitution {
                        0 => {
                            accounts[5].pubkey = alternate_context;
                            InstructionError::InvalidArgument
                        }
                        1 => {
                            accounts[6].pubkey = alternate_delegate;
                            InstructionError::InvalidArgument
                        }
                        2 => {
                            accounts[1].pubkey = foreign.pubkey();
                            InstructionError::InvalidArgument
                        }
                        _ => {
                            accounts[5].pubkey = alternate_context;
                            accounts[6].pubkey = alternate_delegate;
                            custom(PercolatorError::Unauthorized)
                        }
                    };
                    let tx = sign(
                        &h,
                        consent.instruction(route, size),
                        accounts,
                        &[&h.owners[0]],
                        10 + substitution,
                    );
                    reject(&mut h, tx, expected, &extra, &mut evidence);
                }
            }
            grant(&mut h, false, 0);
            reject(
                &mut h,
                retained,
                custom(PercolatorError::EngineStale),
                &extra,
                &mut evidence,
            );
            for route in [batch, !batch] {
                let disabled = trade(&h, Consent::capture(&h), route, size, 20);
                reject(
                    &mut h,
                    disabled,
                    custom(PercolatorError::Unauthorized),
                    &extra,
                    &mut evidence,
                );
            }
            grant(&mut h, true, 4);
            for route in [batch, !batch] {
                let stale = trade(&h, consent, route, size, 21);
                reject(
                    &mut h,
                    stale,
                    custom(PercolatorError::EngineStale),
                    &extra,
                    &mut evidence,
                );
            }
            h.env.svm.warp_to_slot(3);
            let current = Consent::capture(&h);
            let expiring = [
                trade(&h, current, batch, size, 22),
                trade(&h, current, !batch, size, 22),
            ];
            let late = [
                trade(&h, current, batch, size, 24),
                trade(&h, current, !batch, size, 24),
            ];
            for (at_boundary, after_boundary) in expiring.iter().zip(&late) {
                assert_eq!(
                    at_boundary.message.instructions[2],
                    after_boundary.message.instructions[2]
                );
                assert_ne!(at_boundary.signatures, after_boundary.signatures);
            }
            for tx in &expiring {
                live(&mut h, tx, &extra, &mut evidence);
            }
            h.env.svm.warp_to_slot(4);
            for tx in expiring {
                reject(
                    &mut h,
                    tx,
                    custom(PercolatorError::Unauthorized),
                    &extra,
                    &mut evidence,
                );
            }
            h.env.svm.warp_to_slot(5);
            for tx in late {
                reject(
                    &mut h,
                    tx,
                    custom(PercolatorError::Unauthorized),
                    &extra,
                    &mut evidence,
                );
            }
            for route in [batch, !batch] {
                let rebuilt = trade(&h, Consent::capture(&h), route, size, 23);
                reject(
                    &mut h,
                    rebuilt,
                    custom(PercolatorError::Unauthorized),
                    &extra,
                    &mut evidence,
                );
            }
            grant(&mut h, true, EXPIRY);
            h.slot = 5;
            h.assert_state();
            let extra_before = extra.map(|key| h.env.svm.get_account(&key));
            finish(&mut h, batch, size, &mut evidence);
            assert_eq!(
                extra.map(|key| h.env.svm.get_account(&key)),
                extra_before,
                "fresh fills and custody cannot touch a displaced context or slab"
            );
            evidence.worlds += 1;
        }
    }
    assert_eq!(
        (
            evidence.worlds,
            evidence.live_simulations,
            evidence.rejections,
            evidence.fills
        ),
        (4, 16, 88, 8)
    );
    eprintln!("INV-012 admin/tuple/expiry scope product: {evidence:?}");
}
