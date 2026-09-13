//! INV-014 / row 432: each single-CPI instruction owns its signed rate ceiling.
//! A retained atomic open/close cannot spend the opening's unused fee allowance
//! on its closing instruction after repricing, even with zero net position.
//! Public SPL custody, exact rollback of the charged opening, and owner payouts.

use super::*;

const OPEN_CAP: u64 = 99;
const RAISED_BPS: u64 = 37;

fn check_flat(w: &World, deposited: bool, paid_per_owner: u128, payouts: [u64; 2]) {
    let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
    let (_, group) = w.env.market_state();
    for actor in 0..2 {
        assert_eq!(accounts[actor].owner, w.owners[actor].pubkey().to_bytes());
        assert_eq!(
            accounts[actor].capital.get(),
            u128::from(DEPOSITS[actor] + if deposited && actor == 0 { PREFIX } else { 0 })
                - paid_per_owner
                - u128::from(payouts[actor])
        );
        assert_eq!(accounts[actor].pnl.get(), 0);
        assert_eq!(accounts[actor].fee_credits.get(), 0);
        assert!(!has_active_leg_for_asset(&accounts[actor], 0));
        assert_eq!(
            w.env.token_amount(w.sources[actor]),
            payouts[actor] + if !deposited && actor == 0 { PREFIX } else { 0 }
        );
    }
    let vault = DEPOSITS.iter().sum::<u64>() + if deposited { PREFIX } else { 0 }
        - payouts.iter().sum::<u64>();
    assert_eq!(group.assets[0].effective_price, PRICE);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
    assert_eq!(&group.insurance_domain_budget[..2], &[paid_per_owner; 2]);
    assert!(group.insurance_domain_budget[2..]
        .iter()
        .all(|amount| *amount == 0));
    assert_eq!(group.insurance, 2 * paid_per_owner);
    assert_eq!(group.c_tot, u128::from(vault) - 2 * paid_per_owner);
    assert_eq!(group.vault, u128::from(vault));
    assert_eq!(w.env.token_amount(w.env.vault), vault);
    let supply = DEPOSITS.iter().sum::<u64>() + PREFIX;
    assert_eq!(
        vault
            + w.sources
                .map(|key| w.env.token_amount(key))
                .iter()
                .sum::<u64>(),
        supply
    );
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, supply);
    assert_eq!(mint.mint_authority, COption::None);
    assert_market_stock_census(
        "retained single-CPI round trip fee consent",
        &group,
        &w.env.svm.get_account(&w.env.market).unwrap().data,
        &accounts,
        vault.into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained single-CPI round trip", &group, &accounts)
        .unwrap();
}

