//! INV-024/037/039/066: one portfolio owns a pending creditor leg and an
//! unsettled debtor leg in different domains through public resolved settlement.
//! Integral inputs isolate role attribution from fractional cohort rounding.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
    verify_close_residual_partition,
};
use percolator::SOCIAL_LOSS_DEN;

const DEPOSITS: [u128; 5] = [400_000, 180_000, 300_000, 250_000, 777];
const GAIN: u128 = 200_000;
const RESIDUAL: u128 = GAIN - DEPOSITS[1];

struct Book {
    assets: [usize; 2],
    sign: i128,
    debt: u128,
    debt_k: i128,
    booked: bool,
    charged: bool,
    debt_settled: bool,
    converted: bool,
    deleted: [bool; 5],
}

impl Book {
    fn support(&self) -> u128 {
        self.debt.min(DEPOSITS[1])
    }

    fn support_face(&self) -> u128 {
        // K settlement precedes B: source support retires face at the input
        // backing/claim ratio. This is separate from the later B debit.
        assert_eq!(self.support() * GAIN % DEPOSITS[1], 0);
        self.support() * GAIN / DEPOSITS[1]
    }

    fn face_discount(&self) -> u128 {
        self.support_face() - self.support()
    }

    fn converted_face(&self) -> u128 {
        GAIN.saturating_sub(self.support_face() + RESIDUAL)
    }

    fn payouts(&self) -> [u128; 5] {
        [
            DEPOSITS[0] + GAIN - RESIDUAL - self.debt - self.face_discount(),
            0,
            DEPOSITS[2] + self.debt,
            DEPOSITS[3],
            DEPOSITS[4],
        ]
    }

    fn entitlement_matches(actual: [i128; 5], expected: [i128; 5]) -> bool {
        actual == expected
    }

