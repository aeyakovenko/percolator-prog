//! INV-005/008/010/011/012/014: retained closes across a funded oracle handoff.
//! Trading funds the incumbent's insurance role before the LP, acting as cold
//! admin, acquires observation power. Neither that role nor the LP's permissive
//! grant expands the absent taker's signed closing fee cap.

use super::*;

fn role(w: &World, kind: u8, epoch: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.owners[1].pubkey(), true),
            AccountMeta::new(w.owners[1].pubkey(), true),
            AccountMeta::new(w.env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: w.env.asset_market_id(0),
            authority_epoch: epoch,
            kind,
            new_pubkey: w.owners[1].pubkey().to_bytes(),
        }
        .encode(),
    }
}

#[test]
fn v16_retained_close_preserves_fee_consent_after_funded_oracle_role_handoff() {
    const HIGH_BPS: u64 = 37;
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    assert!(OLD_BPS < HIGH_BPS && HIGH_BPS < u64::from(LP_CAP_BPS));
    assert_eq!((fee(OLD_BPS), fee(HIGH_BPS)), (49, 95));
    let mut peaks = [0; 4]; // Simulation, controls, rollback, close/payout.
    for direction in [-1, 1] {
        for kind in [
            processor::ASSET_AUTH_INSURANCE,
            processor::ASSET_AUTH_INSURANCE_OPERATOR,
        ] {
            let mut w = World::new(&matcher_bytes);
            w.env
                .try_update_per_asset_authority_with_cu(
                    &w.env.admin.insecure_clone(),
                    Some(&w.owners[1]),
                    0,
                    processor::ASSET_AUTH_ADMIN,
                    w.owners[1].pubkey().to_bytes(),
                )
                .unwrap();
            w.env.svm.warp_to_slot(1);
            w.env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
            let size = direction * QUANTITY;
            let open =
                w.env
                    .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, size, OLD_BPS, PRICE);
            let opening = w.bundle(&open, 0);
            peaks[3] = peaks[3].max(w.deliver(
                opening,
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.sources[0],
                    w.env.vault,
                    w.context,
                ],
                [2, 1, 1],
            ));
            w.check(size, OLD_BPS);
            let initial = w.env.control_sequences(0);
            let original_profile = state::read_asset_oracle_profile(
                &w.env.svm.get_account(&w.env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(
                original_profile.oracle_authority,
                w.env.admin.pubkey().to_bytes()
            );
            assert_eq!(
                original_profile.insurance_authority,
                original_profile.oracle_authority
            );
            assert_eq!(
                original_profile.insurance_operator,
                original_profile.oracle_authority
            );
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let request_sequence = w.env.market_state().0.matcher_req_seq;
            let close =
                w.env
                    .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, -size, OLD_BPS, PRICE);
            let closing = w.bundle_instructions(&close)[1].clone();
            let retained = [10, 11, 12].map(|nonce| w.sign(&[closing.clone()], nonce));
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for tx in &retained {
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(!tx.message.account_keys[..2].contains(&w.owners[1].pubkey()));
                peaks[0] = peaks[0].max(w.simulate(tx));
            }

            // The close really executes and charges both users before the
            // cold-admin funded-role suffix rejects. Its episode must survive.
            let oracle = role(&w, processor::ASSET_AUTH_ORACLE, initial.authority_epoch);
            let seize = role(&w, kind, initial.authority_epoch + 1);
            let bundle = w.sign(&[oracle.clone(), closing.clone(), seize], 13);
            assert_eq!(bundle.message.header.num_required_signatures, 3);
            assert!(!bundle.message.account_keys[..3].contains(&w.env.admin.pubkey()));
            peaks[2] = peaks[2].max(w.deliver_with_error(
                bundle,
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )),
                &[],
                [2, 0, 1],
            ));
            w.check(size, OLD_BPS);
            assert_eq!(w.env.control_sequences(0), initial);
            assert_eq!(w.env.portfolio_matcher_config(w.portfolios[1]), grant);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence);

            let rotation = w.sign(&[oracle], 14);
            assert_eq!(rotation.message.header.num_required_signatures, 2);
            assert!(!rotation.message.account_keys[..2].contains(&w.env.admin.pubkey()));
            peaks[1] = peaks[1].max(w.deliver(rotation, false, &[w.env.market], [1, 0, 0]));
            let mut expected_profile = original_profile;
            expected_profile.oracle_authority = w.owners[1].pubkey().to_bytes();
            assert_eq!(
                state::read_asset_oracle_profile(
                    &w.env.svm.get_account(&w.env.market).unwrap().data,
                    0,
                )
                .unwrap(),
                expected_profile
            );
            let mut controls = initial;
            controls.authority_epoch += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            w.check(size, OLD_BPS);
            peaks[0] = peaks[0].max(w.simulate(&retained[0]));

            peaks[1] = peaks[1].max(w.policy(HIGH_BPS, 15));
            controls.trade_fee += 1;
            assert_eq!(w.env.control_sequences(0), controls);
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            w.check(size, OLD_BPS);
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                grant_sequence
            );
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs
            );

            peaks[1] = peaks[1].max(w.policy(OLD_BPS, 16));
            controls.trade_fee += 1;
            assert_eq!(w.sign(&[closing.clone()], 11), retained[1]);
            for (tx, bytes) in retained.iter().zip(wires) {
                tx.verify().unwrap();
                assert_eq!(bincode::serialize(tx).unwrap(), bytes);
            }
            peaks[3] = peaks[3].max(w.deliver(
                retained[1].clone(),
                false,
                &[w.env.market, w.portfolios[0], w.portfolios[1], w.context],
                [1, 0, 1],
            ));
            let paid = 2 * fee(OLD_BPS);
            check_flat(&w, true, paid, [0; 2]);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence + 1);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|e| e + 1)
            );
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

            // A different pre-signed transaction avoids the runtime signature
            // cache and must still fail on the consumed application episode.
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[2].clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )),
                &[],
                [0, 0, 0],
            ));
            check_flat(&w, true, paid, [0; 2]);
            let mut withdrawn = [0; 2];
            for actor in 0..2 {
                let amount = DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 } - paid as u64;
                let payout = w.sign(
                    &[Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.owners[actor].pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                            AccountMeta::new(w.portfolios[actor], false),
                            AccountMeta::new(w.sources[actor], false),
                            AccountMeta::new(w.env.vault, false),
                            AccountMeta::new_readonly(w.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: w
                            .env
                            .withdraw_ix(w.portfolios[actor], amount.into())
                            .encode(),
                    }],
                    20 + actor as u32,
                );
                peaks[3] = peaks[3].max(w.deliver(
                    payout,
                    false,
                    &[
                        w.env.market,
                        w.portfolios[actor],
                        w.sources[actor],
                        w.env.vault,
                    ],
                    [1, 1, 0],
                ));
                withdrawn[actor] = amount;
                check_flat(&w, true, paid, withdrawn);
            }
        }
    }
    eprintln!("INV-005/014 retained oracle close: 4 worlds, 4 charged-close rollbacks, 4 cap rejections, 4 retained closes, 4 consumed-episode rejections, 8 SPL payouts; peak CU simulation/control/rollback/exit={peaks:?}");
}
