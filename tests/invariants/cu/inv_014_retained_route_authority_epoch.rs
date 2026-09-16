//! INV-005/014/024/047, row 411: retained route switching after authority ABA.
//! Unlike the flat single-CPI ABA and authority-stable mixed-route products,
//! a stale policy suffix here rolls back a bilateral reduction's fee debit AND
//! LP grant revocation while preserving previously earned insurance. A retained
//! grant renewal then crosses back to CPI under the taker's original rate cap.
//! All state transitions use public System/SPL/wrapper/authenticated-matcher paths.

use super::*;

#[test]
fn v16_retained_route_switch_after_authority_aba_preserves_fees_and_grant_rollback() {
    const HIGH_BPS: u64 = 41;
    const RESTORED_BPS: u64 = 23;
    let matcher = std::fs::read(auth_matcher_program_path()).unwrap();
    let signed_budget =
        fee(CPI_CAP) + fee_atoms(REDUCTION, DIRECT_RATE) + fee_atoms(QUANTITY - REDUCTION, CPI_CAP);
    let fees = [
        fee(OLD_BPS),
        fee_atoms(REDUCTION, DIRECT_RATE),
        fee_atoms(QUANTITY - REDUCTION, RESTORED_BPS),
    ];
    assert_eq!(fees, [49, 100, 36]);
    assert!(CPI_CAP < HIGH_BPS && HIGH_BPS < DIRECT_RATE);
    assert!(HIGH_BPS < u64::from(LP_CAP_BPS));
    let mut peaks = [0; 4]; // Simulation, authority/policy, rollback, committed routes.
    let (mut worlds, mut simulations, mut rollbacks) = (0, 0, 0);
    for direction in [-1, 1] {
        for direct_batch in [false, true] {
            let mut w = World::new(&matcher);
            let a = w.env.admin.pubkey();
            let b = w.owners[1].pubkey();
            let initial = w.env.control_sequences(0);
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let expiry = w.env.portfolio_matcher_expiry(w.portfolios[1]);
            let requests = w.env.market_state().0.matcher_req_seq;
            let sizes = [
                direction * QUANTITY,
                -direction * REDUCTION,
                -direction * (QUANTITY - REDUCTION),
            ];
            let opening = trade(&w, false, 0, sizes[0]);
            let reduction = trade(&w, direct_batch, 1, sizes[1]);
            let closing = trade(&w, false, 2, sizes[2]);
            let deposit = w.bundle_instructions(&w.env.trade_cpi_ix(
                w.portfolios[0],
                w.portfolios[1],
                0,
                sizes[0],
                CPI_CAP,
                PRICE,
            ))[0]
                .clone();
            let old_policy = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(a, true),
                    AccountMeta::new(w.env.market, false),
                ],
                data: ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: RESTORED_BPS,
                    policy_sequence: initial.trade_fee + 7,
                    authority_epoch: initial.authority_epoch,
                }
                .encode(),
            };
            let suffix_ixs = [deposit.clone(), reduction.clone(), old_policy.clone()];
            let direct_ixs = [deposit, reduction];
            // Future position epochs and grant sequence are signed instructions,
            // not injected account state. The bilateral reduction revokes the LP.
            let renewal = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(b, true),
                    AccountMeta::new_readonly(w.env.market, false),
                    AccountMeta::new(w.portfolios[1], false),
                    AccountMeta::new_readonly(w.matcher, false),
                    AccountMeta::new_readonly(w.context, false),
                    AccountMeta::new_readonly(w.delegate, false),
                ],
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id: w.env.portfolio_id(w.portfolios[1]),
                    expected_sequence: sequence,
                    position_epoch: epochs[1] + 2,
                    asset_generation_frontier: w.env.market_state().1.next_market_id,
                    enabled: 1,
                    trade_fee_cap_bps: LP_CAP_BPS,
                    expiry_slot: expiry,
                }
                .encode(),
            };
            let close_ixs = [renewal, closing];
            let retained = [
                w.sign(&[opening], 0),
                w.sign(&suffix_ixs, 1),
                w.sign(&direct_ixs, 2),
                w.sign(&close_ixs, 3),
                w.sign(&close_ixs, 4),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            assert_consent(&w, &retained[0], false, 0, sizes[0]);
            assert_consent(&w, &retained[2], direct_batch, 1, sizes[1]);
            for tx in &retained[3..] {
                assert_eq!(tx.message.header.num_required_signatures, 3);
                assert!(!tx.message.account_keys[..3].contains(&a));
                let ProgInstruction::TradeCpi { fee_bps, .. } =
                    ProgInstruction::decode(&tx.message.instructions[3].data).unwrap()
                else {
                    unreachable!()
                };
                assert_eq!(fee_bps, CPI_CAP);
            }

            let mut ledger = Ledger::default();
            ledger.check(&w, signed_budget);
            peaks[0] = peaks[0].max(w.simulate(&retained[0]));
            simulations += 1;
            peaks[3] = peaks[3].max(w.deliver(
                retained[0].clone(),
                false,
                &[w.env.market, w.portfolios[0], w.portfolios[1], w.context],
                [1, 0, 1],
            ));
            ledger.size = sizes[0];
            ledger.fees = fees[0];
            ledger.check(&w, signed_budget);
            let opening_grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            // The exact obsolete bundle is admissible before the handoffs.
            peaks[0] = peaks[0].max(w.simulate(&retained[1]));
            simulations += 1;

            let mut controls = initial;
            for (step, (current, next)) in [(a, b), (b, a)].into_iter().enumerate() {
                let tx = w.sign(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(current, true),
                            AccountMeta::new(next, true),
                            AccountMeta::new(w.env.market, false),
                        ],
                        data: ProgInstruction::UpdateAuthority {
                            authority_epoch: controls.authority_epoch,
                            new_pubkey: next.to_bytes(),
                        }
                        .encode(),
                    }],
                    10 + step as u32,
                );
                peaks[1] = peaks[1].max(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
                controls.authority_epoch += 1;
                assert_eq!(w.env.market_state().0.marketauth, next.to_bytes());
                assert_eq!(w.env.control_sequences(0), controls);
                ledger.check(&w, signed_budget);
                if step == 0 {
                    let mut high = old_policy.clone();
                    high.accounts[0].pubkey = b;
                    high.data = ProgInstruction::UpdateTradeFeePolicy {
                        trade_fee_base_bps: HIGH_BPS,
                        policy_sequence: initial.trade_fee + 1,
                        authority_epoch: controls.authority_epoch,
                    }
                    .encode();
                    let tx = w.sign(&[high], 12);
                    peaks[1] = peaks[1].max(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
                    controls.trade_fee += 1;
                    ledger.check(&w, signed_budget);
                }
            }
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, HIGH_BPS);
            assert!(initial.trade_fee + 7 > controls.trade_fee);
            // A is current again and the policy sequence is still ahead. The
            // stale epoch alone rejects AFTER the deposit and bilateral fill.
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[1].clone(),
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [2, 1, 0],
            ));
            rollbacks += 1;
            ledger.check(&w, signed_budget);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(
                w.env.portfolio_matcher_config(w.portfolios[1]),
                opening_grant
            );
            assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
            assert_eq!(w.env.portfolio_matcher_expiry(w.portfolios[1]), expiry);
            assert_eq!(w.env.market_state().0.matcher_req_seq, requests + 1);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|epoch| epoch + 1)
            );

            // Epoch-only renewal is a positive control for the failing suffix.
            let mut renewed_ixs = suffix_ixs.clone();
            renewed_ixs[2].data = ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: RESTORED_BPS,
                policy_sequence: initial.trade_fee + 7,
                authority_epoch: controls.authority_epoch,
            }
            .encode();
            let renewed = w.sign(&renewed_ixs, 1);
            let mut expected = retained[1].message.clone();
            expected.instructions[4].data = renewed_ixs[2].data.clone();
            assert_eq!(renewed.message, expected);
            assert_ne!(renewed.signatures, retained[1].signatures);
            peaks[0] = peaks[0].max(w.simulate(&renewed));
            simulations += 1;
            // Commit the independently pre-signed direct-only envelope at 99
            // bps, while B's surviving live policy is 41 bps, above the CPI cap.
            peaks[3] = peaks[3].max(w.deliver(
                retained[2].clone(),
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.sources[0],
                    w.env.vault,
                ],
                [2, 1, 0],
            ));
            ledger.size += sizes[1];
            ledger.fees += fees[1];
            ledger.deposited = PREFIX;
            ledger.check(&w, signed_budget);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, HIGH_BPS);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
            assert_eq!(w.env.portfolio_matcher_expiry(w.portfolios[1]), 0);
            assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);

            // Renewing the LP's permissive grant cannot widen the taker's cap.
            // Rejection also rolls back the successful renewal and its sequence.
            peaks[2] = peaks[2].max(w.deliver(retained[3].clone(), true, &[], [1, 0, 0]));
            rollbacks += 1;
            ledger.check(&w, signed_budget);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 0);
            assert_eq!(w.env.portfolio_matcher_sequence(w.portfolios[1]), sequence);
            peaks[1] = peaks[1].max(w.policy(RESTORED_BPS, 13));
            controls.trade_fee += 1;
            peaks[0] = peaks[0].max(w.simulate(&retained[4]));
            simulations += 1;
            peaks[3] = peaks[3].max(w.deliver(
                retained[4].clone(),
                false,
                &[w.env.market, w.portfolios[0], w.portfolios[1], w.context],
                [2, 0, 1],
            ));
            ledger.size += sizes[2];
            ledger.fees += fees[2];
            ledger.check(&w, signed_budget);
            assert_eq!((ledger.size, ledger.fees), (0, 185));
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.matcher_req_seq, requests + 2);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                sequence + 1
            );
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]).enabled(), 1);
            assert_eq!(w.env.portfolio_matcher_expiry(w.portfolios[1]), expiry);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|epoch| epoch + 3)
            );
            for (tx, wire) in retained.iter().zip(&wires) {
                tx.verify().unwrap();
                assert_eq!(&bincode::serialize(tx).unwrap(), wire);
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, simulations, rollbacks), (4, 16, 8));
    eprintln!("INV-005/014/024/047 row411 route authority ABA: worlds={worlds}, simulations={simulations}, exact_account_rollbacks={rollbacks}, peak_CU_simulation_controls_rollback_routes={peaks:?}");
}
