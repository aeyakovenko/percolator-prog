//! Row 411 / INV-014/011/024/047/081: two pending funding domains do not
//! enlarge retained gross fee consent. Compare CPI and bilateral batches.

use super::*;

const SIZES: [i128; 2] = [QUANTITY, 57 * POS_SCALE as i128];

fn admin_instruction(w: &World, request: ProgInstruction) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.env.admin.pubkey(), true),
            AccountMeta::new(w.env.market, false),
        ],
        data: request.encode(),
    }
}

fn observe(w: &World, slot: u64) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(w.env.payer.pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.portfolios[1], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations_for_assets(&[0, 1]),
        }
        .encode(),
    }
}

fn trades(w: &World, cpi: bool, close: bool) -> Vec<Instruction> {
    let route = if cpi {
        Route::BatchCpi
    } else {
        Route::BatchNoCpi
    };
    let accounts = w.bundle_instructions(route, 1, 1, OPEN_CAP)[1]
        .accounts
        .clone();
    let [a, b] = w.portfolios;
    let sign = if close { 1 } else { -1 };
    let requests = if cpi {
        vec![w.env.batch_trade_cpi_ix_with_caps(
            a,
            b,
            SIZES
                .into_iter()
                .enumerate()
                .map(|(asset, size)| BatchTradeCpiLeg {
                    asset_index: asset as u16,
                    market_id: w.env.asset_market_id(asset as u16),
                    size_q: sign * size,
                    fee_bps: u64::from(LP_CAP),
                    limit_price: PRICE,
                })
                .collect(),
            0,
            SIZES.into_iter().map(|q| fee(q, OPEN_CAP)).sum(),
        )]
    } else {
        vec![w.env.batch_trade_no_cpi_ix(
            a,
            b,
            SIZES
                .into_iter()
                .enumerate()
                .map(|(asset, size)| BatchTradeLeg {
                    asset_index: asset as u16,
                    market_id: w.env.asset_market_id(asset as u16),
                    size_q: sign * size,
                    exec_price: PRICE,
                    fee_bps: OPEN_CAP,
                })
                .collect(),
        )]
    };
    requests
        .into_iter()
        .map(|request| Instruction {
            program_id: w.env.program_id,
            accounts: accounts.clone(),
            data: request.encode(),
        })
        .collect()
}

fn check_value(w: &World, closed: bool, converted: bool, paid: [u64; 2], custody: &[Account; 3]) {
    let fees = SIZES.map(|q| fee(q, OPEN_CAP) * (1 + u128::from(closed)));
    let gross = fees.iter().sum::<u128>() as u64;
    let funding = u64::from(closed) * SIZES.into_iter().map(funding_quote).sum::<u64>();
    let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
    let (cfg, group) = w.env.market_state();
    assert_eq!(cfg.fee_redirect_to_market_0_bps, 0);
    for actor in 0..2 {
        let expected = PRINCIPAL[actor] + u64::from(actor == 0) * DEPOSIT
            - gross
            - u64::from(actor == 0) * funding
            + u64::from(actor == 1 && converted) * funding
            - paid[actor];
        assert_eq!(accounts[actor].owner, w.owners[actor].pubkey().to_bytes());
        assert_eq!(accounts[actor].capital.get(), u128::from(expected));
        assert_eq!(
            accounts[actor].pnl.get(),
            i128::from(actor == 1 && !converted) * i128::from(funding)
        );
        assert_eq!(accounts[actor].fee_credits.get(), 0);
        for (asset, size) in SIZES.into_iter().enumerate() {
            if closed {
                assert!(!has_active_leg_for_asset(&accounts[actor], asset));
            } else {
                assert_eq!(
                    active_leg_for_asset(&accounts[actor], asset).basis_pos_q,
                    size * if actor == 0 { -1 } else { 1 }
                );
            }
        }
    }
    for asset in 0..2 {
        assert_eq!(group.assets[asset].effective_price, PRICE);
        let oi = if closed { 0 } else { SIZES[asset] as u128 };
        assert_eq!(group.assets[asset].oi_eff_long_q, oi);
        assert_eq!(group.assets[asset].oi_eff_short_q, oi);
        assert_eq!(
            &group.insurance_domain_budget[2 * asset..2 * asset + 2],
            &[fees[asset]; 2]
        );
        assert_eq!(
            group.assets[asset].f_long_num,
            if closed { ADL_ONE as i128 } else { 0 }
        );
    }
    assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
    assert_eq!(group.funding_epoch, if closed { 2 } else { 0 });
    assert_eq!(group.insurance, 2 * u128::from(gross));
    assert_eq!(group.c_tot, accounts.iter().map(|p| p.capital.get()).sum());
    let supply = PRINCIPAL.iter().sum::<u64>() + DEPOSIT;
    let vault = supply - paid.iter().sum::<u64>();
    assert_eq!(group.vault, u128::from(vault));
    assert_eq!(
        group.vault,
        group.c_tot + group.insurance + u128::from(!converted) * u128::from(funding)
    );
    for ((key, before), amount) in [w.tokens[0], w.tokens[1], w.env.vault]
        .into_iter()
        .zip(custody)
        .zip([paid[0], paid[1], vault])
    {
        let mut expected = before.clone();
        let mut token = TokenAccount::unpack(&expected.data).unwrap();
        token.amount = amount;
        TokenAccount::pack(token, &mut expected.data).unwrap();
        assert_eq!(w.env.svm.get_account(&key), Some(expected));
    }
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, supply);
    assert_eq!(mint.mint_authority, COption::None);
    assert_market_stock_census(
        "retained two-asset funding",
        &group,
        &w.env.svm.get_account(&w.env.market).unwrap().data,
        &accounts,
        vault.into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained two-asset funding", &group, &accounts).unwrap();
}

