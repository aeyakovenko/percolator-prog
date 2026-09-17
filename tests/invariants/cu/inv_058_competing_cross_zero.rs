//! INV-058 / row 427: direct cross-zero at a cap shared by three disjoint pairs.
//! A retained flip competes at the transient attach boundary, including a
//! rolled-back competing refill and bounded close/reopen at the cap.
//! Public construction, fixed mark, unit ADL, zero fees.

use super::*;

fn cpis(route: TradeRoute) -> usize {
    usize::from(matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi))
}

fn packet_bytes(w: &mut World, ixs: &[Instruction]) -> u64 {
    let bytes = bincode::serialized_size(&w.transaction(ixs)).unwrap();
    assert!(bytes <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    bytes
}

fn admissible(w: &World, pair: usize, delta: i128) {
    assert!(delta.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
    for (actor, change) in [(pair, delta), (pair + 1, -delta)] {
        let proposed = w.positions[actor][0] + change;
        assert!(proposed.unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
        assert!(notional(proposed) < percolator::MAX_ACCOUNT_NOTIONAL);
        assert!(notional(proposed) < CAPITAL);
    }
}

fn fill(w: &mut World, pair: usize, route: TradeRoute, delta: i128) {
    admissible(w, pair, delta);
    let legs = [(0, delta)];
    let ixs = w.instructions(pair, route, &legs, 0);
    packet_bytes(w, &ixs);
    w.accept(&ixs, &[pair]);
    w.record(pair, &legs, 0, 1);
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
        [3; 2],
        "cross-zero retains all three disjoint pairs"
    );
}

#[test]
fn v16_program_direct_cross_zero_competes_for_shared_side_oi_headroom() {
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let q = max / 4;
    let initial = [q, max / 3, max - q - max / 3];
    assert!(q > 2 && 2 * q + 1 < max);
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut flips = 0;
    let mut split_reversals = 0;
    let mut peaks = [0; 3];
    let mut max_bundle_bytes = 0;
    for direction in [-1i128, 1] {
        for [releaser, competitor] in [[2usize, 4], [4, 2]] {
            for (index, flip_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
                let release_route = INV_058_TRADE_ROUTES[(index + 1) % 4];
                let competing_route = INV_058_TRADE_ROUTES[(index + 2) % 4];
                let mut w = World::new();
                for (pair, amount) in initial.into_iter().enumerate() {
                    fill(&mut w, pair * 2, TradeRoute::NoCpi, direction * amount);
                }
                for (pair, route) in [
                    (0, flip_route),
                    (releaser, release_route),
                    (competitor, competing_route),
                ] {
                    if cpis(route) != 0 {
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
                }
                checkpoint(&w, 0);

                // The engine attaches the first new-side leg before removing
                // its peer's old leg, so even a net-neutral flip needs headroom.
                let neutral = w.instructions(0, flip_route, &[(0, -direction * 2 * q)], 0);
                packet_bytes(&mut w, &neutral);
                admissible(&w, 0, -direction * 2 * q);
                w.reject(
                    &neutral,
                    PercolatorError::EngineInvalidLeg as u32,
                    0,
                    cpis(flip_route),
                );
                rollbacks += 1;
                checkpoint(&w, 0);

                let cross = -direction * (2 * q + 1);
                admissible(&w, 0, cross);
                assert_eq!(w.positions[0][0] + cross, -direction * (q + 1));
                let retained = w.instructions(0, flip_route, &[(0, cross)], 0);
                packet_bytes(&mut w, &retained);
                w.reject(
                    &retained,
                    PercolatorError::EngineInvalidLeg as u32,
                    0,
                    cpis(flip_route),
                );
                rollbacks += 1;
                checkpoint(&w, 0);

                let release_q = -direction * (q + 1);
                admissible(&w, releaser, release_q);
                admissible(&w, competitor, direction);
                let release = w.instructions(releaser, release_route, &[(0, release_q)], 0);
                packet_bytes(&mut w, &release);
                w.accept(&release, &[releaser]);
                w.record(releaser, &[(0, release_q)], 0, 1);
                checkpoint(&w, (q + 1) as u128);

                let compete = w.instructions(competitor, competing_route, &[(0, direction)], 0);
                let mut bundle = compete;
                bundle.extend(retained.clone());
                max_bundle_bytes = max_bundle_bytes.max(packet_bytes(&mut w, &bundle));
                w.reject(
                    &bundle,
                    PercolatorError::EngineInvalidLeg as u32,
                    1,
                    cpis(competing_route) + cpis(flip_route),
                );
                rollbacks += 1;
                checkpoint(&w, (q + 1) as u128);

                // Both failed attempts preserve this exact request's epochs and
                // matcher sequence. Only the enclosing blockhash is refreshed.
                w.accept(&retained, &[0]);
                w.record(0, &[(0, cross)], 0, 1);
                assert_eq!(w.positions[0][0], -direction * (q + 1));
                assert_eq!(
                    w.positions[competitor][0],
                    direction * initial[competitor / 2]
                );
                checkpoint(&w, q as u128);
                flips += 1;

                // The next direct flip shrinks exposure by two atoms. Another
                // existing pair can consume exactly that released capacity.
                fill(&mut w, 0, flip_route, direction * 2 * q);
                assert_eq!(w.positions[0][0], direction * (q - 1));
                checkpoint(&w, (q + 2) as u128);
                flips += 1;
                fill(&mut w, competitor, competing_route, direction * (q + 2));
                checkpoint(&w, 0);
                admissible(&w, releaser, direction);
                let extra = w.instructions(releaser, release_route, &[(0, direction)], 0);
                packet_bytes(&mut w, &extra);
                w.reject(
                    &extra,
                    PercolatorError::EngineInvalidLeg as u32,
                    0,
                    cpis(release_route),
                );
                rollbacks += 1;
                checkpoint(&w, 0);

                // At the shared cap, the owner pair still has a two-call
                // reversal that leaves the disjoint competitors untouched.
                fill(&mut w, 0, flip_route, -direction * (q - 1));
                let asset = w.env.market_state().1.assets[0];
                assert_eq!(
                    [asset.oi_eff_long_q, asset.oi_eff_short_q],
                    [(max - q + 1) as u128; 2]
                );
                fill(&mut w, 0, flip_route, -direction * (q - 1));
                assert_eq!(w.positions[0][0], -direction * (q - 1));
                checkpoint(&w, 0);
                split_reversals += 1;

                for (pair, route) in [
                    (competitor, competing_route),
                    (0, flip_route),
                    (releaser, release_route),
                ] {
                    let close = -w.positions[pair][0];
                    fill(&mut w, pair, route, close);
                }
                assert_eq!(w.positions, [[0; ASSETS]; ACTORS]);
                let asset = w.env.market_state().1.assets[0];
                assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], [0; 2]);
                assert_eq!(
                    [asset.stored_pos_count_long, asset.stored_pos_count_short],
                    [0; 2]
                );
                for (peak, observed) in peaks.iter_mut().zip(w.peak) {
                    *peak = (*peak).max(observed);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(
        (worlds, flips, split_reversals, rollbacks),
        (16, 32, 16, 64)
    );
    println!("INV-058 competing cross-zero: {worlds} worlds, {flips} direct flips, {split_reversals} bounded split reversals, {rollbacks} exact rollbacks; peak CU [reject, trade, custody]={peaks:?}; max bundle bytes={max_bundle_bytes}");
}
