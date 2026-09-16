//! INV-014 / row 411: a retained residual batch has its own gross fee cap after
//! a paid single-CPI partial and a live policy increase. Prior insurance credits
//! neither subsidize that cap nor consume it again. Unlike the parent's partial
//! opening/exact-close histories, this completes the original requested quantity
//! through batch CPI, with one-atom boundary controls and a committed fee prefix.
//! INV-009's residual route matrix holds policy fixed; the mixed-route and policy
//! budget tests have no actual partial. One asset, base fees, public SPL paths.

use super::*;

#[test]
fn v16_retained_residual_batch_cap_excludes_committed_partial_fees_after_repricing() {
    const OPEN_BPS: u64 = 19;
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    for direction in [-1, 1] {
        for numerator in [64u8, 191] {
            let request = direction * REQUEST;
            let partial = request * i128::from(numerator) / 255;
            let residual = request - partial;
            let paid = fee(partial, OPEN_BPS);
            let cap = fee(residual, CURRENT_BPS);
            assert!(0 < partial.unsigned_abs() && partial.unsigned_abs() < REQUEST as u128);
            assert!(fee(residual, OPEN_BPS) <= cap - 1);
            assert!(cap.saturating_sub(paid) <= cap - 1);
            assert!(paid + cap > cap, "the cap excludes previously paid fees");
            assert_ne!(paid + cap, fee(REQUEST, OPEN_BPS));
            assert_ne!(paid + cap, fee(REQUEST, CURRENT_BPS));

            let mut w = World::new(&matcher);
            let mut book = Book::new(&w);
            let mut controls = w.env.control_sequences(0);
            let mut grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            book.check(&w);
            counts.record(w.partial(numerator));
            book.check(&w);
            let opening =
                w.bundle_instructions(Route::PartialCpi, request, partial, OPEN_BPS)[1].clone();
            let retry = w.sign_with_nonce(&[opening.clone()], 10);
            let retry_wire = bincode::serialize(&retry).unwrap();
            probe(&mut w, &retry, true, 2, &mut counts);
            book.check(&w);
            let tx = w.sign_with_nonce(&[opening], 11);
            let changed = [w.env.market, w.portfolios[0], w.portfolios[1], w.context];
            counts.record(w.deliver(tx, false, &changed, [1, 0, 1]));
            counts.fills += 1;
            book.position = partial;
            book.fees = paid;
            book.fills = 1;
            book.cpi_fills = 1;
            book.check(&w);
            grant.set_position_epoch(book.epochs[1] + 1).unwrap();
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
            let fill =
                read_matcher_return(&w.env.svm.get_account(&w.context).unwrap().data).unwrap();
            assert_eq!((fill.exec_size, fill.exec_price_e6), (partial, PRICE));
            assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);

            // Restore full matcher capacity publicly. The residual is new consent
            // for current episodes; the original request remains consumed.
            let tx = w.sign_with_nonce(
                &[Instruction {
                    program_id: w.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                        AccountMeta::new(w.context, false),
                    ],
                    data: vec![11, 9, 0],
                }],
                12,
            );
            let context = w.context;
            counts.record(w.deliver(tx, false, &[context], [0, 0, 1]));
            book.check(&w);
            let exact = w.bundle_instructions(Route::BatchCpi, residual, residual, CURRENT_BPS);
            let mut short = exact.clone();
            let mut instruction = ProgInstruction::decode(&short[1].data).unwrap();
            let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut instruction else {
                unreachable!()
            };
            assert_eq!(*max_fee_atoms, cap);
            *max_fee_atoms -= 1;
            short[1].data = instruction.encode();
            let mut late = exact.to_vec();
            late.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &w.tokens[0],
                    &w.env.vault,
                    &w.owners[0].pubkey(),
                    &[],
                    1,
                )
                .unwrap(),
            );
            let retained = [
                w.sign_with_nonce(&short, 20),
                w.sign_with_nonce(&late, 21),
                w.sign_with_nonce(&exact, 22),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for tx in &retained {
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(!tx.message.account_keys[..2].contains(&w.owners[1].pubkey()));
            }
            for index in [0, 2] {
                probe(&mut w, &retained[index], true, 3, &mut counts);
                book.check(&w);
            }
            counts.record(policy(&mut w, CURRENT_BPS, 30));
            controls.trade_fee += 1;
            book.check(&w);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);

            consumed_retry(&mut w, &retry, &mut counts);
            book.check(&w);
            // Both failures retain the already-committed partial and its insurance.
            // First the funded residual reaches the matcher but exceeds its cap;
            // then the exact residual executes before a deliberately failing SPL suffix.
            counts.record(w.deliver(retained[0].clone(), true, &[], [1, 1, 1]));
            counts.rollbacks += 1;
            book.check(&w);
            counts.record(w.deliver_with_error(
                retained[1].clone(),
                Some((
                    4,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                )),
                &[],
                [2, 1, 1],
            ));
            counts.rollbacks += 1;
            book.check(&w);

            // The original exact-cap envelope still succeeds, although cumulative
            // fees now exceed its cap. Rejection consumed neither epochs nor fees.
            let changed = [
                w.env.market,
                w.portfolios[0],
                w.portfolios[1],
                w.context,
                w.tokens[0],
                w.env.vault,
            ];
            counts.record(w.deliver(retained[2].clone(), false, &changed, [2, 1, 1]));
            counts.fills += 1;
            book.position = request;
            book.fees += cap;
            book.deposited = true;
            book.fills += 1;
            book.cpi_fills += 1;
            book.check(&w);
            grant.set_position_epoch(book.epochs[1] + 2).unwrap();
            assert_eq!(book.fees, paid + cap);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                grant_sequence
            );
            for (tx, wire) in retained.iter().zip(&wires).chain([(&retry, &retry_wire)]) {
                tx.verify().unwrap();
                assert_eq!(&bincode::serialize(tx).unwrap(), wire);
            }
            counts.worlds += 1;
        }
    }
    assert_eq!((counts.worlds, counts.permitted, counts.denied), (4, 12, 0));
    assert_eq!((counts.rollbacks, counts.fills), (12, 8));
    eprintln!("INV-014 row411 retained residual batch cap: {counts:?}");
}
