//! INV-058 / row427: generated cumulative admission on existing disjoint-pair legs.
//! Compare merged and partitioned public fills with a raw OI/notional census at
//! every commit and complete Account rollback at aggregate max-plus-one.
//! Fixed marks, unit ADL and zero fees isolate the shared quantity bound; elapsed
//! rates, nonzero PnL/funding and maximum portfolio shapes remain outside coverage.

use super::*;
use rand::{seq::SliceRandom, Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

struct Event {
    source: usize,
    recipient: usize,
    amount: [i128; ASSETS],
    release_parts: Vec<[i128; ASSETS]>,
    refill_parts: Vec<[i128; ASSETS]>,
    release_route: usize,
    refill_route: usize,
}

fn partition(rng: &mut XorShiftRng, amount: [i128; ASSETS]) -> Vec<[i128; ASSETS]> {
    let count = rng.gen_range(2..=3);
    let mut left = amount;
    let mut parts = Vec::new();
    for remaining in (1..count).rev() {
        let part = std::array::from_fn(|asset| {
            let q = rng.gen_range(1..=left[asset].abs() - remaining as i128);
            left[asset].signum() * q
        });
        for asset in 0..ASSETS {
            left[asset] -= part[asset];
        }
        parts.push(part);
    }
    parts.push(left);
    assert_eq!(
        std::array::from_fn::<_, ASSETS, _>(|asset| parts.iter().map(|p| p[asset]).sum::<i128>()),
        amount
    );
    parts
}

fn history(seed: u64, route_offset: usize, direction: i128) -> ([[i128; ASSETS]; 3], Vec<Event>) {
    let mut rng = XorShiftRng::seed_from_u64(seed);
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let mut initial = [[0; ASSETS]; 3];
    for asset in 0..ASSETS {
        let a = rng.gen_range(max / 5..max / 3);
        let b = rng.gen_range(max / 5..max / 3);
        let sign = direction * if asset == 0 { 1 } else { -1 };
        for (pair, q) in [a, b, max - a - b].into_iter().enumerate() {
            initial[pair][asset] = sign * q;
        }
    }
    let mut edges = [(0, 2), (0, 4), (2, 0), (2, 4), (4, 0), (4, 2)];
    edges.shuffle(&mut rng);
    let mut positions = initial;
    let mut events = Vec::new();
    for (index, (source, recipient)) in edges.into_iter().enumerate() {
        let amount = std::array::from_fn(|asset| {
            let available = positions[source / 2][asset];
            let q = match index {
                1 => POS_SCALE as i128 / PRICE as i128 - 1 + 2 * asset as i128,
                4 => POS_SCALE as i128 / PRICE as i128 + 1 - 2 * asset as i128,
                _ => rng.gen_range(4..=available.abs() / 8),
            };
            assert!(q >= 4 && q < available.abs());
            available.signum() * q
        });
        let release_parts = partition(&mut rng, amount.map(|q| -q));
        let refill_parts = partition(&mut rng, amount.map(|q| q - q.signum()));
        for asset in 0..ASSETS {
            positions[source / 2][asset] -= amount[asset];
            positions[recipient / 2][asset] += amount[asset];
        }
        events.push(Event {
            source,
            recipient,
            amount,
            release_parts,
            refill_parts,
            release_route: index % INV_058_TRADE_ROUTES.len(),
            refill_route: (route_offset + index / 4) % INV_058_TRADE_ROUTES.len(),
        });
    }
    (initial, events)
}

fn legs(amount: [i128; ASSETS], split: bool) -> Legs {
    let order = if split { [1, 0] } else { [0, 1] };
    order.map(|asset| (asset as u16, amount[asset])).to_vec()
}

fn prepare_route(w: &mut World, pair: usize, route: TradeRoute) {
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
        w.check();
    }
}

