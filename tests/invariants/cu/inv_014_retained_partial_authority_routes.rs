//! Row 411: a committed partial fill followed by authority A -> B -> A and
//! retained single/batch continuation. INV-005/010/011/014/024/036/047 evidence.
//! Reuses the partial-policy world's public setup, full frames and input ledger.

use super::*;

#[path = "inv_014_retained_funding_collection.rs"]
mod retained_funding_collection;

fn fee_control(w: &World, signer: Pubkey, rate: u64, sequence: u64, epoch: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(w.env.market, false),
        ],
        data: ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: rate,
            policy_sequence: sequence,
            authority_epoch: epoch,
        }
        .encode(),
    }
}

fn handoff(w: &mut World, from: Pubkey, to: Pubkey, nonce: u32) -> u64 {
    let mut controls = w.env.control_sequences(0);
    let tx = w.sign_with_nonce(
        &[Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(from, true),
                AccountMeta::new(to, true),
                AccountMeta::new(w.env.market, false),
            ],
            data: ProgInstruction::UpdateAuthority {
                authority_epoch: controls.authority_epoch,
                new_pubkey: to.to_bytes(),
            }
            .encode(),
        }],
        nonce,
    );
    let cu = w.deliver(tx, false, &[w.env.market], [1, 0, 0]);
    controls.authority_epoch += 1;
    assert_eq!(w.env.control_sequences(0), controls);
    assert_eq!(w.env.market_state().0.marketauth, to.to_bytes());
    assert_eq!(
        state::read_asset_oracle_profile(&w.env.svm.get_account(&w.env.market).unwrap().data, 0)
            .unwrap()
            .insurance_authority,
        to.to_bytes()
    );
    cu
}

