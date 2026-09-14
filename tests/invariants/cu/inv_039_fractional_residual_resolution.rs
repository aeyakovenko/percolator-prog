//! INV-037/038/039/066/076: fractional cohort carry is not an owner's payout.
//! Two unequal fractional holders share one bankrupt residual through resolution.
//! Input arithmetic separates price floors, B booking/settlement remainders and
//! terminal cash; the insured owner deliberately uses exact ratios instead.
//! Matched versus Recovery peer exits also separate source-conversion rounding
//! from common receipt floors. Nominal face is not assumed to be payable cash.
//! No ADL, funding, fees, insurance, close drift or arbitrary-history claim.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
    verify_close_residual_partition,
};
use percolator::{BOUND_SCALE, CREDIT_RATE_SCALE, SOCIAL_LOSS_DEN};

#[path = "inv_039_fractional_cohort_recreation.rs"]
mod cohort_recreation;

#[derive(Clone, Copy, Debug)]
struct Inputs {
    weights: [u128; 2],
    residual: u128,
}

impl Inputs {
    const MOVEMENT: u128 = 200_000;

    fn gains(self) -> [u128; 2] {
        self.weights.map(|q| q * Self::MOVEMENT / POS_SCALE)
    }

    fn debts(self) -> [u128; 2] {
        self.weights
            .map(|q| (q * Self::MOVEMENT).div_ceil(POS_SCALE))
    }

    fn deposits(self) -> [u128; 5] {
        [
            200_000,
            self.debts()[0] - self.residual,
            300_000,
            250_000,
            777,
        ]
    }

    fn target(self) -> u128 {
        self.residual * SOCIAL_LOSS_DEN / self.weights.iter().sum::<u128>()
    }

    fn booking_remainder(self) -> u128 {
        self.residual * SOCIAL_LOSS_DEN % self.weights.iter().sum::<u128>()
    }

    fn losses(self) -> [u128; 2] {
        self.weights.map(|q| q * self.target() / SOCIAL_LOSS_DEN)
    }

    fn carries(self) -> [u128; 2] {
        self.weights.map(|q| q * self.target() % SOCIAL_LOSS_DEN)
    }

    fn payouts(self) -> [u128; 5] {
        let d = self.deposits();
        let g = self.gains();
        let losses = self.losses();
        [
            d[0] + g[0] - losses[0],
            0,
            d[2] + g[1] - losses[1],
            d[3] - self.debts()[1],
            d[4],
        ]
    }

    fn cash_residue(self) -> u128 {
        let price_rounding = self.debts().iter().sum::<u128>() - self.gains().iter().sum::<u128>();
        let social_loss_carry = self.residual - self.losses().iter().sum::<u128>();
        price_rounding
            .checked_sub(social_loss_carry)
            .expect("chosen fully funded endpoints")
    }
}

struct Book {
    input: Inputs,
    recovery_peer: bool,
    recreated_capital: u128,
    booked: bool,
    settled: [bool; 2],
    detached: [bool; 2],
    deleted: [bool; 5],
    converted: [Option<u128>; 2],
}

impl Book {
    fn owner_entitlements(&self) -> [u128; 5] {
        let mut expected = self.input.payouts();
        expected[1] += self.recreated_capital;
        expected[4] -= self.recreated_capital;
        expected
    }

    fn source_backing(&self) -> u128 {
        self.input.deposits()[1]
            + if self.recovery_peer {
                0
            } else {
                self.input.debts()[1]
            }
    }

    fn junior_pool(&self) -> u128 {
        if self.recovery_peer {
            self.input.debts()[1]
        } else {
            0
        }
    }

    fn faces(&self) -> [u128; 2] {
        std::array::from_fn(|i| {
            self.input.gains()[i]
                - self.input.losses()[i]
                - self.converted[i].expect("both source conversions precede terminal allocation")
        })
    }

    fn payouts(&self) -> [u128; 5] {
        // A fully source-realized owner may exit before the other conversion.
        // No terminal receipt haircut is allowed before all source faces refine.
        if self.converted.iter().any(Option::is_none) {
            return self.owner_entitlements();
        }
        let faces = self.faces();
        let total = faces.iter().sum::<u128>();
        let mut payouts = self.owner_entitlements();
        for i in 0..2 {
            let junior = if total == 0 {
                0
            } else {
                faces[i] * self.junior_pool().min(total) / total
            };
            payouts[2 * i] -= faces[i] - junior;
        }
        payouts
    }