#[test]
fn v16_retained_two_asset_funding_preserves_cpi_and_bilateral_fee_consent() {
    let matcher = std::fs::read(hostile_matcher_program_path()).unwrap();
    let mut counts = Counts::default();
    let fees = SIZES.map(|q| fee(q, OPEN_CAP));
    let funding = SIZES.map(funding_quote);
    assert_eq!(fees, [36, 22]);
    assert_eq!(fees.iter().sum::<u128>(), 58);
    assert_eq!(fee(SIZES.iter().sum(), OPEN_CAP), 57);
    assert_eq!(funding, [95, 57]);
    assert!(
        u128::from(funding.iter().sum::<u64>())
            > SIZES.into_iter().map(|q| fee(q, CLOSE_FRESH)).sum()
    );
    for cpi in [true, false] {
        eprintln!("retained two-asset funding: cpi={cpi}");
        let mut w = World::with_params(
            &matcher,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                trade_fee_base_bps: OPEN_CAP,
                max_abs_funding_e9_per_slot: FUNDING_RATE,
                max_price_move_bps_per_slot: 1,
                h_max: 1,
                ..V16CuMarketParams::default()
            },
        );
        for asset in 0..2 {
            let controls = w.env.control_sequences(asset);
            let ix = admin_instruction(
                &w,
                ProgInstruction::ConfigureEwmaMark {
                    market_id: w.env.asset_market_id(asset as u16),
                    asset_index: asset as u16,
                    now_slot: 0,
                    initial_mark_e6: PRICE,
                    mark_ewma_halflife_slots: 1,
                    mark_min_fee: 0,
                    observation_sequence: controls.oracle_observation + 1,
                    authority_epoch: controls.authority_epoch,
                },
            );
            let tx = w.sign_with_nonce(&[ix], 1 + asset as u32);
            counts.record(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
        }
        let mut opening = vec![w.bundle_instructions(Route::BatchCpi, 1, 1, OPEN_CAP)[0].clone()];
        opening.extend(trades(&w, true, false));
        let tx = w.sign_with_nonce(&opening, 3);
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
        counts.fills += 2;
        let custody =
            [w.tokens[0], w.tokens[1], w.env.vault].map(|key| w.env.svm.get_account(&key).unwrap());
        check_value(&w, false, false, [0; 2], &custody);
        let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));
        let sequences = w
            .portfolios
            .map(|key| w.env.portfolio_matcher_sequence(key));
        let requests = w.env.market_state().0.matcher_req_seq;
        assert_eq!(
            w.env
                .portfolio_matcher_config(w.portfolios[1])
                .trade_fee_cap_bps(),
            LP_CAP
        );
        let controls = w.env.control_sequences(0);
        let close = trades(&w, cpi, true);
        let mut late = close.clone();
        late.push(fee_control(
            &w,
            w.env.admin.pubkey(),
            OPEN_CAP,
            controls.trade_fee + 1,
            controls.authority_epoch,
        ));
        let retained = [
            w.sign_with_nonce(&close, 10),
            w.sign_with_nonce(&late, 11),
            w.sign_with_nonce(&close, 12),
            w.sign_with_nonce(&close, 13),
        ];
        let mut short = close.clone();
        if cpi {
            let mut request = ProgInstruction::decode(&short[0].data).unwrap();
            if let ProgInstruction::BatchTradeCpi { max_fee_atoms, .. } = &mut request {
                assert_eq!(*max_fee_atoms, fees.iter().sum());
                *max_fee_atoms -= 1;
            }
            short[0].data = request.encode();
        }
        let short = w.sign_with_nonce(&short, 14);
        let wires = retained
            .each_ref()
            .map(|tx| bincode::serialize(tx).unwrap());
        let short_wire = bincode::serialize(&short).unwrap();
        counts.record(policy(&mut w, CLOSE_FRESH, 20));
        for (slot, mark) in [(1, 98), (2, 101)] {
            w.env.svm.warp_to_slot(slot);
            for asset in 0..2 {
                let controls = w.env.control_sequences(asset);
                let ix = admin_instruction(
                    &w,
                    ProgInstruction::PushEwmaMark {
                        market_id: w.env.asset_market_id(asset as u16),
                        asset_index: asset as u16,
                        now_slot: slot,
                        mark_e6: mark,
                        observation_sequence: controls.oracle_observation + 1,
                        authority_epoch: controls.authority_epoch,
                    },
                );
                let tx = w.sign_with_nonce(&[ix], 21 + 2 * slot as u32 + asset as u32);
                counts.record(w.deliver(tx, false, &[w.env.market], [1, 0, 0]));
            }
            if slot == 1 {
                let tx = w.sign_with_nonce(&[observe(&w, slot)], 30);
                counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[1]], [1, 0, 0]));
            }
            check_value(&w, false, false, [0; 2], &custody);
        }
        counts.record(w.deliver_with_error(
            retained[0].clone(),
            Some((
                2,
                InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
            )),
            &[],
            [0, 0, usize::from(cpi)],
        ));
        counts.rollbacks += 1;
        check_value(&w, false, false, [0; 2], &custody);
        counts.record(policy(&mut w, OPEN_CAP, 31));
        if cpi {
            counts.record(w.deliver_with_error(
                short.clone(),
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                )),
                &[],
                [0, 0, 1],
            ));
            counts.rollbacks += 1;
        }
        counts.record(w.deliver_with_error(
            retained[1].clone(),
            Some((
                3,
                InstructionError::Custom(PercolatorError::EngineStale as u32),
            )),
            &[],
            [1, 0, usize::from(cpi)],
        ));
        counts.rollbacks += 1;
        check_value(&w, false, false, [0; 2], &custody);
        let mut changed = vec![w.env.market, w.portfolios[0], w.portfolios[1]];
        if cpi {
            changed.push(w.context);
        }
        counts.record(w.deliver(
            retained[2].clone(),
            false,
            &changed,
            [1, 0, usize::from(cpi)],
        ));
        counts.fills += 2;
        check_value(&w, true, false, [0; 2], &custody);
        assert_eq!(
            w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
            epochs.map(|e| e + 1)
        );
        assert_eq!(
            w.env.portfolio_matcher_sequence(w.portfolios[0]),
            sequences[0]
        );
        assert_eq!(
            w.env.portfolio_matcher_sequence(w.portfolios[1]),
            sequences[1]
        );
        assert_eq!(
            w.env.market_state().0.matcher_req_seq,
            requests + u64::from(cpi)
        );
        assert_eq!(
            w.env.portfolio_matcher_config(w.portfolios[1]).enabled(),
            u64::from(cpi)
        );
        assert_eq!(
            w.env.portfolio_matcher_expiry(w.portfolios[1]),
            if cpi { u64::MAX } else { 0 }
        );
        counts.record(w.deliver_with_error(
            retained[3].clone(),
            Some((
                2,
                InstructionError::Custom(PercolatorError::EngineStale as u32),
            )),
            &[],
            [0, 0, 0],
        ));
        counts.rollbacks += 1;
        check_value(&w, true, false, [0; 2], &custody);
        for (tx, wire) in retained.iter().zip(wires) {
            tx.verify().unwrap();
            assert_eq!(bincode::serialize(tx).unwrap(), wire);
        }
        short.verify().unwrap();
        assert_eq!(bincode::serialize(&short).unwrap(), short_wire);
        w.env.svm.warp_to_slot(3);
        let tx = w.sign_with_nonce(&[observe(&w, 3)], 40);
        counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[1]], [1, 0, 0]));
        let tx = w.sign_with_nonce(
            &[Instruction {
                program_id: w.env.program_id,
                accounts: vec![
                    AccountMeta::new(w.owners[1].pubkey(), true),
                    AccountMeta::new(w.env.market, false),
                    AccountMeta::new(w.portfolios[1], false),
                ],
                data: w
                    .env
                    .convert_released_pnl_ix(w.portfolios[1], funding.iter().sum::<u64>().into())
                    .encode(),
            }],
            41,
        );
        counts.record(w.deliver(tx, false, &[w.env.market, w.portfolios[1]], [1, 0, 0]));
        let mut paid = [0; 2];
        check_value(&w, true, true, paid, &custody);
        let gross = 2 * fees.iter().sum::<u128>() as u64;
        let total_funding = funding.iter().sum::<u64>();
        let payouts = [
            PRINCIPAL[0] + DEPOSIT - gross - total_funding,
            PRINCIPAL[1] - gross + total_funding,
        ];
        assert_eq!(payouts, [99_848, 200_043]);
        for actor in 0..2 {
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
                        .withdraw_ix(w.portfolios[actor], payouts[actor].into())
                        .encode(),
                }],
                42 + actor as u32,
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
            paid[actor] = payouts[actor];
            counts.payouts += 1;
            check_value(&w, true, true, paid, &custody);
        }
        assert_eq!(w.env.token_amount(w.env.vault), 232);
        counts.worlds += 1;
    }
    assert_eq!(
        (
            counts.worlds,
            counts.rollbacks,
            counts.fills,
            counts.payouts
        ),
        (2, 7, 8, 4)
    );
    eprintln!("INV-014 retained multi-asset funding: {counts:?}");
}