    fn check(&mut self, world: &AttributionWorld) {
        let env = &world.env;
        let group = env.market_state().1;
        let target_b = RESIDUAL * SOCIAL_LOSS_DEN / POS_SCALE;
        assert_eq!(target_b * POS_SCALE, RESIDUAL * SOCIAL_LOSS_DEN);
        let creditor_side = usize::from(self.sign < 0);
        let mut accounts = Vec::new();
        let mut oi = [[0u128; 2]; 3];
        let mut weights = oi;
        let mut stored = [[0u64; 2]; 3];
        let mut pending = stored;
        let mut actual = [0i128; 5];
        let mut expected = self.payouts().map(|x| x as i128);

        let a = group.assets[self.assets[0]];
        let b = [a.b_long_num, a.b_short_num];
        assert!(b[creditor_side] == 0 || b[creditor_side] == target_b);
        assert_eq!(b[1 - creditor_side], 0);
        let booked = b[creditor_side] == target_b;
        assert!(!self.booked || booked, "booked residual is monotonic");
        self.booked = booked;

        for (actor, owner) in world.actors.iter().enumerate() {
            let paid = env.token_amount(owner.token) as u128;
            assert!(paid <= self.payouts()[actor], "owner {actor}: payout cap");
            if self.deleted[actor] {
                assert_eq!(paid, self.payouts()[actor]);
                assert!(env
                    .svm
                    .get_account(&owner.portfolio)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                actual[actor] = paid as i128;
                continue;
            }
            let account = env.portfolio_state(owner.portfolio);
            assert_eq!(account.owner, owner.owner.pubkey().to_bytes());
            let source_domain = self.assets[usize::from(actor == 2)] * 2 + 1 - creditor_side;
            for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
                assert!(actor == 0 || actor == 2);
                assert_eq!(
                    source.domain.get() as usize,
                    source_domain,
                    "owner-local source claim"
                );
            }
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
                .collect();
            for leg in &legs {
                let asset = leg.asset_index as usize;
                let pair = self.assets.iter().position(|x| *x == asset).unwrap();
                assert!(actor == 0 || (actor == 2 && pair == 1));
                let side = usize::from(leg.side == SideV16::Short);
                let q = if pair == 0 { POS_SCALE } else { 2 * POS_SCALE };
                assert_eq!(leg.loss_weight, q);
                assert_eq!(side, creditor_side ^ usize::from(actor == 0 && pair == 1));
                assert!(leg.basis_pos_q == 0 || leg.basis_pos_q.unsigned_abs() == q);
                if pair == 0 {
                    assert_eq!(leg.basis_pos_q, 0, "creditor retains only loss weight");
                    assert!(leg.b_snap == 0 || leg.b_snap == target_b);
                } else {
                    assert_eq!(leg.b_snap, 0, "creditor residual cannot migrate domains");
                }
                assert_eq!(leg.b_rem, 0);
                oi[asset][side] += leg.basis_pos_q.unsigned_abs();
                weights[asset][side] += q;
                stored[asset][side] += 1;
                pending[asset][side] += u64::from(leg.basis_pos_q == 0);
            }
            if actor == 0 {
                let creditor = legs
                    .iter()
                    .find(|l| l.asset_index as usize == self.assets[0]);
                let debtor = legs
                    .iter()
                    .find(|l| l.asset_index as usize == self.assets[1]);
                let charged = creditor.is_none_or(|l| l.b_snap == target_b);
                assert!(
                    !charged || booked,
                    "weight removal requires its domain booking"
                );
                assert!(!self.charged || charged);
                self.charged = charged;
                if let Some(leg) = debtor {
                    assert!(leg.k_snap == 0 || leg.k_snap == self.debt_k);
                }
                let settled = debtor.is_none_or(|l| l.k_snap == self.debt_k);
                assert!(!self.debt_settled || settled);
                self.debt_settled = settled;
                let domain = self.assets[0] * 2 + 1 - creditor_side;
                let has_source = account.source_domains.iter().any(|s| {
                    s.is_occupied()
                        && s.domain.get() as usize == domain
                        && s.source_claim_bound_num.get() != 0
                });
                if self.converted_face() > 0 {
                    let converted = !has_source;
                    assert!(!self.converted || converted);
                    assert!(!converted || (charged && settled));
                    self.converted = converted;
                }
                expected[0] = DEPOSITS[0] as i128 + GAIN as i128
                    - if charged { RESIDUAL as i128 } else { 0 }
                    - if settled {
                        (self.debt + self.face_discount()) as i128
                    } else {
                        0
                    };
                if !charged || !settled {
                    assert_eq!(paid, 0, "unsettled mixed roles cannot authorize payout");
                }
            }
            let close = close_progress(&account);
            verify_close_residual_partition("mixed-role resolution", &close).unwrap();
            if actor == 1 {
                assert!(close.active && !close.canceled);
                assert_eq!(close.asset_index as usize, self.assets[0]);
                assert_eq!(
                    close.domain_side,
                    if self.sign > 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(close.gross_loss_at_close_start, RESIDUAL);
                assert_eq!(
                    (
                        close.support_consumed,
                        close.junior_face_burned,
                        close.insurance_spent,
                        close.explicit_loss_assigned,
                        close.drift_consumed,
                        close.quantity_adl_applied_q
                    ),
                    (0, 0, 0, 0, 0, 0)
                );
                assert_eq!(close.finalized, booked);
                assert_eq!(close.b_loss_booked, if booked { RESIDUAL } else { 0 });
                assert_eq!(close.residual_remaining, if booked { 0 } else { RESIDUAL });
                expected[1] = if booked { 0 } else { -(RESIDUAL as i128) };
            } else {
                assert_eq!(close, CloseProgressLedgerV16::default());
            }
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert_eq!(actor, 2);
                assert!(self.booked && self.charged && self.debt_settled);
                assert_eq!(receipt.terminal_positive_claim_face, self.support());
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    receipt.terminal_positive_claim_face * BOUND_SCALE
                );
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            actual[actor] =
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128;
            accounts.push(account);
        }
        assert!(Self::entitlement_matches(actual, expected),
            "owner entitlement: actual={actual:?}, expected={expected:?}, booked={}, charged={}, debt_settled={}",
            self.booked, self.charged, self.debt_settled);
        let mut reassigned = actual;
        reassigned[0] -= 1;
        reassigned[2] += 1;
        assert_eq!(reassigned.iter().sum::<i128>(), actual.iter().sum::<i128>());
        assert!(
            !Self::entitlement_matches(reassigned, expected),
            "conserved wrong-owner observation"
        );

        let source = group.source_credit[self.assets[0] * 2 + 1 - creditor_side];
        let face = GAIN.saturating_sub(
            if self.debt_settled {
                self.support_face()
            } else {
                0
            } + if self.charged { RESIDUAL } else { 0 },
        );
        assert_eq!(
            source.positive_claim_bound_num,
            if self.converted {
                0
            } else {
                face * BOUND_SCALE
            }
        );
        let backing = DEPOSITS[1]
            - if self.debt_settled { self.support() } else { 0 }
            - if self.converted {
                self.converted_face()
            } else {
                0
            };
        assert_eq!(source.fresh_reserved_backing_num, backing * BOUND_SCALE);

        for asset in 0..3 {
            let a = group.assets[asset];
            assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
            assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
            assert_eq!(
                [a.loss_weight_sum_long, a.loss_weight_sum_short],
                weights[asset]
            );
            assert_eq!(
                [a.stored_pos_count_long, a.stored_pos_count_short],
                stored[asset]
            );
            assert_eq!(
                [
                    a.pending_obligation_count_long,
                    a.pending_obligation_count_short
                ],
                pending[asset]
            );
            assert_eq!(
                [
                    a.social_loss_remainder_long_num,
                    a.social_loss_remainder_short_num,
                    a.social_loss_dust_long_num,
                    a.social_loss_dust_short_num,
                    a.explicit_unallocated_loss_long,
                    a.explicit_unallocated_loss_short
                ],
                [0; 6]
            );
            if asset != self.assets[0] {
                assert_eq!([a.b_long_num, a.b_short_num], [0; 2]);
            }
        }
        let vault = env.token_amount(env.vault) as u128;
        assert_eq!(
            vault
                + world
                    .actors
                    .iter()
                    .map(|a| env.token_amount(a.token) as u128)
                    .sum::<u128>(),
            DEPOSITS.iter().sum::<u128>()
        );
        assert_eq!(
            Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            DEPOSITS.iter().sum::<u128>()
        );
        assert_eq!(group.insurance, 0);
        assert_market_stock_census(
            "mixed-role resolution",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &accounts,
            vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("mixed-role resolution", &group, &accounts).unwrap();
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) -> bool {
        let before = world.frame();
        let rejected = match world.payout(actor, false) {
            Ok(cu) => {
                assert_cu_within("mixed-role resolved continuation", cu, CUSTODY_CU_LIMIT);
                *peak = (*peak).max(cu);
                assert_ne!(world.frame(), before);
                false
            }
            Err(error) => {
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(world.frame(), before);
                true
            }
        };
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
                    "foreign account {key}"
                );
            }
        }
        self.check(world);
        rejected
    }
}