    fn record_conversion(&mut self, pair: usize) {
        let backing = self.source_backing() - self.converted.iter().flatten().sum::<u128>();
        let faces: [u128; 2] = std::array::from_fn(|i| {
            if self.converted[i].is_some() {
                0
            } else {
                self.input.gains()[i]
                    - if self.settled[i] {
                        self.input.losses()[i]
                    } else {
                        0
                    }
            }
        });
        let rate =
            (backing * CREDIT_RATE_SCALE / faces.iter().sum::<u128>()).min(CREDIT_RATE_SCALE);
        self.converted[pair] = Some(faces[pair] * rate / CREDIT_RATE_SCALE);
    }

    fn check(&self, world: &AttributionWorld) {
        let input = self.input;
        let group = world.env.market_state().1;
        let side = usize::from(world.quantities[0] < 0);
        let asset = group.assets[1];
        let b = [asset.b_long_num, asset.b_short_num];
        let rem = [
            asset.social_loss_remainder_long_num,
            asset.social_loss_remainder_short_num,
        ];
        let dust = [
            asset.social_loss_dust_long_num,
            asset.social_loss_dust_short_num,
        ];
        let explicit = [
            asset.explicit_unallocated_loss_long,
            asset.explicit_unallocated_loss_short,
        ];
        let target = if self.booked { input.target() } else { 0 };
        let market_rem = if self.booked {
            input.booking_remainder()
        } else {
            0
        };
        let detached_carry: u128 = (0..2)
            .filter(|i| self.detached[*i])
            .map(|i| input.carries()[i])
            .sum();
        assert_eq!(b[side], target);
        assert_eq!(rem[side], market_rem);
        assert_eq!(dust[side], detached_carry % SOCIAL_LOSS_DEN);
        assert_eq!(explicit[side], detached_carry / SOCIAL_LOSS_DEN);
        assert_eq!(
            (
                b[1 - side],
                rem[1 - side],
                dust[1 - side],
                explicit[1 - side]
            ),
            (0, 0, 0, 0)
        );
        if self.booked {
            let mut allocated = market_rem + dust[side] + explicit[side] * SOCIAL_LOSS_DEN;
            for pair in 0..2 {
                allocated += if self.settled[pair] {
                    input.losses()[pair] * SOCIAL_LOSS_DEN
                        + if self.detached[pair] {
                            0
                        } else {
                            input.carries()[pair]
                        }
                } else {
                    input.weights[pair] * target
                };
            }
            assert_eq!(allocated, input.residual * SOCIAL_LOSS_DEN,
                "B allocation, live obligations, detached dust and booking remainder partition the original residual");
        }
        let mut accounts = Vec::new();
        let mut weight = [0u128; 2];
        let mut pending = [0u64; 2];
        for (actor, a) in world.actors.iter().enumerate() {
            let wallet = world.env.token_amount(a.token) as u128;
            if self.deleted[actor] {
                let expected = if actor == 0 || actor == 2 {
                    self.payouts()[actor]
                } else {
                    self.owner_entitlements()[actor]
                };
                assert_eq!(wallet, expected);
                assert!(world
                    .env
                    .svm
                    .get_account(&a.portfolio)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                continue;
            }
            let account = world.env.portfolio_state(a.portfolio);
            assert_eq!(account.owner, a.owner.pubkey().to_bytes());
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert!(actor == 0 || actor == 2);
                assert!(self.booked && self.detached[actor / 2]);
                let face = input.gains()[actor / 2]
                    - input.losses()[actor / 2]
                    - self.converted[actor / 2].unwrap();
                assert_eq!(receipt.terminal_positive_claim_face, face);
                assert_eq!(receipt.prior_bound_contribution_num, face * BOUND_SCALE);
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            let expected = match actor {
                0 | 2 => {
                    if self.converted.iter().all(Option::is_some)
                        && resolved_portfolio_is_terminal(&world.env, a.portfolio)
                    {
                        self.payouts()[actor] as i128
                    } else {
                        (input.deposits()[actor] + input.gains()[actor / 2]
                            - if self.settled[actor / 2] {
                                input.losses()[actor / 2]
                            } else {
                                0
                            }) as i128
                    }
                }
                1 if !self.booked => -(input.residual as i128),
                _ => self.owner_entitlements()[actor] as i128,
            };
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + due as i128 + wallet as i128,
                expected,
                "owner {actor}: {input:?}, booked={}, settled={:?}, detached={:?}, capital={}, pnl={}, wallet={wallet}, receipt={receipt:?}, payout={:?}",
                self.booked,
                self.settled,
                self.detached,
                account.capital.get(),
                account.pnl.get(),
                group.resolved_payout_ledger
            );
            assert!(wallet <= self.owner_entitlements()[actor]);
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
                .collect();
            let retained = (actor == 0 || actor == 2) && !self.detached[actor / 2];
            assert_eq!(legs.len(), usize::from(retained));
            if retained {
                let pair = actor / 2;
                let leg = legs[0];
                assert_eq!(leg.asset_index, 1);
                assert_eq!(
                    leg.side,
                    if side == 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(leg.basis_pos_q, 0);
                assert_eq!(leg.loss_weight, input.weights[pair]);
                assert_eq!(
                    leg.b_snap,
                    if self.settled[pair] {
                        input.target()
                    } else {
                        0
                    }
                );
                assert_eq!(
                    leg.b_rem,
                    if self.settled[pair] {
                        input.carries()[pair]
                    } else {
                        0
                    }
                );
                weight[side] += input.weights[pair];
                pending[side] += 1;
            }
            let close = close_progress(&account);
            verify_close_residual_partition("INV-039 fractional residual", &close).unwrap();
            if actor == 1 && self.recreated_capital == 0 {
                assert!(close.active && !close.canceled);
                assert_eq!(close.finalized, self.booked);
                assert_eq!(close.gross_loss_at_close_start, input.residual);
                assert_eq!(
                    close.b_loss_booked,
                    if self.booked { input.residual } else { 0 }
                );
                assert_eq!(
                    close.residual_remaining,
                    if self.booked { 0 } else { input.residual }
                );
                assert_eq!(
                    (
                        close.support_consumed,
                        close.insurance_spent,
                        close.explicit_loss_assigned,
                        close.drift_consumed,
                        close.junior_face_burned
                    ),
                    (0, 0, 0, 0, 0)
                );
            } else {
                assert_eq!(close, CloseProgressLedgerV16::default());
            }
            accounts.push(account);
        }
        let source = group.source_credit[2 + (1 - side)];
        let remaining_face: u128 = (0..2)
            .filter(|i| self.converted[*i].is_none())
            .map(|i| {
                input.gains()[i]
                    - if self.settled[i] {
                        input.losses()[i]
                    } else {
                        0
                    }
            })
            .sum();
        assert_eq!(
            source.positive_claim_bound_num,
            remaining_face * BOUND_SCALE
        );
        assert_eq!(
            source.fresh_reserved_backing_num,
            (self.source_backing() - self.converted.iter().flatten().sum::<u128>()) * BOUND_SCALE
        );
        if group.payout_snapshot_captured && self.converted.iter().all(Option::is_some) {
            assert_eq!(
                group.resolved_payout_ledger.snapshot_residual,
                self.junior_pool()
            );
            assert_eq!(
                group.resolved_payout_ledger.current_payout_rate_num,
                self.junior_pool() * BOUND_SCALE
            );
            assert_eq!(
                group.resolved_payout_ledger.current_payout_rate_den,
                self.faces().iter().sum::<u128>() * BOUND_SCALE
            );
        }
        assert_eq!([asset.a_long, asset.a_short], [ADL_ONE; 2]);
        assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], [0; 2]);
        assert_eq!(
            [asset.loss_weight_sum_long, asset.loss_weight_sum_short],
            weight
        );
        assert_eq!(
            [asset.stored_pos_count_long, asset.stored_pos_count_short],
            pending
        );
        assert_eq!(
            [
                asset.pending_obligation_count_long,
                asset.pending_obligation_count_short
            ],
            pending
        );
        let vault = world.env.token_amount(world.env.vault) as u128;
        let wallets: u128 = world
            .actors
            .iter()
            .map(|a| world.env.token_amount(a.token) as u128)
            .sum();
        assert_eq!(vault + wallets, input.deposits().iter().sum::<u128>());
        assert_eq!(
            Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            vault + wallets
        );
        assert_eq!(group.insurance, 0);
        let raw = world.env.svm.get_account(&world.env.market).unwrap();
        assert_market_stock_census(
            "fractional residual stocks",
            &group,
            &raw.data,
            &accounts,
            vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census(
            "fractional residual reservations",
            &group,
            &accounts,
        )
        .unwrap();
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        let before = world.frame();
        let cu = match world.payout(actor, false) {
            Ok(cu) => cu,
            Err(error) => {
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(world.frame(), before);
                self.check(world);
                return;
            }
        };
        assert_cu_within("fractional residual resolved close", cu, CUSTODY_CU_LIMIT);
        *peak = (*peak).max(cu);
        if actor == 1 {
            self.booked = true;
        }
        if actor == 0 || actor == 2 {
            let account = world.env.portfolio_state(world.actors[actor].portfolio);
            let pair = actor / 2;
            self.detached[pair] = !has_active_leg_for_asset(&account, 1);
            if self.detached[pair] {
                assert!(self.booked);
                self.settled[pair] = true;
            } else {
                let snap = active_leg_for_asset(&account, 1).b_snap;
                assert!(snap == 0 || (self.booked && snap == self.input.target()));
                self.settled[pair] = snap != 0;
            }
            if self.converted[pair].is_none()
                && self.detached[pair]
                && account
                    .source_domains
                    .iter()
                    .all(|s| s.source_claim_bound_num.get() == 0)
            {
                self.record_conversion(pair);
            }
        }
        for (key, account) in before {
            if ![
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ]
            .contains(&key)
            {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "foreign Account {key}"
                );
            }
        }
        self.check(world);
    }

    fn delete(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        let before = world.frame();
        let a = &world.actors[actor];
        *peak = (*peak).max(world.env.close_portfolio_with_cu(&a.owner, a.portfolio));
        self.deleted[actor] = true;
        for (key, account) in before {
            if ![world.env.market, a.portfolio].contains(&key) {
                assert_eq!(world.env.svm.get_account(&key), account);
            }
        }
        self.check(world);
    }
}

