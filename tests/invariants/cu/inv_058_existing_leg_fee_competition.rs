//! INV-058 / row427: fee-bearing existing pairs compete for shared side headroom.
//! Unequal rounded fees stay owner-attributed through pair/route order and late
//! cap rollback. Fixed marks, unit ADL, zero PnL/funding; bounded conformance only.

use super::*;

fn prepare(w: &mut World, pair: usize, route: TradeRoute) {
    if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        let (program, context, delegate) = w.matchers[pair / 2];
        w.env.set_matcher_config(
            program,
            &w.owners[pair + 1],
            w.portfolios[pair + 1],
            context,
            delegate,
            1,
        );
    }
    w.check();
}

fn cpis(route: TradeRoute) -> usize {
    usize::from(matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi))
}

fn admissible(w: &World, pair: usize, delta: i128) {
    assert!(delta.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
    for (actor, q) in [(pair, delta), (pair + 1, -delta)] {
        let proposed = w.positions[actor][0] + q;
        assert!(proposed.unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
        assert!(notional(proposed) < percolator::MAX_ACCOUNT_NOTIONAL);
        let fee = ceil_ratio(notional(q) * u128::from(FEE_BPS), 10_000);
        assert!(notional(proposed) + fee < CAPITAL - w.fees[actor]);
    }
}

fn checkpoint(w: &World, headroom: u128, fees: [u128; ACTORS]) {
    w.check();
    assert_eq!(w.fees, fees);
    let group = w.env.market_state().1;
    let a = group.assets[0];
    assert_eq!(
        [a.oi_eff_long_q, a.oi_eff_short_q],
        [percolator::MAX_OI_SIDE_Q - headroom; 2]
    );
    assert_eq!(
        [a.stored_pos_count_long, a.stored_pos_count_short],
        [2; 2],
        "both disjoint pairs retain existing legs"
    );
    assert_eq!(w.positions[4..], [[0; ASSETS]; 2]);
    let side_fees = fees.iter().sum::<u128>() / 2;
    assert_eq!(w.domains, [side_fees, side_fees, 0, 0]);
}

#[test]
fn v16_existing_pairs_compete_for_fee_bearing_side_headroom_across_pair_order() {
    let half = i128::try_from(percolator::MAX_OI_SIDE_Q / 2).unwrap();
    let amounts = [RELEASE_Q as i128, RELEASE_Q as i128 - 2];
    assert_eq!(half as u128 * 2, percolator::MAX_OI_SIDE_Q);
    assert!(amounts.iter().all(|&q| q > 1 && q < half));
    assert_eq!(amounts.map(notional), [73, 72]);
    assert_eq!(
        amounts.map(|q| ceil_ratio(notional(q) * u128::from(FEE_BPS), 10_000)),
        [2, 1]
    );
    assert_eq!(
        ceil_ratio(
            RELEASE_Q * u128::from(PRICE) * u128::from(FEE_BPS),
            POS_SCALE * 10_000
        ),
        1,
        "the first fee requires both ceilings"
    );
    let routes = [
        [TradeRoute::NoCpi, TradeRoute::BatchCpi],
        [TradeRoute::BatchCpi, TradeRoute::NoCpi],
        [TradeRoute::Cpi, TradeRoute::BatchNoCpi],
        [TradeRoute::BatchNoCpi, TradeRoute::Cpi],
    ];
    let mut peaks = [0; 3];
    let mut worlds = 0;
    let mut rollbacks = 0;
    for direction in [-1i128, 1] {
        for order in [[0usize, 2], [2, 0]] {
            for pair_routes in routes {
                let label = format!("direction={direction} order={order:?} routes={pair_routes:?}");
                let mut w = World::new();
                w.check();
                for pair in [0, 2] {
                    let opening = [(0, direction * (half - amounts[pair / 2]))];
                    let ixs = w.instructions(pair, TradeRoute::NoCpi, &opening, 0);
                    w.accept(&ixs, &[pair]);
                    w.record(pair, &opening, 0, 1);
                    w.check();
                }
                w.env.update_trade_fee_policy_with_cu(FEE_BPS);
                for pair in order {
                    prepare(&mut w, pair, pair_routes[pair / 2]);
                }
                checkpoint(&w, amounts.iter().sum::<i128>() as u128, [0; ACTORS]);

                let [first, second] = order;
                let fills = amounts.map(|q| [(0, direction * q)]);
                let first_ixs =
                    w.instructions(first, pair_routes[first / 2], &fills[first / 2], FEE_BPS);
                let second_ixs =
                    w.instructions(second, pair_routes[second / 2], &fills[second / 2], FEE_BPS);
                let excess = direction * (amounts[second / 2] + 1);
                admissible(&w, first, fills[first / 2][0].1);
                admissible(&w, second, excess);
                let over = w.instructions(second, pair_routes[second / 2], &[(0, excess)], FEE_BPS);
                let mut bundle = first_ixs.clone();
                bundle.extend(over.clone());
                w.reject(
                    &bundle,
                    PercolatorError::EngineInvalidLeg as u32,
                    1,
                    pair_routes.into_iter().map(cpis).sum(),
                );
                rollbacks += 1;
                checkpoint(&w, amounts.iter().sum::<i128>() as u128, [0; ACTORS]);

                // Reuse the already-built requests after rollback; only the enclosing
                // transaction/blockhash is renewed. No account bytes are restored.
                w.accept(&first_ixs, &[first]);
                w.record(first, &fills[first / 2], FEE_BPS, 1);
                let mut expected_fees = [0; ACTORS];
                expected_fees[first..first + 2].fill(if first == 0 { 2 } else { 1 });
                checkpoint(&w, amounts[second / 2] as u128, expected_fees);
                w.reject(
                    &over,
                    PercolatorError::EngineInvalidLeg as u32,
                    0,
                    cpis(pair_routes[second / 2]),
                );
                rollbacks += 1;
                checkpoint(&w, amounts[second / 2] as u128, expected_fees);
                w.accept(&second_ixs, &[second]);
                w.record(second, &fills[second / 2], FEE_BPS, 1);
                checkpoint(&w, 0, [2, 2, 1, 1, 0, 0]);
                assert_eq!(
                    w.positions[..4],
                    [
                        [direction * half, 0],
                        [-direction * half, 0],
                        [direction * half, 0],
                        [-direction * half, 0]
                    ]
                );
                assert_eq!(w.env.market_state().1.c_tot, SUPPLY - 6);
                assert_eq!(w.env.market_state().1.insurance, 6);

                for pair in order {
                    prepare(&mut w, pair, pair_routes[pair / 2]);
                    admissible(&w, pair, direction);
                    let extra =
                        w.instructions(pair, pair_routes[pair / 2], &[(0, direction)], FEE_BPS);
                    w.reject(
                        &extra,
                        PercolatorError::EngineInvalidLeg as u32,
                        0,
                        cpis(pair_routes[pair / 2]),
                    );
                    rollbacks += 1;
                    checkpoint(&w, 0, [2, 2, 1, 1, 0, 0]);
                }

                w.env.update_trade_fee_policy_with_cu(0);
                for pair in order {
                    let close = [(0, -direction * half)];
                    let ixs = w.instructions(pair, TradeRoute::NoCpi, &close, 0);
                    w.accept(&ixs, &[pair]);
                    w.record(pair, &close, 0, 1);
                    w.check();
                }
                for actor in 0..ACTORS {
                    let amount = CAPITAL - [2, 2, 1, 1, 0, 0][actor];
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
                    let tx = w.transaction(&[ix]);
                    let before = frame(&w.env, &tx, &w.keys());
                    let fee = FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let meta = w.env.svm.send_transaction(tx).unwrap();
                    check_frame(
                        &w.env,
                        before,
                        fee,
                        &[
                            w.env.market,
                            w.portfolios[actor],
                            w.tokens[actor],
                            w.env.vault,
                        ],
                    );
                    w.paid[actor] = amount;
                    w.check();
                    assert_cu_within(
                        "existing-pair payout",
                        meta.compute_units_consumed,
                        CUSTODY_CU_LIMIT,
                    );
                    w.peak[2] = w.peak[2].max(meta.compute_units_consumed);
                }
                assert_eq!(w.positions, [[0; ASSETS]; ACTORS]);
                assert_eq!(w.env.market_state().1.c_tot, 0);
                assert_eq!(w.env.market_state().1.vault, 6);
                for (peak, observed) in peaks.iter_mut().zip(w.peak) {
                    *peak = (*peak).max(observed);
                }
                worlds += 1;
                println!("INV-058 fee-bearing existing pairs {label}: four rollbacks, six payouts");
            }
        }
    }
    assert_eq!((worlds, rollbacks), (16, 64));
    println!("INV-058 existing-pair fee competition: worlds={worlds}, rollbacks={rollbacks}, payouts={}, peak CU reject/trade/custody={peaks:?}", worlds * ACTORS);
}