fn setup(reverse: bool, assets: [usize; 2], debt: u128) -> (AttributionWorld, Book) {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_abs_funding_e9_per_slot: 0,
            liquidation_fee_bps: 0,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..V16CuMarketParams::default()
        },
        DEPOSITS,
    );
    let sign = if reverse { -1i128 } else { 1 };
    for (pair, holder, debtor, lots) in [(0, 0, 1, 1), (1, 2, 0, 2)] {
        world.env.trade_asset_with_cu(
            assets[pair] as u16,
            &world.actors[holder].owner,
            world.actors[holder].portfolio,
            &world.actors[debtor].owner,
            world.actors[debtor].portfolio,
            lots * POS_SCALE as i128 * sign,
            1_000_000,
            0,
        );
    }
    for (pair, movement) in [GAIN, debt / 2].into_iter().enumerate() {
        for step in 1..=5 {
            let slot = (pair as u64) * 5 + step;
            world.env.svm.warp_to_slot(slot);
            world.env.push_auth_mark_for_asset_as_admin(
                assets[pair] as u16,
                slot,
                (1_000_000 + movement as i128 * step as i128 / 5 * sign) as u64,
            );
            world.env.crank(
                world.actors[4].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_for_assets(&[1, 2]),
                },
            );
        }
        if pair == 0 {
            world.env.trade_asset_with_cu(
                assets[0] as u16,
                &world.actors[0].owner,
                world.actors[0].portfolio,
                &world.actors[1].owner,
                world.actors[1].portfolio,
                -(POS_SCALE as i128) * sign,
                (1_000_000 + GAIN as i128 * sign) as u64,
                0,
            );
        } else {
            world.env.crank(
                world.actors[2].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 10,
                    observations: crank_observations(assets[1] as u16),
                },
            );
        }
    }
    for (pair, movement) in [GAIN, debt / 2].into_iter().enumerate() {
        let asset = world.env.market_state().1.assets[assets[pair]];
        let k = movement as i128 * sign * ADL_ONE as i128;
        assert_eq!([asset.k_long, asset.k_short], [k, -k]);
        assert_eq!(
            asset.effective_price,
            (1_000_000 + movement as i128 * sign) as u64
        );
    }
    let mut book = Book {
        assets,
        sign,
        debt,
        debt_k: -(debt as i128 / 2) * ADL_ONE as i128,
        booked: false,
        charged: false,
        debt_settled: false,
        converted: false,
        deleted: [false; 5],
    };
    assert_ne!(book.debt_k, 0);
    book.check(&world);
    assert!(!book.booked && !book.charged && !book.debt_settled);
    assert_eq!(
        world
            .env
            .portfolio_state(world.actors[0].portfolio)
            .legs
            .iter()
            .filter(|l| l.try_to_runtime().unwrap().active)
            .count(),
        2
    );
    (world, book)
}

