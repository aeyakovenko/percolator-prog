//! INV-058 / Row427: actual partial fills compete for shared side-OI headroom.
//! Three disjoint pairs, fractional quantities, explicit split/aggregate fees,
//! and public matcher controls. No economic state injection or claim tables.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, FLAG_PARTIAL_OK};

fn control(w: &mut World, data: Vec<u8>) {
    let (program_id, context, _) = w.matchers[0];
    w.env.svm.expire_blockhash();
    send_raw_tx(
        &mut w.env.svm,
        &w.env.payer,
        Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new_readonly(w.owners[1].pubkey(), true),
                AccountMeta::new(context, false),
            ],
            data,
        },
        &[&w.owners[1]],
    )
    .expect("public matcher control");
}

fn install_partial_matcher(w: &mut World) {
    let program = Pubkey::new_unique();
    w.env.svm.add_program(
        program,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let context_key = Keypair::new();
    system_create_account_for_test(
        &mut w.env.svm,
        &w.env.payer,
        &context_key,
        MATCHER_CONTEXT_LEN,
        program,
    );
    let context = context_key.pubkey();
    let delegate = matcher_delegate_key(
        &w.env.program_id,
        &w.env.market,
        &w.portfolios[1],
        &w.owners[1].pubkey(),
        &program,
        &context,
    );
    w.matchers[0] = (program, context, delegate);
    control(w, vec![10]);
    w.env
        .set_matcher_config(program, &w.owners[1], w.portfolios[1], context, delegate, 1);
    w.check();
}

fn packet(w: &mut World, ixs: &[Instruction], peak: &mut u64) {
    let bytes = bincode::serialized_size(&w.transaction(ixs)).unwrap();
    assert!(bytes <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    *peak = (*peak).max(bytes);
}

fn fill(w: &mut World, pair: usize, route: TradeRoute, q: i128, bps: u64, bytes: &mut u64) {
    let legs = [(0, q)];
    let ixs = w.instructions(pair, route, &legs, bps);
    packet(w, &ixs, bytes);
    w.accept(&ixs, &[pair]);
    w.record(pair, &legs, bps, 1);
    w.check();
}

fn checkpoint(w: &World, headroom: u128) {
    w.check();
    let asset = w.env.market_state().1.assets[0];
    assert_eq!(
        [asset.oi_eff_long_q, asset.oi_eff_short_q],
        [percolator::MAX_OI_SIDE_Q - headroom; 2]
    );
    assert_eq!(
        [asset.stored_pos_count_long, asset.stored_pos_count_short],
        [3; 2]
    );
}

#[test]
fn v16_program_shared_maker_partial_handoff_retries_at_exact_side_oi_cap() {
    let request = |w: &World, taker: usize, q: i128, maker_prefixes: u64| {
        let (program, context, delegate) = w.matchers[0];
        let mut ix = w
            .env
            .trade_cpi_ix(w.portfolios[taker], w.portfolios[1], 0, q, FEE_BPS, PRICE);
        match &mut ix {
            ProgInstruction::TradeCpi {
                account_b_position_epoch,
                ..
            } => *account_b_position_epoch += maker_prefixes,
            _ => unreachable!(),
        }
        Instruction {
            program_id: w.env.program_id,
            accounts: vec![
                AccountMeta::new(w.owners[taker].pubkey(), true),
                AccountMeta::new(w.env.market, false),
                AccountMeta::new(w.portfolios[taker], false),
                AccountMeta::new(w.portfolios[1], false),
                AccountMeta::new_readonly(program, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            data: ix.encode(),
        }
    };
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let h = RELEASE_Q as i128;
    let requested = 3 * h + 2;
    let initial = [max / 4, max / 3, max - max / 4 - max / 3];
    assert_eq!(requested / 3, h);
    assert!(3 * h < initial[1]);
    assert!(requested.unsigned_abs() < percolator::MAX_TRADE_SIZE_Q);
    assert!((initial[0] + requested).unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
    assert_eq!(ceil_ratio(notional(h) * u128::from(FEE_BPS), 10_000), 2);
    let mut peaks = [0; 3];
    let mut max_bytes = 0;
    for direction in [-1i128, 1] {
        let mut w = World::new();
        for (pair, amount) in initial.into_iter().enumerate() {
            fill(
                &mut w,
                2 * pair,
                TradeRoute::BatchNoCpi,
                direction * amount,
                0,
                &mut max_bytes,
            );
        }
        install_partial_matcher(&mut w);
        control(&mut w, vec![11, 19, 85]);
        w.env.update_trade_fee_policy_with_cu(FEE_BPS);
        checkpoint(&w, 0);

        // Both edges share maker 1 and its matcher. The prefix releases h-1,
        // then the retained request actually fills h at the maker's next epoch.
        let retained = request(&w, 0, direction * requested, 1);
        let bad = [
            request(&w, 2, -direction * 3 * (h - 1), 0),
            retained.clone(),
        ];
        packet(&mut w, &bad, &mut max_bytes);
        w.reject(&bad, PercolatorError::EngineInvalidLeg as u32, 1, 2);
        checkpoint(&w, 0);

        let good = [request(&w, 2, -direction * 3 * h, 0), retained];
        packet(&mut w, &good, &mut max_bytes);
        w.accept(&good, &[0, 1]);
        let context = w.env.svm.get_account(&w.matchers[0].1).unwrap();
        let ret = read_matcher_return(&context.data[..64]).unwrap();
        assert_eq!(ret.exec_size, direction * h);
        assert_eq!(ret.exec_price_e6, PRICE);
        assert_ne!(ret.flags & FLAG_PARTIAL_OK, 0);
        // Recording edge (1,2) in reverse matches the prefix's public (2,1) fill.
        w.record(1, &[(0, direction * h)], FEE_BPS, 1);
        w.record(0, &[(0, direction * h)], FEE_BPS, 1);
        checkpoint(&w, 0);
        assert_eq!(w.fees, [2, 4, 2, 0, 0, 0]);
        assert_eq!(w.domains, [4, 4, 0, 0]);
        assert_eq!(w.positions[1][0], -direction * initial[0]);

        w.env.update_trade_fee_policy_with_cu(0);
        for pair in [0, 1] {
            fill(
                &mut w,
                pair,
                TradeRoute::NoCpi,
                -direction * h,
                0,
                &mut max_bytes,
            );
        }
        checkpoint(&w, 0);
        for pair in [0, 2, 4] {
            let close = -w.positions[pair][0];
            fill(
                &mut w,
                pair,
                TradeRoute::BatchNoCpi,
                close,
                0,
                &mut max_bytes,
            );
        }
        assert_eq!(w.positions, [[0; ASSETS]; ACTORS]);
        for (peak, observed) in peaks.iter_mut().zip(w.peak) {
            *peak = (*peak).max(observed);
        }
    }
    println!("INV-058 shared-maker partial handoff: 2 worlds, 2 exact rollbacks, 4 committed partials; peak CU [reject, trade, custody]={peaks:?}; max packet bytes={max_bytes}");
}

#[test]
fn v16_program_partial_fills_compete_for_side_oi_and_match_aggregate_exit() {
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let h = RELEASE_Q as i128;
    let requested = 3 * h + 2;
    let executed = requested / 3;
    let residual = requested - executed;
    assert_eq!(executed, h);
    assert!(requested > 2 * h);
    assert_eq!(
        [executed, residual, requested]
            .map(|q| ceil_ratio(notional(q) * u128::from(FEE_BPS), 10_000)),
        [2, 2, 3],
        "actual split fees differ from the aggregate fee by one atom per owner"
    );
    let initial = [max / 4, max / 3, max - max / 4 - max / 3 - 2 * h];
    assert!(initial[2] > residual);
    assert!((initial[0] + requested).unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
    let mut peaks = [0; 3];
    let mut max_bytes = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut payouts = 0;
    for direction in [-1i128, 1] {
        let mut reference = None;
        for residual_route in INV_058_TRADE_ROUTES {
            for split in [false, true] {
                let mut w = World::new();
                for (pair, amount) in initial.into_iter().enumerate() {
                    fill(
                        &mut w,
                        2 * pair,
                        TradeRoute::BatchNoCpi,
                        direction * amount,
                        0,
                        &mut max_bytes,
                    );
                }
                install_partial_matcher(&mut w);
                w.env.update_trade_fee_policy_with_cu(FEE_BPS);
                checkpoint(&w, (2 * h) as u128);

                if split {
                    control(&mut w, vec![11, 19, 85]);
                    let partial =
                        w.instructions(0, TradeRoute::Cpi, &[(0, direction * requested)], FEE_BPS);
                    // The successful competitor leaves h-1 headroom. The
                    // matcher's actual h fill, not its larger request, exceeds it by one.
                    let mut late = w.instructions(
                        2,
                        TradeRoute::BatchNoCpi,
                        &[(0, direction * (h + 1))],
                        FEE_BPS,
                    );
                    late.extend(partial.clone());
                    packet(&mut w, &late, &mut max_bytes);
                    w.reject(&late, PercolatorError::EngineInvalidLeg as u32, 1, 1);
                    rollbacks += 1;
                    checkpoint(&w, (2 * h) as u128);

                    packet(&mut w, &partial, &mut max_bytes);
                    w.accept(&partial, &[0]);
                    let context = w.env.svm.get_account(&w.matchers[0].1).unwrap();
                    let ret = read_matcher_return(&context.data[..64]).unwrap();
                    assert_eq!(ret.exec_size, direction * executed);
                    assert_eq!(ret.exec_price_e6, PRICE);
                    assert_ne!(ret.flags & FLAG_PARTIAL_OK, 0);
                    w.record(0, &[(0, direction * executed)], FEE_BPS, 1);
                    checkpoint(&w, h as u128);
                    control(&mut w, vec![11, 9, 0]);
                }

                let remaining = if split { residual } else { requested };
                let retained =
                    w.instructions(0, residual_route, &[(0, direction * remaining)], FEE_BPS);
                packet(&mut w, &retained, &mut max_bytes);
                fill(
                    &mut w,
                    2,
                    TradeRoute::BatchNoCpi,
                    direction * h,
                    FEE_BPS,
                    &mut max_bytes,
                );
                checkpoint(&w, if split { 0 } else { h as u128 });
                if split {
                    let cpis = usize::from(matches!(
                        residual_route,
                        TradeRoute::Cpi | TradeRoute::BatchCpi
                    ));
                    w.reject(&retained, PercolatorError::EngineInvalidLeg as u32, 0, cpis);
                    rollbacks += 1;
                    checkpoint(&w, 0);
                }
                fill(
                    &mut w,
                    4,
                    TradeRoute::NoCpi,
                    -direction * residual,
                    FEE_BPS,
                    &mut max_bytes,
                );
                checkpoint(&w, remaining as u128);
                // Unrelated pairs change the shared cap without invalidating
                // this owner's retained bytes, position epochs or matcher grant.
                w.accept(&retained, &[0]);
                w.record(0, &[(0, direction * remaining)], FEE_BPS, 1);
                checkpoint(&w, 0);
                let target_fee = if split { 4 } else { 3 };
                assert_eq!(w.fees, [target_fee, target_fee, 2, 2, 2, 2]);

                let group = w.env.market_state().1;
                let extra = u128::from(split);
                let projection = (
                    w.positions,
                    std::array::from_fn::<_, ACTORS, _>(|actor| {
                        let p = w.env.portfolio_state(w.portfolios[actor]);
                        (
                            p.capital.get() + if actor < 2 { extra } else { 0 },
                            p.pnl.get(),
                            health_cert(&p).certified_worst_case_loss,
                            w.epochs[actor] - u64::from(split && actor < 2),
                        )
                    }),
                    group.c_tot + 2 * extra,
                    group.insurance - 2 * extra,
                    group.vault,
                );
                if let Some(expected) = &reference {
                    assert_eq!(
                        &projection, expected,
                        "{direction}/{residual_route:?}/{split}"
                    );
                } else {
                    reference = Some(projection);
                }

                w.env.update_trade_fee_policy_with_cu(0);
                for pair in [4, 0, 2] {
                    let close = -w.positions[pair][0];
                    fill(
                        &mut w,
                        pair,
                        TradeRoute::BatchNoCpi,
                        close,
                        0,
                        &mut max_bytes,
                    );
                }
                assert_eq!(w.positions, [[0; ASSETS]; ACTORS]);
                for actor in 0..ACTORS {
                    let amount = CAPITAL - w.fees[actor];
                    let ix = Instruction {
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
                        data: w.env.withdraw_ix(w.portfolios[actor], amount).encode(),
                    };
                    packet(&mut w, std::slice::from_ref(&ix), &mut max_bytes);
                    let tx = w.transaction(&[ix]);
                    let meta = w.env.svm.send_transaction(tx).unwrap();
                    w.paid[actor] = amount;
                    w.check();
                    assert_cu_within(
                        "partial side-OI payout",
                        meta.compute_units_consumed,
                        CUSTODY_CU_LIMIT,
                    );
                    w.peak[2] = w.peak[2].max(meta.compute_units_consumed);
                    payouts += 1;
                }
                assert_eq!(w.env.market_state().1.c_tot, 0);
                assert_eq!(w.env.market_state().1.vault, if split { 16 } else { 14 });
                for (peak, observed) in peaks.iter_mut().zip(w.peak) {
                    *peak = (*peak).max(observed);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks, payouts), (16, 16, 96));
    println!("INV-058 partial side-OI: {worlds} worlds, {rollbacks} exact rollbacks, {payouts} payouts; peak CU [reject, trade, custody]={peaks:?}; max packet bytes={max_bytes}");
}
