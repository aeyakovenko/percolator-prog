//! INV-045 / row 425: signed reductions before an outstanding AuthMark crank.
//! Bounded INV-024/038/041/052/071/085/086/088 composition, not generic closure.

use super::*;

#[test]
fn v16_program_row425_precrank_reductions_preserve_carry_and_owner_entitlement() {
    let mut worlds = 0;
    let mut max_cu = 0;
    for direction in [-1, 1] {
        for early_parity in 0..2 {
            for early_lots in [1, 2] {
                let mut baseline = None;
                for batch in [false, true] {
                    for reverse in [false, true] {
                        let history = History {
                            direction,
                            batch,
                            split: early_lots == 1,
                            placement: early_parity,
                            reverse,
                        };
                        let mut world = World::new(history);
                        world.trace.push(format!(
                            "precrank schedule: early_parity={early_parity}, early_lots={early_lots}"
                        ));
                        world.check(history, 0);
                        let mut pending_carry_trades = 0;
                        for slot in 1..=5 {
                            world.trace.push(format!("Clock.slot={slot}"));
                            world.env.svm.warp_to_slot(slot);
                            let before_crank = if slot as usize % 2 == early_parity {
                                early_lots
                            } else {
                                0
                            };
                            if before_crank != 0 {
                                let market = world.env.svm.get_account(&world.env.market).unwrap();
                                let profiles = [0, 1].map(|asset| {
                                    state::read_asset_oracle_profile(&market.data, asset).unwrap()
                                });
                                pending_carry_trades += usize::from(
                                    profiles.iter().all(|p| p.price_move_remainder_bps_num != 0),
                                );
                                // Clock advances independently of the market frontier. These
                                // zero-funding trades retain that frontier and its carried atoms.
                                world.reduce(history, slot - 1, before_crank);
                                let market = world.env.svm.get_account(&world.env.market).unwrap();
                                assert_eq!(
                                    [0, 1].map(|asset| {
                                        state::read_asset_oracle_profile(&market.data, asset)
                                            .unwrap()
                                    }),
                                    profiles,
                                    "precrank trade must frame both complete profiles: {:?}",
                                    world.trace
                                );
                            }
                            // The input ledger now holds the quantities that actually cross the
                            // next price atom; it never credits the already reduced quantity.
                            world.crank(2, history, slot);
                            if before_crank < 2 {
                                world.reduce(history, slot, 2 - before_crank);
                            }
                            for actor in if reverse { [1, 0, 3] } else { [0, 1, 3] } {
                                world.crank(actor, history, slot);
                            }
                        }
                        assert!(pending_carry_trades >= 2);
                        assert!(world.latent_checks > 0);
                        let endpoint = world.check(history, 5);
                        assert_eq!(endpoint.carry, [2_000, 5_000]);
                        assert_eq!(endpoint.lots, [[3, 7], [-3, -7], [7, 11], [-7, -11]]);
                        // Asset 1 moves at slot 4 and asset 0 at slot 5. Earlier reduction
                        // changes which owner's live lots receive that atom, with opposite signs.
                        let timing_delta = if early_parity == 0 {
                            early_lots
                        } else {
                            -early_lots
                        };
                        let active_pnl = (-6 + timing_delta) * direction;
                        assert_eq!(
                            endpoint.entitlement,
                            [
                                PRINCIPAL[0] as i128 + active_pnl,
                                PRINCIPAL[1] as i128 - active_pnl,
                                PRINCIPAL[2] as i128 - 4 * direction,
                                PRINCIPAL[3] as i128 + 4 * direction,
                            ]
                        );
                        assert_ne!(active_pnl, -6 * direction);
                        if let Some(expected) = &baseline {
                            assert_eq!(&endpoint, expected, "{:?}", world.trace);
                        } else {
                            baseline = Some(endpoint);
                        }
                        max_cu = max_cu.max(world.max_cu);
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert_cu_within("row425 precrank carry/entitlement", max_cu, 1_400_000);
    eprintln!("row425 precrank: {worlds} histories, max transition CU={max_cu}");
}
