//! Row 432: retained CPI fee consent composed with an authority A -> B -> A.
//! A forward-gap fee policy from A's old epoch cannot restore a permitted rate
//! before a retained fill, or persist after that fill. Epoch-only policy renewal
//! and a byte-identical trade-only envelope provide distinct live controls.
//! Public System/SPL/wrapper/authenticated-matcher paths; bounded full fills.

use super::*;

fn fee_control(w: &World, authority: Pubkey, bps: u64, sequence: u64, epoch: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(authority, true),
            AccountMeta::new(w.env.market, false),
        ],
        data: ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: bps,
            policy_sequence: sequence,
            authority_epoch: epoch,
        }
        .encode(),
    }
}

fn handoff(w: &mut World, current: Pubkey, next: Pubkey, nonce: u32) -> u64 {
    let mut expected = w.env.control_sequences(0);
    let tx = w.sign(
        &[Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(current, true),
                AccountMeta::new(next, true),
                AccountMeta::new(w.env.market, false),
            ],
            data: ProgInstruction::UpdateAuthority {
                authority_epoch: expected.authority_epoch,
                new_pubkey: next.to_bytes(),
            }
            .encode(),
        }],
        nonce,
    );
    let cu = w.deliver(tx, false, &[w.env.market], [1, 0, 0]);
    expected.authority_epoch += 1;
    assert_eq!(w.env.control_sequences(0), expected);
    assert_eq!(w.env.market_state().0.marketauth, next.to_bytes());
    assert_eq!(
        state::read_asset_oracle_profile(&w.env.svm.get_account(&w.env.market).unwrap().data, 0)
            .unwrap()
            .insurance_authority,
        next.to_bytes(),
        "the inherited fee authority follows both handoffs"
    );
    w.check(0, 0);
    cu
}