fn setup(input: Inputs, reverse: bool, recovery_peer: bool, peak: &mut u64) -> AttributionWorld {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..V16CuMarketParams::default()
        },
        input.deposits(),
    );
    let sign = if reverse { -1 } else { 1 };
    world.quantities = [
        input.weights[0] as i128 * sign,
        -(input.weights[0] as i128) * sign,
        input.weights[1] as i128 * sign,
        -(input.weights[1] as i128) * sign,
    ];
    for pair in 0..2 {
        *peak = (*peak).max(world.env.trade_asset_with_cu(
            1,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            world.quantities[2 * pair],
            1_000_000,
            0,
        ));
    }
    let opening = world.env.market_state().1.assets[1];
    assert_eq!(
        [opening.oi_eff_long_q, opening.oi_eff_short_q],
        [input.weights.iter().sum::<u128>(); 2]
    );
    for slot in 1..=5 {
        world.env.svm.warp_to_slot(slot);
        world.env.push_auth_mark_for_asset_as_admin(
            1,
            slot,
            (1_000_000 + 40_000 * slot as i128 * sign) as u64,
        );
        *peak = (*peak).max(world.env.crank(
            world.actors[4].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        ));
    }
    let mark = (1_000_000 + Inputs::MOVEMENT as i128 * sign) as u64;
    assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
    for pair in 0..2 {
        if pair == 1 && recovery_peer {
            *peak = (*peak).max(world.env.crank(
                world.actors[2].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 5,
                    observations: crank_observations(1),
                },
            ));
            world.env.update_asset_lifecycle_as_admin_with_cu(
                processor::ASSET_ACTION_SHUTDOWN,
                1,
                5,
                0,
            );
            for actor in [2, 3] {
                let a = &world.actors[actor];
                *peak = (*peak).max(world.env.forfeit_recovery_leg_with_cu(
                    &a.owner,
                    a.portfolio,
                    1,
                    u128::MAX,
                ));
            }
            continue;
        }
        *peak = (*peak).max(world.env.trade_asset_with_cu(
            1,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            -world.quantities[2 * pair],
            mark,
            0,
        ));
    }
    world
}