#[test]
fn v16_program_mixed_creditor_debtor_roles_preserve_owner_entitlement_through_resolution() {
    let mut worlds = 0;
    let mut peak = 0;
    let mut waits = 0;
    let mut receipt_retries = 0;
    for debt in [36_000, 240_000] {
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                for live_booking in [false, true] {
                    for order in [[0, 2, 1, 3, 4], [1, 2, 0, 4, 3]] {
                        let (mut world, mut book) = setup(reverse, assets, debt);
                        let sibling = world.env.market_state().1.assets[0];
                        if live_booking {
                            world.env.crank(
                                world.actors[1].portfolio,
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: 10,
                                    observations: crank_observations(assets[0] as u16),
                                },
                            );
                            book.check(&world);
                            assert!(book.booked && !book.charged && !book.debt_settled);
                        }
                        let before = world.frame();
                        world.env.resolve();
                        assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                        for (key, account) in before {
                            if key != world.env.market {
                                assert_eq!(world.env.svm.get_account(&key), account);
                            }
                        }
                        book.check(&world);
                        world.env.svm.warp_to_slot(15);
                        for _ in 0..16 {
                            for actor in order {
                                if book.deleted[actor] {
                                    continue;
                                }
                                if !resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    waits += usize::from(book.close(&mut world, actor, &mut peak));
                                }
                                if resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    assert_eq!(
                                        world.env.token_amount(world.actors[actor].token) as u128,
                                        book.payouts()[actor]
                                    );
                                    if resolved_receipt(
                                        &world.env.portfolio_state(world.actors[actor].portfolio),
                                    )
                                    .present
                                    {
                                        let before = world.frame();
                                        peak = peak.max(
                                            world.payout(actor, true).expect("paid receipt retry"),
                                        );
                                        assert_eq!(world.frame(), before);
                                        receipt_retries += 1;
                                    }
                                    let before = world.frame();
                                    let a = &world.actors[actor];
                                    let cu =
                                        world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                                    assert_cu_within(
                                        "mixed-role portfolio deletion",
                                        cu,
                                        CUSTODY_CU_LIMIT,
                                    );
                                    for (key, account) in before {
                                        if ![world.env.market, a.owner.pubkey(), a.portfolio]
                                            .contains(&key)
                                        {
                                            assert_eq!(world.env.svm.get_account(&key), account);
                                        }
                                    }
                                    book.deleted[actor] = true;
                                    book.check(&world);
                                }
                            }
                            if book.deleted.iter().all(|x| *x) {
                                break;
                            }
                        }
                        assert!(book.deleted.iter().all(|x| *x));
                        assert!(book.booked && book.charged && book.debt_settled);
                        let group = world.env.market_state().1;
                        assert_eq!(
                            (
                                group.vault,
                                group.c_tot,
                                group.pnl_pos_tot,
                                group.materialized_portfolio_count
                            ),
                            (book.face_discount(), 0, 0, 0)
                        );
                        assert_eq!(group.assets[0], sibling);
                        let retained_backing = DEPOSITS[1] - book.support() - book.converted_face();
                        assert_eq!(
                            group
                                .source_credit
                                .iter()
                                .map(|s| s.fresh_reserved_backing_num)
                                .sum::<u128>(),
                            retained_backing * BOUND_SCALE
                        );
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert!(waits > 0);
    assert_eq!(receipt_retries, 32);
    println!("INV-039 mixed creditor/debtor: {worlds} worlds, {waits} exact waiting rejections, {receipt_retries} paid receipt retries; peak resolved CU={peak}");
}