fn fill(w: &mut World, pair: usize, route: TradeRoute, amount: [i128; ASSETS], split: bool) {
    prepare_route(w, pair, route);
    let input = legs(amount, split);
    let ixs = w.instructions(pair, route, &input, 0);
    // Check the raw census between singles as well as between transaction partitions.
    if matches!(route, TradeRoute::NoCpi | TradeRoute::Cpi) {
        for leg in input {
            let ix = w.instructions(pair, route, &[leg], 0);
            w.accept(&ix, &[pair]);
            w.record(pair, &[leg], 0, 1);
            w.check();
        }
    } else {
        w.accept(&ixs, &[pair]);
        w.record(pair, &input, 0, 1);
        w.check();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Projection {
    owners: [([i128; ASSETS], u128, i128, u128); ACTORS],
    oi: [[u128; 2]; ASSETS],
    counts: [[u64; 2]; ASSETS],
    stock: [u128; 3],
}

fn checkpoint(w: &World, headroom: u128) -> Projection {
    w.check();
    let group = w.env.market_state().1;
    let owners = w.portfolios.map(|key| {
        let p = w.env.portfolio_state(key);
        let q = std::array::from_fn(|asset| active_leg_for_asset(&p, asset).basis_pos_q);
        assert!(q
            .iter()
            .all(|n| n.unsigned_abs() < percolator::MAX_POSITION_ABS_Q));
        (
            q,
            p.capital.get(),
            p.pnl.get(),
            health_cert(&p).certified_worst_case_loss,
        )
    });
    let oi = std::array::from_fn(|asset| {
        let a = group.assets[asset];
        let observed = [a.oi_eff_long_q, a.oi_eff_short_q];
        assert_eq!(observed, [percolator::MAX_OI_SIDE_Q - headroom; 2]);
        observed
    });
    let counts = std::array::from_fn(|asset| {
        let a = group.assets[asset];
        let observed = [a.stored_pos_count_long, a.stored_pos_count_short];
        assert_eq!(observed, [3; 2], "every pair retains both existing legs");
        observed
    });
    Projection {
        owners,
        oi,
        counts,
        stock: [group.c_tot, group.vault, group.insurance],
    }
}

fn assert_local_admissibility(w: &World, pair: usize, input: &Legs) {
    let mut proposal = w.positions[pair];
    for &(asset, q) in input {
        assert!(q.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
        proposal[asset as usize] += q;
    }
    assert!(proposal
        .iter()
        .all(|q| q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q));
    let risk: u128 = proposal.into_iter().map(notional).sum();
    assert!(risk < percolator::MAX_ACCOUNT_NOTIONAL && risk < CAPITAL);
    assert_eq!(w.positions[pair + 1], w.positions[pair].map(|q| -q));
}

#[test]
fn v16_program_generated_existing_pair_side_oi_caps_compose_across_split_merge_routes() {
    let mut worlds = 0;
    let mut rejects = 0;
    let mut peaks = [0; 3];
    let mut route_pairs = [[false; 4]; 4];
    for (seed_index, seed) in [427, 427058, 427052, 427080].into_iter().enumerate() {
        for direction in [-1, 1] {
            let (initial, events) = history(seed, seed_index, direction);
            let mut reference = None;
            for split in [false, true] {
                let label = format!("seed={seed} direction={direction} split={split}");
                println!("INV-058 existing-pair {label}");
                let mut w = World::new();
                w.check();
                for (pair, q) in initial.into_iter().enumerate() {
                    fill(&mut w, pair * 2, TradeRoute::BatchNoCpi, q, split);
                }
                let mut checkpoints = vec![checkpoint(&w, 0)];
                for (index, event) in events.iter().enumerate() {
                    let release_route = INV_058_TRADE_ROUTES[event.release_route];
                    let refill_route = INV_058_TRADE_ROUTES[event.refill_route];
                    route_pairs[event.release_route][event.refill_route] = true;
                    prepare_route(&mut w, event.source, release_route);
                    prepare_route(&mut w, event.recipient, refill_route);
                    let release = legs(event.amount.map(|q| -q), split);
                    let mut over = legs(event.amount, split);
                    over.last_mut().unwrap().1 += over.last().unwrap().1.signum();
                    assert_local_admissibility(&w, event.source, &release);
                    assert_local_admissibility(&w, event.recipient, &over);
                    let reduce = w.instructions(event.source, release_route, &release, 0);
                    let refill = w.instructions(event.recipient, refill_route, &over, 0);
                    let cpis = [(release_route, reduce.len()), (refill_route, refill.len())]
                        .into_iter()
                        .filter(|(r, _)| matches!(r, TradeRoute::Cpi | TradeRoute::BatchCpi))
                        .map(|(_, n)| n)
                        .sum();
                    let mut bad = reduce;
                    bad.extend(refill);
                    w.reject(
                        &bad,
                        PercolatorError::EngineInvalidLeg as u32,
                        bad.len() - 1,
                        cpis,
                    );
                    rejects += 1;
                    assert_eq!(checkpoint(&w, 0), *checkpoints.last().unwrap(), "{label}");

                    for (pair, parts, merged, offset) in [
                        (
                            event.source,
                            &event.release_parts,
                            event.amount.map(|q| -q),
                            event.release_route,
                        ),
                        (
                            event.recipient,
                            &event.refill_parts,
                            event.amount.map(|q| q - q.signum()),
                            event.refill_route,
                        ),
                    ] {
                        if split {
                            for (part, &q) in parts.iter().enumerate() {
                                let route = INV_058_TRADE_ROUTES[(offset + part) % 4];
                                fill(&mut w, pair, route, q, split);
                            }
                        } else {
                            fill(&mut w, pair, TradeRoute::BatchNoCpi, merged, split);
                        }
                    }
                    checkpoints.push(checkpoint(&w, 1));

                    // The unused pair also competes for the same last atom on each side.
                    let bystander = 6 - event.source - event.recipient;
                    for route in INV_058_TRADE_ROUTES {
                        prepare_route(&mut w, bystander, route);
                        let over = legs(event.amount.map(|q| 2 * q.signum()), split);
                        assert_local_admissibility(&w, bystander, &over);
                        let bad = w.instructions(bystander, route, &over[..1], 0);
                        w.reject(
                            &bad,
                            PercolatorError::EngineInvalidLeg as u32,
                            0,
                            usize::from(matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)),
                        );
                        rejects += 1;
                    }
                    fill(
                        &mut w,
                        event.recipient,
                        if split {
                            INV_058_TRADE_ROUTES[index % 4]
                        } else {
                            TradeRoute::BatchNoCpi
                        },
                        event.amount.map(i128::signum),
                        split,
                    );
                    checkpoints.push(checkpoint(&w, 0));
                }
                if let Some(expected) = &reference {
                    assert_eq!(
                        &checkpoints, expected,
                        "{label}: merged/partitioned economics"
                    );
                } else {
                    reference = Some(checkpoints);
                }
                for pair in [4, 2, 0] {
                    let close = w.positions[pair].map(|q| -q);
                    fill(&mut w, pair, TradeRoute::BatchNoCpi, close, split);
                }
                let group = w.env.market_state().1;
                assert!(group.assets[..ASSETS].iter().all(|a| a.oi_eff_long_q == 0
                    && a.oi_eff_short_q == 0
                    && a.stored_pos_count_long == 0
                    && a.stored_pos_count_short == 0));
                for (peak, observed) in peaks.iter_mut().zip(w.peak) {
                    *peak = (*peak).max(observed);
                }
                worlds += 1;
            }
        }
    }
    assert!(route_pairs.into_iter().flatten().all(|seen| seen));
    assert_eq!((worlds, rejects), (16, 480));
    println!("INV-058 generated existing-pair composition: {worlds} worlds, {rejects} exact rollbacks, 104 paired economic checkpoints; peak CU [reject, trade, custody]={peaks:?}");
}