#[test]
fn v16_retained_cpi_fee_terms_survive_authority_aba_and_stale_policy_bundles() {
    const SIGNED_BPS: u64 = 37;
    const GAP_BPS: u64 = 31;
    const RESTORED_BPS: u64 = 23;
    const HIGH_BPS: u64 = 41;
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    let mut peaks = [0; 4]; // Simulation, controls, rollback, committed fill.
    let (mut worlds, mut simulations, mut rollbacks, mut retained_fills, mut renewed_fills) =
        (0, 0, 0, 0, 0);
    assert!(HIGH_BPS > SIGNED_BPS && HIGH_BPS < u64::from(LP_CAP_BPS));
    assert!(fee(RESTORED_BPS) < fee(GAP_BPS) && fee(GAP_BPS) < fee(SIGNED_BPS));

    for direction in [-1, 1] {
        for renew_policy in [false, true] {
            let mut w = World::new(&matcher_bytes);
            let authority_a = w.env.admin.pubkey();
            // The LP is the temporary policy authority, but never signs the
            // retained trade-only transaction or receives taker fee discretion.
            let authority_b = w.owners[1].pubkey();
            let initial = w.env.control_sequences(0);
            let gap_sequence = initial.trade_fee + 7;
            let size = direction * QUANTITY;
            let trade =
                w.env
                    .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, size, SIGNED_BPS, PRICE);
            let [deposit, fill] = w.bundle_instructions(&trade);
            let old_policy = fee_control(
                &w,
                authority_a,
                GAP_BPS,
                gap_sequence,
                initial.authority_epoch,
            );
            let prefix_ixs = [deposit.clone(), old_policy.clone(), fill.clone()];
            let suffix_ixs = [deposit.clone(), fill.clone(), old_policy];
            let trade_ixs = [deposit, fill];
            let retained = [
                w.sign(&prefix_ixs, 0),
                w.sign(&suffix_ixs, 1),
                w.sign(&trade_ixs, 2),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for (tx, signatures) in retained.iter().zip([3, 3, 2]) {
                assert_eq!(tx.message.header.num_required_signatures, signatures);
                assert!(!tx.message.account_keys[..signatures as usize].contains(&authority_b));
                peaks[0] = peaks[0].max(w.simulate(tx));
                simulations += 1;
            }
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let grant_expiry = w.env.portfolio_matcher_expiry(w.portfolios[1]);
            let request_sequence = w.env.market_state().0.matcher_req_seq;

            peaks[1] = peaks[1].max(handoff(&mut w, authority_a, authority_b, 10));
            let high = fee_control(
                &w,
                authority_b,
                HIGH_BPS,
                initial.trade_fee + 1,
                initial.authority_epoch + 1,
            );
            let tx = w.sign(&[high], 11);
            peaks[1] = peaks[1].max(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
            w.check(0, 0);
            peaks[1] = peaks[1].max(handoff(&mut w, authority_b, authority_a, 12));
            let mut controls = initial;
            controls.authority_epoch += 2;
            controls.trade_fee += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, HIGH_BPS);
            assert!(gap_sequence > controls.trade_fee);

            // A is the live signer again and sequence +7 is still ahead. Only
            // the old authority epoch disqualifies the otherwise usable policy.
            assert_eq!(w.sign(&prefix_ixs, 0), retained[0]);
            assert_eq!(bincode::serialize(&retained[0]).unwrap(), wires[0]);
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [1, 1, 0],
            ));
            rollbacks += 1;
            w.check(0, 0);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, HIGH_BPS);

            let mut fresh_ixs = prefix_ixs.clone();
            fresh_ixs[1] = fee_control(
                &w,
                authority_a,
                GAP_BPS,
                gap_sequence,
                controls.authority_epoch,
            );
            let renewed = w.sign(&fresh_ixs, 0);
            let mut expected_message = retained[0].message.clone();
            expected_message.instructions[3].data = fresh_ixs[1].data.clone();
            assert_eq!(renewed.message, expected_message);
            assert_ne!(renewed.signatures, retained[0].signatures);
            peaks[0] = peaks[0].max(w.simulate(&renewed));
            simulations += 1;

            peaks[1] = peaks[1].max(w.policy(RESTORED_BPS, 13));
            controls.trade_fee += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            assert!(gap_sequence > controls.trade_fee);
            assert_eq!(w.sign(&suffix_ixs, 1), retained[1]);
            assert_eq!(bincode::serialize(&retained[1]).unwrap(), wires[1]);
            // Now the unchanged trade succeeds before the obsolete policy
            // suffix fails. Full Account rollback includes the matcher return.
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[1].clone(),
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [2, 1, 1],
            ));
            rollbacks += 1;
            w.check(0, 0);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, RESTORED_BPS);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs
            );
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                grant_sequence
            );
            assert_eq!(
                w.env.portfolio_matcher_expiry(w.portfolios[1]),
                grant_expiry
            );

            assert_eq!(w.sign(&trade_ixs, 2), retained[2]);
            assert_eq!(bincode::serialize(&retained[2]).unwrap(), wires[2]);
            peaks[0] = peaks[0].max(w.simulate(&retained[2]));
            simulations += 1;
            let (accepted, bps, wrapper_calls) = if renew_policy {
                renewed_fills += 1;
                controls.trade_fee = gap_sequence;
                (renewed, GAP_BPS, 3)
            } else {
                retained_fills += 1;
                (retained[2].clone(), RESTORED_BPS, 2)
            };
            peaks[3] = peaks[3].max(w.deliver(
                accepted,
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.env.vault,
                    w.sources[0],
                    w.context,
                ],
                [wrapper_calls, 1, 1],
            ));
            w.check(size, bps);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, bps);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence + 1);
            for (key, epoch) in w.portfolios.into_iter().zip(epochs) {
                assert_eq!(w.env.portfolio_position_epoch(key), epoch + 1);
            }
            let mut expected_grant = grant;
            expected_grant.control = state::next_portfolio_position_control(grant.control)
                .unwrap()
                .1;
            assert_eq!(
                w.env.portfolio_matcher_config(w.portfolios[1]),
                expected_grant
            );
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                grant_sequence
            );
            assert_eq!(
                w.env.portfolio_matcher_expiry(w.portfolios[1]),
                grant_expiry
            );
            worlds += 1;
        }
    }
    assert_eq!(
        (
            worlds,
            simulations,
            rollbacks,
            retained_fills,
            renewed_fills
        ),
        (4, 20, 8, 2, 2)
    );
    eprintln!("INV-014 row432 authority ABA: worlds={worlds}, simulations={simulations}, exact_rollbacks={rollbacks}, retained_fills={retained_fills}, epoch_renewed_policy_fills={renewed_fills}; max CU simulation={}, controls={}, rollback={}, fill={}", peaks[0], peaks[1], peaks[2], peaks[3]);
}