#[test]
fn v16_retained_partial_fill_authority_return_preserves_four_route_fee_consent() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    for direction in [-1, 1] {
        let requested = direction * REQUEST;
        let executed = requested * 95 / 255;
        let charged = fee(executed, OPEN_CAP);
        assert!(charged > 0 && executed.unsigned_abs() < requested.unsigned_abs());
        assert!(fee(executed, CLOSE_FRESH) > charged);
        assert!(CLOSE_FRESH < u64::from(LP_CAP));
        let mut endpoint = None;
        for route in [
            Route::SingleCpi,
            Route::SingleNoCpi,
            Route::BatchCpi,
            Route::BatchNoCpi,
        ] {
            eprintln!("partial authority return: direction={direction}, route={route:?}");
            let mut w = World::new(&matcher);
            let mut book = Book::new(&w);
            let authority_a = w.env.admin.pubkey();
            let authority_b = w.owners[1].pubkey();
            counts.record(policy(&mut w, OPEN_CAP, 1));
            counts.record(w.partial(95));
            let opening = w.bundle_instructions(Route::PartialCpi, requested, executed, OPEN_CAP);
            let open = w.sign_with_nonce(&opening, 2);
            let consumed_open = w.sign_with_nonce(&[opening[1].clone()], 3);
            let changed = [
                w.env.market,
                w.portfolios[0],
                w.portfolios[1],
                w.tokens[0],
                w.env.vault,
                w.context,
            ];
            counts.record(w.deliver(open, false, &changed, [2, 1, 1]));
            counts.fills += 1;
            book.position = executed;
            book.fees = charged;
            book.deposited = true;
            book.fills = 1;
            book.cpi_fills = 1;
            book.check(&w);
            let fill =
                read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
            assert_eq!((fill.exec_size, fill.exec_price_e6), (executed, PRICE));
            assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);

            let exact = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(authority_b, true),
                        AccountMeta::new(w.context, false),
                    ],
                    data: vec![11, 9, 0],
                }],
                4,
            );
            let context = w.context;
            counts.record(w.deliver(exact, false, &[context], [0, 0, 1]));
            let initial = w.env.control_sequences(0);
            let profile = state::read_asset_oracle_profile(
                &w.env.svm.get_account(&w.env.market).unwrap().data,
                0,
            )
            .unwrap();
            let grant = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let close = w.bundle_instructions(route, -executed, -executed, OPEN_CAP)[1].clone();
            let old_policy = fee_control(
                &w,
                authority_a,
                OPEN_CAP,
                initial.trade_fee + 7,
                initial.authority_epoch,
            );
            let prefix_ixs = [old_policy.clone(), close.clone()];
            let suffix_ixs = [close.clone(), old_policy];
            let retained = [
                w.sign_with_nonce(&prefix_ixs, 10),
                w.sign_with_nonce(&suffix_ixs, 11),
                w.sign_with_nonce(&[close.clone()], 12),
                w.sign_with_nonce(&[close.clone()], 13),
                w.sign_with_nonce(&[close.clone()], 14),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            assert_eq!(
                retained[2].message.header.num_required_signatures,
                if route.cpi() { 2 } else { 3 }
            );
            assert_eq!(
                retained[2].message.account_keys
                    [..retained[2].message.header.num_required_signatures as usize]
                    .contains(&authority_b),
                !route.cpi()
            );
            for index in [0, 1, 3] {
                probe(&mut w, &retained[index], true, 2, &mut counts);
            }
            let mut retry_message = retained[2].message.clone();
            retry_message.instructions[1] = retained[3].message.instructions[1].clone();
            assert_eq!(
                retry_message, retained[3].message,
                "same pre-signed trade, distinct transaction envelope"
            );
            book.check(&w);

            counts.record(handoff(&mut w, authority_a, authority_b, 20));
            book.check(&w);
            let high_policy = fee_control(
                &w,
                authority_b,
                CLOSE_FRESH,
                initial.trade_fee + 1,
                initial.authority_epoch + 1,
            );
            let high = w.sign_with_nonce(&[high_policy], 21);
            let market = w.env.market;
            counts.record(w.deliver(high, false, &[market], [1, 0, 0]));
            book.check(&w);
            counts.record(handoff(&mut w, authority_b, authority_a, 22));
            let mut controls = initial;
            controls.authority_epoch += 2;
            controls.trade_fee += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, CLOSE_FRESH);
            assert_eq!(
                state::read_asset_oracle_profile(&w.env.svm.get_account(&market).unwrap().data, 0)
                    .unwrap(),
                profile,
                "same authority profile, different epoch"
            );
            book.check(&w);

            // The sequence is still usable and A is current again. Only the
            // obsolete epoch prevents restoring the signed fee before the close.
            assert!(initial.trade_fee + 7 > controls.trade_fee);
            counts.record(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            let mut renewed_ixs = prefix_ixs.clone();
            renewed_ixs[0] = fee_control(
                &w,
                authority_a,
                OPEN_CAP,
                initial.trade_fee + 7,
                controls.authority_epoch,
            );
            let renewed = w.sign_with_nonce(&renewed_ixs, 10);
            let mut expected = retained[0].message.clone();
            expected.instructions[2].data = renewed_ixs[0].data.clone();
            assert_eq!(
                renewed.message, expected,
                "only policy authority epoch renewed"
            );
            assert_ne!(renewed.signatures, retained[0].signatures);
            probe(&mut w, &renewed, true, 2, &mut counts);
            probe(&mut w, &retained[2], false, 2, &mut counts);
            counts.record(w.deliver_with_error(
                retained[2].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [0, 0, usize::from(route == Route::BatchCpi)],
            ));
            counts.rollbacks += 1;
            book.check(&w);
            consumed_retry(&mut w, &consumed_open, &mut counts);
            book.check(&w);

            counts.record(policy(&mut w, OPEN_CAP, 23));
            controls.trade_fee += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            let calls = [1, 0, usize::from(route.cpi())];
            // A successful close and fee collection precede the stale suffix.
            counts.record(w.deliver_with_error(
                retained[1].clone(),
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                calls,
            ));
            counts.rollbacks += 1;
            book.check(&w);
            assert_eq!(w.env.control_sequences(0), controls);
            probe(&mut w, &retained[3], true, 2, &mut counts);
            // The alternative was signed before the handoffs. Failed delivery
            // signatures remain recorded, so only its envelope distinguishes it.
            assert_eq!(w.sign_with_nonce(&[close], 13), retained[3]);
            let mut changed = vec![market, w.portfolios[0], w.portfolios[1]];
            if route.cpi() {
                changed.push(context);
            }
            counts.record(w.deliver(retained[3].clone(), false, &changed, calls));
            counts.fills += 1;
            book.position = 0;
            book.fees += charged;
            book.fills += 1;
            book.cpi_fills += u64::from(route.cpi());
            book.check(&w);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), grant);
            assert_eq!(
                w.env.portfolio_matcher_config(w.portfolios[1]).enabled(),
                u64::from(route.cpi())
            );
            consumed_retry(&mut w, &retained[4], &mut counts);
            book.check(&w);
            for (tx, wire) in retained.iter().zip(wires) {
                tx.verify().unwrap();
                assert_eq!(bincode::serialize(tx).unwrap(), wire);
            }
            for actor in 0..2 {
                let amount = book.entitlement(actor);
                let payout = w.sign_with_nonce(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.owners[actor].pubkey(), true),
                            AccountMeta::new(market, false),
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
                    30 + actor as u32,
                );
                let changed = [market, w.portfolios[actor], w.tokens[actor], w.env.vault];
                counts.record(w.deliver(payout, false, &changed, [1, 1, 0]));
                counts.payouts += 1;
                book.paid[actor] = amount;
                book.check(&w);
            }
            let outcome = (book.paid, book.fees, w.env.token_amount(w.env.vault));
            if let Some(expected) = endpoint {
                assert_eq!(
                    outcome, expected,
                    "all continuation routes preserve owner value"
                );
            } else {
                endpoint = Some(outcome);
            }
            counts.worlds += 1;
        }
    }
    assert_eq!((counts.worlds, counts.permitted, counts.denied), (8, 40, 8));
    assert_eq!(
        (counts.rollbacks, counts.fills, counts.payouts),
        (40, 16, 16)
    );
    eprintln!("INV-014 partial authority return: {counts:?}");
}
