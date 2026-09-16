//! INV-014 / row 411: a paid bilateral position precedes retained batch consent.
//! A base-policy update and elapsed EWMA slot add a movement fee on existing
//! OI, not just the new fill. Exact atom caps include that externality;
//! a failed suffix also restores the completed fee and staged mark transition.

use super::*;

const CURRENT_BPS: u64 = 37;
const SPREAD_BPS: u64 = 1_000;
const ADDITION: i128 = (25 * POS_SCALE + POS_SCALE / 3 + 1) as i128;

#[derive(Clone, Copy, Debug)]
struct Economics {
    print: u64,
    mark: u64,
    base: u128,
    total: u128,
    externality: u128,
}

fn economics(direction: i128) -> Economics {
    let print = if direction > 0 { 110 } else { 90 };
    let mark = if direction > 0 { 105 } else { 95 };
    let notional = (ADDITION.unsigned_abs() * u128::from(print)).div_ceil(POS_SCALE);
    let incumbent = (QUANTITY.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert!(incumbent > notional);
    let charge = |bps: u64| (notional * u128::from(bps)).div_ceil(10_000);
    let base = charge(CURRENT_BPS);
    // One elapsed slot and halflife one move 100 halfway to 90/110. Existing
    // gross OI owns the 500-bps externality; prior insurance cannot pay it again.
    let externality = (2 * incumbent * 500).div_ceil(10_000);
    let total = (CURRENT_BPS..=10_000)
        .map(charge)
        .find(|paid| 2 * paid >= 2 * base + externality)
        .unwrap();
    let empty_oi_fee = (CURRENT_BPS..=10_000)
        .map(charge)
        .find(|paid| 2 * paid >= 2 * base + (2 * notional * 500).div_ceil(10_000))
        .unwrap();
    assert!(total - 1 > empty_oi_fee + fee(OLD_BPS));
    assert!(total > base);
    Economics {
        print,
        mark,
        base,
        total,
        externality,
    }
}

fn book(w: &World, direction: i128, e: Economics, filled: bool) {
    let opening = fee(OLD_BPS);
    let paid = opening + if filled { e.total } else { 0 };
    let budgeted = opening + if filled { e.base } else { 0 };
    let size = direction * (QUANTITY + if filled { ADDITION } else { 0 });
    let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
    let (_, market) = w.env.market_state();
    let vault = DEPOSITS.iter().sum::<u64>() + if filled { PREFIX } else { 0 };
    for actor in 0..2 {
        let p = &accounts[actor];
        assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
        assert_eq!(
            p.capital.get(),
            u128::from(DEPOSITS[actor] + if filled && actor == 0 { PREFIX } else { 0 }) - paid
        );
        assert_eq!(p.pnl.get(), 0);
        assert_eq!(p.fee_credits.get(), 0);
        assert_eq!(
            active_leg_for_asset(p, 0).basis_pos_q,
            if actor == 0 { size } else { -size }
        );
        assert_eq!(
            w.env.token_amount(w.sources[actor]),
            if !filled && actor == 0 { PREFIX } else { 0 }
        );
    }
    assert_eq!(market.assets[0].effective_price, PRICE);
    assert_eq!(market.assets[0].oi_eff_long_q, size.unsigned_abs());
    assert_eq!(market.assets[0].oi_eff_short_q, size.unsigned_abs());
    assert_eq!(market.c_tot, u128::from(vault) - 2 * paid);
    assert_eq!(market.insurance, 2 * paid);
    assert_eq!(&market.insurance_domain_budget[..2], &[budgeted; 2]);
    assert!(market.insurance_domain_budget[2..].iter().all(|x| *x == 0));
    assert_eq!(market.insurance_domain_budget_remaining_total, 2 * budgeted);
    assert_eq!(market.vault, u128::from(vault));
    assert_eq!(w.env.token_amount(w.env.vault), vault);
    let profile =
        state::read_asset_oracle_profile(&w.env.svm.get_account(&w.env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(profile.mark_ewma_e6, if filled { e.mark } else { PRICE });
    assert_eq!(profile.mark_ewma_last_slot, u64::from(filled));
    assert_eq!(
        market.assets[0].raw_oracle_target_price,
        if filled { e.mark } else { PRICE }
    );
    if filled {
        assert!(2 * (e.total - e.base) >= e.externality);
        assert_eq!(
            market.insurance - market.insurance_domain_budget_remaining_total,
            2 * (e.total - e.base),
            "only newly collected movement fees are unbudgeted"
        );
    }
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>() + PREFIX);
    assert_eq!(
        vault
            + w.sources
                .map(|key| w.env.token_amount(key))
                .iter()
                .sum::<u64>(),
        mint.supply
    );
    assert_market_stock_census(
        "retained OI mark consent",
        &market,
        &w.env.svm.get_account(&w.env.market).unwrap().data,
        &accounts,
        vault.into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained OI mark consent", &market, &accounts).unwrap();
}

#[test]
fn v16_retained_route_switch_caps_mark_fees_on_existing_open_interest() {
    let matcher = std::fs::read(auth_matcher_program_path()).unwrap();
    let mut peaks = [0; 5]; // Opening, simulation, policy, rejection, committed continuation.
    let (mut worlds, mut simulations, mut rollbacks, mut continuations) = (0, 0, 0, 0);
    for direction in [-1, 1] {
        let e = economics(direction);
        for batch_open in [false, true] {
            eprintln!("direction={direction}, batch_open={batch_open}, economics={e:?}");
            let mut w = World::new(&matcher);
            let controls = w.env.control_sequences(0);
            let tx = w.sign(
                &[Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.env.admin.pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                    ],
                    data: ProgInstruction::ConfigureEwmaMark {
                        market_id: w.env.asset_market_id(0),
                        asset_index: 0,
                        now_slot: 0,
                        initial_mark_e6: PRICE,
                        mark_ewma_halflife_slots: 1,
                        mark_min_fee: 0,
                        observation_sequence: controls.oracle_observation + 1,
                        authority_epoch: controls.authority_epoch,
                    }
                    .encode(),
                }],
                21,
            );
            peaks[2] = peaks[2].max(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
            let [a, b] = w.portfolios;
            let opening = if batch_open {
                w.env.batch_trade_no_cpi_ix(
                    a,
                    b,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: w.env.asset_market_id(0),
                        size_q: direction * QUANTITY,
                        exec_price: PRICE,
                        fee_bps: OLD_BPS,
                    }],
                )
            } else {
                w.env
                    .trade_no_cpi_ix(a, b, 0, direction * QUANTITY, PRICE, OLD_BPS)
            };
            let direct = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(w.owners[0].pubkey(), true),
                    AccountMeta::new(w.owners[1].pubkey(), true),
                    AccountMeta::new(w.env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                ],
                data: opening.encode(),
            };
            let tx = w.sign(&[direct], 0);
            peaks[0] = peaks[0].max(w.deliver(tx, false, &[w.env.market, a, b], [1, 0, 0]));
            assert_eq!(w.env.portfolio_matcher_config(b).enabled(), 0);
            w.env.set_matcher_config_with_trade_fee_cap(
                w.matcher,
                &w.owners[1],
                b,
                w.context,
                w.delegate,
                1,
                LP_CAP_BPS,
            );
            let mut spread = vec![4];
            spread.extend_from_slice(&SPREAD_BPS.to_le_bytes());
            spread.extend_from_slice(&SPREAD_BPS.to_le_bytes());
            send_raw_tx(
                &mut w.env.svm,
                &w.env.payer,
                Instruction {
                    program_id: w.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                        AccountMeta::new(w.context, false),
                    ],
                    data: spread,
                },
                &[&w.owners[1]],
            )
            .unwrap();
            book(&w, direction, e, false);

            let narrow = w.env.batch_trade_cpi_ix_with_caps(
                a,
                b,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: w.env.asset_market_id(0),
                    size_q: direction * ADDITION,
                    fee_bps: u64::from(LP_CAP_BPS),
                    limit_price: e.print,
                }],
                (ADDITION.unsigned_abs() * 10).div_ceil(POS_SCALE),
                e.total - 1,
            );
            let mut exact = narrow.clone();
            let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut exact else {
                unreachable!()
            };
            *max_fee_atoms += 1;
            let template = w.bundle_instructions(&w.env.trade_cpi_ix(
                a,
                b,
                0,
                direction * ADDITION,
                CURRENT_BPS,
                e.print,
            ));
            let bundle = |trade: &ProgInstruction| {
                let mut instructions = template.clone();
                instructions[1].data = trade.encode();
                instructions
            };
            let controls = w.env.control_sequences(0);
            let stale_policy = Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(w.env.admin.pubkey(), true),
                    AccountMeta::new(w.env.market, false),
                ],
                data: ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: OLD_BPS,
                    policy_sequence: controls.trade_fee + 1,
                    authority_epoch: controls.authority_epoch,
                }
                .encode(),
            };
            let mut late = bundle(&exact).to_vec();
            late.push(stale_policy);
            let retained = [
                w.sign(&bundle(&narrow), 10),
                w.sign(&late, 11),
                w.sign(&bundle(&exact), 12),
            ];
            let wires = retained
                .each_ref()
                .map(|tx| bincode::serialize(tx).unwrap());
            for tx in &retained {
                assert!(!tx.message.account_keys
                    [..usize::from(tx.message.header.num_required_signatures)]
                    .contains(&w.owners[1].pubkey()));
                peaks[1] = peaks[1].max(w.simulate(tx));
                simulations += 1;
            }
            let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
            let grant = w.env.portfolio_matcher_config(b);
            let sequence = w.env.portfolio_matcher_sequence(b);
            let requests = w.env.market_state().0.matcher_req_seq;
            peaks[2] = peaks[2].max(w.policy(CURRENT_BPS, 20));
            w.env.svm.warp_to_slot(1);
            let current_controls = w.env.control_sequences(0);
            book(&w, direction, e, false);

            for (index, error, calls) in [
                (0, (3, PercolatorError::InvalidInstruction), [1, 1, 1]),
                (1, (4, PercolatorError::EngineStale), [2, 1, 1]),
            ] {
                assert_eq!(bincode::serialize(&retained[index]).unwrap(), wires[index]);
                peaks[3] = peaks[3].max(w.deliver_with_error(
                    retained[index].clone(),
                    Some((error.0, InstructionError::Custom(error.1 as u32))),
                    &[],
                    calls,
                ));
                rollbacks += 1;
                book(&w, direction, e, false);
                assert_eq!(w.env.control_sequences(0), current_controls);
                assert_eq!(w.env.market_state().0.matcher_req_seq, requests);
                assert_eq!(
                    w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                    epochs
                );
                assert_eq!(w.env.portfolio_matcher_config(b), grant);
            }
            for (tx, wire) in retained.iter().zip(&wires) {
                tx.verify().unwrap();
                assert_eq!(&bincode::serialize(tx).unwrap(), wire);
            }
            peaks[4] = peaks[4].max(w.deliver(
                retained[2].clone(),
                false,
                &[w.env.market, a, b, w.sources[0], w.env.vault],
                [2, 1, 1],
            ));
            book(&w, direction, e, true);
            assert_eq!(w.env.control_sequences(0), current_controls);
            assert_eq!(w.env.market_state().0.matcher_req_seq, requests + 1);
            assert_eq!(
                w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                epochs.map(|x| x + 1)
            );
            assert_eq!(w.env.portfolio_matcher_sequence(b), sequence);
            let mut expected_grant = grant;
            expected_grant.control = state::next_portfolio_position_control(grant.control)
                .unwrap()
                .1;
            assert_eq!(w.env.portfolio_matcher_config(b), expected_grant);
            worlds += 1;
            continuations += 1;
        }
    }
    assert_eq!(
        (worlds, simulations, rollbacks, continuations),
        (4, 12, 8, 4)
    );
    eprintln!("INV-014 row411 incumbent-OI mark consent: worlds={worlds}, pre-policy simulations={simulations}, complete Account rollbacks={rollbacks}, exact-cap continuations={continuations}; peak CU opening/simulation/policy/rejection/continuation={peaks:?}");
}
