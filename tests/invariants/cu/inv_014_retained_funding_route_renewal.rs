//! Row 411: bilateral funding collection revokes a grant; a retained renewal
//! cannot enlarge the residual batch's gross fee cap using fresh funding PnL.
//! The parent supplies unchanged public construction, rollback and input ledger.

use super::*;

fn observed_crank(w: &World, actor: usize, slot: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(w.env.payer.pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.portfolios[actor], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(0),
        }
        .encode(),
    }
}

#[test]
fn v16_retained_funded_bilateral_reduction_and_renewal_preserve_residual_aggregate_cap() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let reduction = 57 * POS_SCALE as i128;
    let residual = QUANTITY - reduction;
    let signed_fees = [QUANTITY, reduction, residual].map(|q| fee(q, OPEN_CAP));
    assert_eq!(signed_fees, [36, 22, 15]);
    let funding = [QUANTITY, residual].map(funding_quote);
    assert_eq!(funding, [95, 38]);
    assert!(u128::from(funding[1]) > fee(residual, CLOSE_FRESH));
    let mut counts = Counts::default();
    for direction in [-1, 1] {
        let mut endpoint = None;
        for direct in [Route::SingleNoCpi, Route::BatchNoCpi] {
            eprintln!("funded route renewal: direction={direction}, direct={direct:?}");
            let mut w = World::with_params(
                &matcher,
                V16CuMarketParams {
                    trade_fee_base_bps: OPEN_CAP,
                    maintenance_fee_per_slot: MAINTENANCE.into(),
                    max_abs_funding_e9_per_slot: FUNDING_RATE,
                    max_price_move_bps_per_slot: 1,
                    h_max: 1,
                    ..V16CuMarketParams::default()
                },
            );
            counts.record(w.env.configure_ewma_mark_with_cu(0, PRICE, 1, 0));
            counts.record(w.partial(95));
            let opening = w.bundle_instructions(
                Route::PartialCpi,
                direction * 255 * POS_SCALE as i128,
                direction * QUANTITY,
                OPEN_CAP,
            );
            let tx = w.sign_with_nonce(&opening, 1);
            counts.record(w.deliver(
                tx,
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.tokens[0],
                    w.env.vault,
                    w.context,
                ],
                [2, 1, 1],
            ));
            counts.fills += 1;
            let fill =
                read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
            assert_eq!(
                (fill.exec_size, fill.exec_price_e6),
                (direction * QUANTITY, PRICE)
            );
            assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);
            let mut book = Ledger::new(&w, direction);
            book.check(&w);
            let reset = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                        AccountMeta::new(w.context, false),
                    ],
                    data: vec![11, 9, 0],
                }],
                2,
            );
            counts.record(w.deliver(reset, false, &[w.context], [0, 0, 1]));
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let sequences = w
                .portfolios
                .map(|key| w.env.portfolio_matcher_sequence(key));
            let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            let expiry = w.env.portfolio_matcher_expiry(w.portfolios[1]);
            let requests = w.env.market_state().0.matcher_req_seq;
            let controls = w.env.control_sequences(0);
            let reduction_ix = w.bundle_instructions(
                direct,
                -direction * reduction,
                -direction * reduction,
                OPEN_CAP,
            )[1]
            .clone();
            let mut close = w.bundle_instructions(
                Route::BatchCpi,
                -direction * residual,
                -direction * residual,
                OPEN_CAP,
            )[1]
            .clone();
            let mut request = ProgInstruction::decode(&close.data).unwrap();
            if let ProgInstruction::BatchTradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                account_b_matcher_sequence,
                max_slippage_atoms,
                max_fee_atoms,
                legs,
                ..
            } = &mut request
            {
                // Future epochs and sequence are signed fields, never state writes.
                *account_a_position_epoch += 1;
                *account_b_position_epoch += 1;
                *account_b_matcher_sequence += 1;
                assert_eq!(*max_slippage_atoms, 0);
                assert_eq!(*max_fee_atoms, signed_fees[2]);
                assert_eq!(legs.len(), 1);
                assert_eq!(legs[0].size_q, -direction * residual);
                assert_eq!(legs[0].limit_price, PRICE);
                assert_eq!(legs[0].fee_bps, u64::from(LP_CAP));
                assert!(legs[0].fee_bps > CLOSE_FRESH);
            } else {
                unreachable!();
            }
            close.data = request.encode();
            let mut revoked = close.clone();
            let mut short = close.clone();
            let mut revoked_request = request.clone();
            if let ProgInstruction::BatchTradeCpi {
                account_b_matcher_sequence,
                ..
            } = &mut revoked_request
            {
                *account_b_matcher_sequence -= 1;
            }
            revoked.data = revoked_request.encode();
            if let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut request {
                *max_fee_atoms -= 1;
            }
            short.data = request.encode();
            let renewal = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(w.owners[1].pubkey(), true),
                    AccountMeta::new_readonly(w.env.market, false),
                    AccountMeta::new(w.portfolios[1], false),
                    AccountMeta::new_readonly(w.matcher, false),
                    AccountMeta::new_readonly(w.context, false),
                    AccountMeta::new_readonly(w.delegate, false),
                ],
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id: w.env.portfolio_id(w.portfolios[1]),
                    expected_sequence: sequences[1],
                    position_epoch: epochs[1] + 1,
                    asset_generation_frontier: w.env.market_state().1.next_market_id,
                    enabled: 1,
                    trade_fee_cap_bps: LP_CAP,
                    expiry_slot: expiry,
                }
                .encode(),
            };
            let stale_policy = fee_control(
                &w,
                w.env.admin.pubkey(),
                OPEN_CAP,
                controls.trade_fee + 1,
                controls.authority_epoch,
            );
            let retained = [
                w.sign_with_nonce(&[reduction_ix.clone()], 10),
                w.sign_with_nonce(&[reduction_ix], 11),
                w.sign_with_nonce(&[revoked], 12),
                w.sign_with_nonce(&[renewal.clone(), close.clone()], 13),
                w.sign_with_nonce(&[renewal.clone(), short], 14),
                w.sign_with_nonce(&[renewal.clone(), close.clone(), stale_policy], 15),
                w.sign_with_nonce(&[renewal.clone(), close.clone()], 16),
                w.sign_with_nonce(&[renewal], 17),
                w.sign_with_nonce(&[close], 18),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            assert_eq!(retained[0].message.header.num_required_signatures, 3);
            assert_eq!(retained[2].message.header.num_required_signatures, 2);
            assert_eq!(retained[6].message.header.num_required_signatures, 3);
            let mut exact_message = retained[3].message.clone();
            exact_message.instructions[1] = retained[6].message.instructions[1].clone();
            assert_eq!(exact_message, retained[6].message);
            let mut short_message = retained[3].message.clone();
            short_message.instructions[1] = retained[4].message.instructions[1].clone();
            let mut cap = ProgInstruction::decode(&short_message.instructions[3].data).unwrap();
            if let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut cap {
                *max_fee_atoms -= 1;
            }
            short_message.instructions[3].data = cap.encode();
            assert_eq!(
                short_message, retained[4].message,
                "only cap and CU envelope differ"
            );
            probe(&mut w, &retained[1], true, 2, &mut counts);
            counts.record(policy(&mut w, CLOSE_FRESH, 20));
            book.check(&w);

            w.env.svm.warp_to_slot(1);
            counts.record(w.env.push_ewma_mark_with_cu(1, 98));
            let crank = w.sign_with_nonce(&[observed_crank(&w, 0, 1)], 21);
            counts.record(w.deliver(crank, false, &[w.env.market, w.portfolios[0]], [1, 0, 0]));
            book.collect(0, 1);
            book.check(&w);
            w.env.svm.warp_to_slot(2);
            counts.record(w.env.push_ewma_mark_with_cu(2, 101));
            counts.record(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            counts.record(policy(&mut w, OPEN_CAP, 22));
            counts.record(w.deliver(
                retained[1].clone(),
                false,
                &[w.env.market, w.portfolios[0], w.portfolios[1]],
                [1, 0, 0],
            ));
            counts.fills += 1;
            book.settled_funding = funding[0];
            book.reduce(reduction);
            book.collect(0, 2);
            book.collect(1, 2);
            book.check(&w);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
            assert_eq!(w.env.portfolio_matcher_expiry(w.portfolios[1]), 0);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                sequences[1]
            );
            assert_eq!(w.env.market_state().0.matcher_req_seq, requests);

            w.env.svm.warp_to_slot(3);
            counts.record(w.env.push_ewma_mark_with_cu(3, 98));
            let crank = w.sign_with_nonce(&[observed_crank(&w, 0, 3)], 23);
            counts.record(w.deliver(crank, false, &[w.env.market, w.portfolios[0]], [1, 0, 0]));
            book.collect(0, 3);
            book.check(&w);
            assert_eq!(w.env.market_state().1.assets[0].f_long_num, ADL_ONE as i128);
            assert_eq!(w.env.market_state().1.funding_epoch, 1);
            w.env.svm.warp_to_slot(4);
            counts.record(w.env.push_ewma_mark_with_cu(4, 101));
            book.check(&w);
            counts.record(w.deliver_with_error(
                retained[2].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::Unauthorized as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            counts.record(policy(&mut w, CLOSE_FRESH, 24));
            counts.record(w.deliver_with_error(
                retained[3].clone(),
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [1, 0, 1],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            counts.record(policy(&mut w, OPEN_CAP, 25));
            counts.record(w.deliver_with_error(
                retained[4].clone(),
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [1, 0, 1],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            // Exact consent executes renewal, funding and close before the stale
            // policy suffix rejects; the entire old disabled grant must return.
            counts.record(w.deliver_with_error(
                retained[5].clone(),
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [2, 0, 1],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                sequences[1]
            );
            assert_eq!(w.env.market_state().1.funding_epoch, 1);
            probe(&mut w, &retained[6], true, 2, &mut counts);
            counts.record(w.deliver(
                retained[6].clone(),
                false,
                &[w.env.market, w.portfolios[0], w.portfolios[1]],
                [2, 0, 1],
            ));
            counts.fills += 1;
            book.settled_funding += funding[1];
            book.reduce(residual);
            book.collect(0, 4);
            book.collect(1, 4);
            book.check(&w);
            assert_eq!(book.trade_fees, signed_fees.iter().sum::<u128>());
            assert_eq!(book.settled_funding, 133);
            let renewed = w.env.portfolio_matcher_config(w.portfolios[1]);
            assert_eq!(
                (
                    renewed.matcher_program,
                    renewed.matcher_context,
                    renewed.matcher_delegate
                ),
                (
                    grant.matcher_program,
                    grant.matcher_context,
                    grant.matcher_delegate
                )
            );
            assert_eq!(renewed.enabled(), 1);
            assert_eq!(renewed.trade_fee_cap_bps(), LP_CAP);
            assert_eq!(renewed.position_epoch(), epochs[1] + 2);
            assert_eq!(w.env.portfolio_matcher_expiry(w.portfolios[1]), expiry);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                sequences[1] + 1
            );
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[0]),
                sequences[0]
            );
            assert_eq!(w.env.market_state().0.matcher_req_seq, requests + 1);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|e| e + 2)
            );
            let (_, group) = w.env.market_state();
            assert_eq!(group.assets[0].f_long_num, 2 * ADL_ONE as i128);
            assert_eq!(group.assets[0].f_short_num, -2 * ADL_ONE as i128);
            assert_eq!(group.funding_epoch, 2);
            let mut final_controls = controls;
            final_controls.oracle_observation += 4;
            final_controls.trade_fee += 4;
            assert_eq!(w.env.control_sequences(0), final_controls);
            for tx in &retained[7..] {
                counts.record(w.deliver_with_error(
                    tx.clone(),
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )),
                    &[],
                    [0, 0, 0],
                ));
                counts.rollbacks += 1;
                book.check(&w);
            }
            for (tx, wire) in retained.iter().zip(wires) {
                tx.verify().unwrap();
                assert_eq!(bincode::serialize(tx).unwrap(), wire);
            }

            w.env.svm.warp_to_slot(5);
            for actor in 0..2 {
                let tx = w.sign_with_nonce(&[collect(&w, actor, 5)], 30 + actor as u32);
                counts.record(w.deliver(
                    tx,
                    false,
                    &[w.env.market, w.portfolios[actor]],
                    [1, 0, 0],
                ));
                book.collect(actor, 5);
                book.check(&w);
            }
            let winner = book.winner();
            let tx = w.sign_with_nonce(&[observed_crank(&w, winner, 5)], 32);
            counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[winner]], [1, 0, 0]));
            book.check(&w);
            let tx = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.owners[winner].pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                        AccountMeta::new(w.portfolios[winner], false),
                    ],
                    data: w
                        .env
                        .convert_released_pnl_ix(w.portfolios[winner], 133)
                        .encode(),
                }],
                33,
            );
            counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[winner]], [1, 0, 0]));
            book.converted = true;
            book.check(&w);
            for actor in 0..2 {
                let amount = book.capital(actor);
                let tx = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.owners[actor].pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                            AccountMeta::new(w.portfolios[actor], false),
                            AccountMeta::new(w.tokens[actor], false),
                            AccountMeta::new(w.env.vault, false),
                            AccountMeta::new_readonly(w.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: w
                            .env
                            .withdraw_ix(w.portfolios[actor], amount.into())
                            .encode(),
                    }],
                    34 + actor as u32,
                );
                counts.record(w.deliver(
                    tx,
                    false,
                    &[
                        w.env.market,
                        w.portfolios[actor],
                        w.tokens[actor],
                        w.env.vault,
                    ],
                    [1, 1, 0],
                ));
                book.paid[actor] = amount;
                counts.payouts += 1;
                book.check(&w);
            }
            let outcome = (book.paid, w.env.token_amount(w.env.vault));
            assert_eq!(outcome.1, 3_216);
            assert_eq!(
                outcome.0,
                if direction < 0 {
                    [98_375, 198_532]
                } else {
                    [98_641, 198_266]
                }
            );
            if let Some(expected) = endpoint {
                assert_eq!(
                    outcome, expected,
                    "bilateral transports preserve every payout"
                );
            } else {
                endpoint = Some(outcome);
            }
            counts.worlds += 1;
        }
    }
    assert_eq!(
        (
            counts.worlds,
            counts.rollbacks,
            counts.fills,
            counts.payouts
        ),
        (4, 28, 12, 8)
    );
    assert_eq!(counts.permitted, 8);
    eprintln!("INV-014 retained funded route renewal: {counts:?}");
}