#[test]
fn v16_program_fractional_cohort_residual_preserves_owner_floors_through_resolution_orders() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut receipt_retries = 0;
    let mut receipt_floor_worlds = 0;
    for (weights, residuals) in [
        ([450_003, 600_004], [7, 11]),
        ([300_003, 700_007], [10, 13]),
    ] {
        for residual in residuals {
            let input = Inputs { weights, residual };
            assert!(input.weights.iter().all(|q| *q % POS_SCALE != 0));
            assert!(input.booking_remainder() > 0);
            assert!(input.carries().iter().all(|r| *r > 0));
            assert!(input.losses().iter().all(|loss| *loss > 0));
            assert_eq!(
                input.carries().iter().sum::<u128>() + input.booking_remainder(),
                (residual - input.losses().iter().sum::<u128>()) * SOCIAL_LOSS_DEN
            );
            let mut baseline = None;
            let mut route_baselines = [None; 2];
            for reverse in [false, true] {
                for recovery_peer in [false, true] {
                    for live_booking in [false, true] {
                        for holders in [[0, 2], [2, 0]] {
                            let mut world = setup(input, reverse, recovery_peer, &mut peak);
                            let sibling = [
                                world.env.market_state().1.assets[0],
                                world.env.market_state().1.assets[2],
                            ];
                            let mut book = Book {
                                input,
                                recovery_peer,
                                recreated_capital: 0,
                                booked: false,
                                settled: [false; 2],
                                detached: [false; 2],
                                deleted: [false; 5],
                                converted: [None; 2],
                            };
                            book.check(&world);
                            if live_booking {
                                let before = world.frame();
                                peak = peak.max(world.env.crank(
                                    world.actors[1].portfolio,
                                    ProgInstruction::PermissionlessCrank {
                                        now_slot: 5,
                                        observations: crank_observations(1),
                                    },
                                ));
                                book.booked = true;
                                for (key, account) in before {
                                    if ![world.env.market, world.actors[1].portfolio].contains(&key)
                                    {
                                        assert_eq!(world.env.svm.get_account(&key), account);
                                    }
                                }
                                book.check(&world);
                            }
                            let before = world.frame();
                            peak = peak.max(world.env.resolve());
                            for (key, account) in before {
                                if key != world.env.market {
                                    assert_eq!(world.env.svm.get_account(&key), account);
                                }
                            }
                            book.check(&world);
                            world.env.svm.warp_to_slot(10);
                            book.close(&mut world, holders[0], &mut peak);
                            book.close(&mut world, 1, &mut peak);
                            book.delete(&mut world, 1, &mut peak);
                            for _ in 0..6 {
                                for actor in [holders[0], 3, holders[1], 4] {
                                    if book.deleted[actor] {
                                        continue;
                                    }
                                    if !resolved_portfolio_is_terminal(
                                        &world.env,
                                        world.actors[actor].portfolio,
                                    ) {
                                        book.close(&mut world, actor, &mut peak);
                                    }
                                    if resolved_portfolio_is_terminal(
                                        &world.env,
                                        world.actors[actor].portfolio,
                                    ) {
                                        if resolved_receipt(
                                            &world
                                                .env
                                                .portfolio_state(world.actors[actor].portfolio),
                                        )
                                        .present
                                        {
                                            let before = world.frame();
                                            peak = peak.max(
                                                world
                                                    .payout(actor, true)
                                                    .expect("paid receipt retry"),
                                            );
                                            assert_eq!(world.frame(), before);
                                            receipt_retries += 1;
                                        }
                                        book.delete(&mut world, actor, &mut peak);
                                    }
                                }
                            }
                            assert!(book.deleted.iter().all(|deleted| *deleted));
                            let paid: [u128; 5] = std::array::from_fn(|i| {
                                world.env.token_amount(world.actors[i].token) as u128
                            });
                            assert_eq!(paid, book.payouts());
                            let group = world.env.market_state().1;
                            let shortfall: u128 = input
                                .payouts()
                                .iter()
                                .zip(paid)
                                .map(|(full, paid)| full - paid)
                                .sum();
                            receipt_floor_worlds += usize::from(shortfall != 0);
                            assert_eq!(group.vault, input.cash_residue() + shortfall);
                            let source_rounding = book.source_backing()
                                - book.converted.iter().flatten().sum::<u128>();
                            let receipt_paid: u128 = (0..2)
                                .map(|i| {
                                    paid[2 * i]
                                        - input.deposits()[2 * i]
                                        - book.converted[i].unwrap()
                                })
                                .sum();
                            assert_eq!(
                                group.vault,
                                source_rounding + book.junior_pool() - receipt_paid
                            );
                            assert_eq!(
                                (
                                    group.c_tot,
                                    group.pnl_pos_tot,
                                    group.insurance,
                                    group.materialized_portfolio_count
                                ),
                                (0, 0, 0, 0)
                            );
                            assert_eq!([group.assets[0], group.assets[2]], sibling);
                            assert_eq!(
                                *route_baselines[usize::from(recovery_peer)]
                                    .get_or_insert((paid, group.vault)),
                                (paid, group.vault),
                                "raw owner payouts and stock must agree across orders within each route"
                            );
                            let normalized = (
                                std::array::from_fn::<_, 5, _>(|i| {
                                    paid[i] + (input.payouts()[i] - book.payouts()[i])
                                }),
                                group.vault - shortfall,
                            );
                            assert_eq!(*baseline.get_or_insert(normalized), normalized);
                            worlds += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 64);
    assert_eq!(receipt_floor_worlds, 16);
    assert!(receipt_retries > 0);
    println!("INV-039 fractional residual: {worlds} worlds, {receipt_floor_worlds} input-predicted receipt-floor worlds, {receipt_retries} receipt retries, peak {peak} CU");
}