#[test]
fn v16_retained_single_cpi_round_trip_cannot_pool_instruction_fee_consent() {
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    assert_eq!(
        (fee(OLD_BPS), fee(RAISED_BPS), fee(OPEN_CAP)),
        (49, 95, 253)
    );
    assert!(OLD_BPS < RAISED_BPS && RAISED_BPS < OPEN_CAP);
    assert!(OPEN_CAP < u64::from(LP_CAP_BPS));
    // The attempted 190 atoms per owner fit inside the two signed ceilings'
    // 302-atom sum. Only the closing instruction's own 19-bps cap is exceeded.
    assert!(2 * fee(RAISED_BPS) < fee(OPEN_CAP) + fee(OLD_BPS));
    let mut peaks = [0; 4]; // Simulation, policy, rejection, committed trade/payout.
    let (mut worlds, mut rollbacks, mut restored, mut renewed, mut payouts) = (0, 0, 0, 0, 0);
    for direction in [-1, 1] {
        for renew_close in [false, true] {
            let mut w = World::new(&matcher_bytes);
            let size = direction * QUANTITY;
            let open =
                w.env
                    .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, size, OPEN_CAP, PRICE);
            let mut close =
                w.env
                    .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, -size, OLD_BPS, PRICE);
            // Predict the public opening's two position-epoch increments in
            // the signed close; no account data is edited to construct it.
            let ProgInstruction::TradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            } = &mut close
            else {
                unreachable!()
            };
            *account_a_position_epoch += 1;
            *account_b_position_epoch += 1;
            let [deposit, opening] = w.bundle_instructions(&open);
            let closing = w.bundle_instructions(&close)[1].clone();
            let instructions = [deposit, opening, closing];
            let retained = [w.sign(&instructions, 0), w.sign(&instructions, 1)];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for tx in &retained {
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(!tx.message.account_keys[..2].contains(&w.owners[1].pubkey()));
            }
            peaks[0] = peaks[0].max(w.simulate(&retained[0]));
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let grant = w.env.portfolio_matcher_config(w.portfolios[1]);
            let grant_sequence = w.env.portfolio_matcher_sequence(w.portfolios[1]);
            let request_sequence = w.env.market_state().0.matcher_req_seq;
            check_flat(&w, false, 0, [0; 2]);

            peaks[1] = peaks[1].max(w.policy(RAISED_BPS, 10));
            let controls = w.env.control_sequences(0);
            assert_eq!(w.sign(&instructions, 0), retained[0]);
            peaks[2] = peaks[2].max(w.deliver_with_error(
                retained[0].clone(),
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [2, 1, 1],
            ));
            rollbacks += 1;
            check_flat(&w, false, 0, [0; 2]);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.trade_fee_base_bps, RAISED_BPS);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs
            );

            let (accepted, bps) = if renew_close {
                let ProgInstruction::TradeCpi { fee_bps, .. } = &mut close else {
                    unreachable!()
                };
                *fee_bps = RAISED_BPS;
                let mut fresh_instructions = instructions.clone();
                fresh_instructions[2].data = close.encode();
                let fresh = w.sign(&fresh_instructions, 1);
                let mut expected = retained[1].message.clone();
                expected.instructions[4].data = close.encode();
                assert_eq!(
                    fresh.message, expected,
                    "only the signed closing fee field changes"
                );
                assert_ne!(fresh.signatures, retained[1].signatures);
                renewed += 1;
                (fresh, RAISED_BPS)
            } else {
                peaks[1] = peaks[1].max(w.policy(OLD_BPS, 11));
                assert_eq!(w.sign(&instructions, 1), retained[1]);
                restored += 1;
                (retained[1].clone(), OLD_BPS)
            };
            for (tx, wire) in retained.iter().zip(&wires) {
                tx.verify().unwrap();
                assert_eq!(&bincode::serialize(tx).unwrap(), wire);
            }
            let controls = w.env.control_sequences(0);
            peaks[3] = peaks[3].max(w.deliver(
                accepted,
                false,
                &[
                    w.env.market,
                    w.portfolios[0],
                    w.portfolios[1],
                    w.sources[0],
                    w.env.vault,
                    w.context,
                ],
                [3, 1, 2],
            ));
            let paid = 2 * fee(bps);
            check_flat(&w, true, paid, [0; 2]);
            assert_eq!(w.env.control_sequences(0), controls);
            assert_eq!(w.env.market_state().0.matcher_req_seq, request_sequence + 2);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|e| e + 2)
            );
            let mut expected_grant = grant;
            for _ in 0..2 {
                expected_grant.control =
                    state::next_portfolio_position_control(expected_grant.control)
                        .unwrap()
                        .1;
            }
            assert_eq!(
                w.env.portfolio_matcher_config(w.portfolios[1]),
                expected_grant
            );
            assert_eq!(
                w.env.portfolio_matcher_sequence(w.portfolios[1]),
                grant_sequence
            );
            let fill = percolator_prog::matcher_abi::read_matcher_return(
                &w.env.svm.get_account(&w.context).unwrap().data,
            )
            .unwrap();
            assert_eq!((fill.exec_size, fill.exec_price_e6), (-size, PRICE));

            let mut withdrawn = [0; 2];
            for actor in 0..2 {
                let amount = DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 }
                    - u64::try_from(paid).unwrap();
                let tx = w.sign(
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
                    tx,
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
                payouts += 1;
            }
            assert_eq!(u128::from(w.env.token_amount(w.env.vault)), 2 * paid);
            worlds += 1;
        }
    }
    assert_eq!(
        (worlds, rollbacks, restored, renewed, payouts),
        (4, 4, 2, 2, 8)
    );
    eprintln!("INV-014 row432 instruction-local consent: worlds={worlds}, exact charged-opening/SPL rollbacks={rollbacks}, restored round trips={restored}, renewed-close round trips={renewed}, owner payouts={payouts}; max CU simulation/policy/rejection/commit={peaks:?}");
}
