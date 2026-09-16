//! INV-039: source backing expires while its claimant still owns both pending
//! creditor weight and cross-asset debt. Expiry is not debt payment or B release.
//! The fresh boundary retains the existing support-discount book; after expiry,
//! uncovered debt consumes positive face at par and must reach the original peer.

use super::*;

const CREDITOR_EXPIRY: u64 = 1_005;

struct UnbackedBook {
    assets: [usize; 2],
    reverse: bool,
    debt: u128,
    booked: bool,
    charged: bool,
    settled: bool,
    deleted: [bool; 5],
}

impl UnbackedBook {
    fn payouts(&self) -> [u128; 5] {
        [
            DEPOSITS[0] + GAIN - RESIDUAL - self.debt,
            0,
            DEPOSITS[2] + self.debt,
            DEPOSITS[3],
            DEPOSITS[4],
        ]
    }

    fn check(&mut self, world: &AttributionWorld) {
        let env = &world.env;
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, 10);
        assert!(self.debt <= GAIN - RESIDUAL);
        let side = usize::from(self.reverse);
        let target_b = RESIDUAL * SOCIAL_LOSS_DEN / POS_SCALE;
        let a = group.assets[self.assets[0]];
        let b = [a.b_long_num, a.b_short_num];
        assert!(b[side] == 0 || b[side] == target_b);
        assert_eq!(b[1 - side], 0);
        let booked = b[side] == target_b;
        assert!(!self.booked || booked);
        self.booked = booked;
        let mut oi = [[0u128; 2]; 3];
        let mut weights = oi;
        let mut stored = [[0u64; 2]; 3];
        let mut pending = stored;
        let mut sources = [0u128; 6];
        let mut portfolios = Vec::new();
        let mut entitlements = [0i128; 5];
        let mut expected = self.payouts().map(|x| x as i128);

        for (actor, owner) in world.actors.iter().enumerate() {
            let paid = env.token_amount(owner.token) as u128;
            assert!(paid <= self.payouts()[actor]);
            if self.deleted[actor] {
                assert_eq!(paid, self.payouts()[actor]);
                assert!(env
                    .svm
                    .get_account(&owner.portfolio)
                    .is_none_or(|a| { a.lamports == 0 && a.data.is_empty() }));
                entitlements[actor] = paid as i128;
                continue;
            }
            let account = env.portfolio_state(owner.portfolio);
            assert_eq!(account.owner, owner.owner.pubkey().to_bytes());
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
                .collect();
            for leg in &legs {
                let asset = leg.asset_index as usize;
                let pair = self.assets.iter().position(|a| *a == asset).unwrap();
                assert!(actor == 0 || (actor == 2 && pair == 1));
                let leg_side = usize::from(leg.side == SideV16::Short);
                assert_eq!(leg_side, side ^ usize::from(actor == 0 && pair == 1));
                let q = (pair as u128 + 1) * POS_SCALE;
                assert_eq!(leg.loss_weight, q);
                assert!(leg.basis_pos_q == 0 || leg.basis_pos_q.unsigned_abs() == q);
                if pair == 0 {
                    assert_eq!(leg.basis_pos_q, 0);
                    assert!(leg.b_snap == 0 || leg.b_snap == target_b);
                } else {
                    assert_eq!(leg.b_snap, 0, "B debt must not move to the peer domain");
                }
                assert_eq!(leg.b_rem, 0);
                oi[asset][leg_side] += leg.basis_pos_q.unsigned_abs();
                weights[asset][leg_side] += q;
                stored[asset][leg_side] += 1;
                pending[asset][leg_side] += u64::from(leg.basis_pos_q == 0);
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
                    "pending weight requires residual booking"
                );
                assert!(!self.charged || charged);
                self.charged = charged;
                let debt_k = -(self.debt as i128 / 2) * ADL_ONE as i128;
                if let Some(leg) = debtor {
                    assert!(leg.k_snap == 0 || leg.k_snap == debt_k);
                }
                let settled = debtor.is_none_or(|l| l.k_snap == debt_k);
                assert!(!self.settled || settled);
                self.settled = settled;
                expected[0] = (DEPOSITS[0] + GAIN
                    - if charged { RESIDUAL } else { 0 }
                    - if settled { self.debt } else { 0 }) as i128;
                if !charged || !settled {
                    assert_eq!(paid, 0, "expiry does not authorize mixed-role payout");
                }
            }
            let close = close_progress(&account);
            verify_close_residual_partition("unsettled mixed-role expiry", &close).unwrap();
            if actor == 1 {
                assert!(close.active && !close.canceled);
                assert_eq!(close.asset_index as usize, self.assets[0]);
                assert_eq!(close.gross_loss_at_close_start, RESIDUAL);
                assert_eq!(close.finalized, booked);
                assert_eq!(close.b_loss_booked, if booked { RESIDUAL } else { 0 });
                assert_eq!(close.residual_remaining, if booked { 0 } else { RESIDUAL });
                assert_eq!(
                    close.support_consumed
                        + close.insurance_spent
                        + close.explicit_loss_assigned
                        + close.drift_consumed
                        + close.junior_face_burned
                        + close.quantity_adl_applied_q,
                    0
                );
                expected[1] = if booked { 0 } else { -(RESIDUAL as i128) };
            } else {
                assert_eq!(close, CloseProgressLedgerV16::default());
            }
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert!(actor == 0 || actor == 2);
                assert!(self.booked && self.charged && self.settled);
                let face = if actor == 0 {
                    GAIN - RESIDUAL - self.debt
                } else {
                    self.debt
                };
                assert!(face > 0);
                assert_eq!(receipt.terminal_positive_claim_face, face);
                assert_eq!(receipt.prior_bound_contribution_num, face * BOUND_SCALE);
                face.checked_sub(receipt.paid_effective).unwrap()
            } else {
                0
            };
            entitlements[actor] =
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128;
            for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
                assert!(actor == 0 || actor == 2);
                let domain = self.assets[usize::from(actor == 2)] * 2 + 1 - side;
                assert_eq!(source.domain.get() as usize, domain);
                let face = if actor == 0 {
                    GAIN - if self.charged { RESIDUAL } else { 0 }
                        - if self.settled { self.debt } else { 0 }
                } else {
                    self.debt
                };
                assert!(
                    source.source_claim_bound_num.get() == 0
                        || source.source_claim_bound_num.get() == face * BOUND_SCALE
                );
                sources[domain] += source.source_claim_bound_num.get();
            }
            portfolios.push(account);
        }
        assert_eq!(
            entitlements, expected,
            "original owner debt after backing expiry"
        );
        let mut wrong_owner = entitlements;
        wrong_owner[0] += 1;
        wrong_owner[2] -= 1;
        assert_eq!(
            wrong_owner.iter().sum::<i128>(),
            expected.iter().sum::<i128>()
        );
        assert!(!Book::entitlement_matches(wrong_owner, expected));
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
            if asset != self.assets[0] {
                assert_eq!([a.b_long_num, a.b_short_num], [0; 2]);
            }
        }
        for (domain, source) in group.source_credit.iter().enumerate() {
            assert_eq!(source.positive_claim_bound_num, sources[domain]);
            assert_eq!(source.fresh_reserved_backing_num, 0);
            assert_eq!(source.spent_backing_num, 0);
            assert_eq!(source.provider_receivable_num, 0);
        }
        assert_eq!(group.insurance, 0);
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
        assert_market_stock_census(
            "unsettled expiry",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &portfolios,
            vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("unsettled expiry", &group, &portfolios).unwrap();
    }
}

#[test]
fn v16_program_source_expiry_before_mixed_role_settlement_preserves_debt_and_weight() {
    let mut worlds = 0;
    let mut normalizations = 0;
    let mut rollbacks = 0;
    let mut waits = 0;
    let mut retries = 0;
    let mut peak = 0;
    for debt in [36_000, GAIN - RESIDUAL] {
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                for order in [[0, 2, 1, 3, 4], [2, 1, 0, 4, 3]] {
                    let mut exact_expiry_outcome = None;
                    for slot in [CREDITOR_EXPIRY - 1, CREDITOR_EXPIRY, CREDITOR_EXPIRY + 6] {
                        let expired = slot >= CREDITOR_EXPIRY;
                        let (mut world, mut fresh) = setup(reverse, assets, debt);
                        let mut unbacked = UnbackedBook {
                            assets,
                            reverse,
                            debt,
                            booked: false,
                            charged: false,
                            settled: false,
                            deleted: [false; 5],
                        };
                        let market = world.env.market;
                        let vault = world.env.vault;
                        let domain = assets[0] * 2 + usize::from(!reverse);
                        assert_eq!(
                            world.env.market_state().1.source_backing_buckets[domain].expiry_slot,
                            CREDITOR_EXPIRY
                        );
                        world.env.resolve();
                        fresh.check(&world);
                        assert!(!fresh.booked && !fresh.charged && !fresh.debt_settled);
                        let before = world.frame();
                        world.env.svm.warp_to_slot(slot);
                        assert_eq!(world.frame(), before);
                        fresh.check(&world);
                        let denied = deletion(&world, 3, false);
                        let normalize = payout(&world, 0, false);
                        if expired {
                            let mixed = world.env.portfolio_state(world.actors[0].portfolio);
                            let group = world.env.market_state().1;
                            peak = peak.max(land(
                                &mut world,
                                &[normalize.clone(), denied.clone()],
                                &[],
                                &[],
                                Some(1),
                            ));
                            rollbacks += 1;
                            fresh.check(&world);
                            let portfolio = world.actors[0].portfolio;
                            let cu =
                                land(&mut world, &[normalize], &[], &[market, portfolio], None);
                            assert_cu_within(
                                "mixed pending source normalization",
                                cu,
                                CUSTODY_CU_LIMIT,
                            );
                            peak = peak.max(cu);
                            normalizations += 1;
                            let after = world.env.portfolio_state(portfolio);
                            assert_eq!(
                                after.legs, mixed.legs,
                                "expiry cannot settle K/B or remove weight"
                            );
                            assert_eq!(after.source_domains, mixed.source_domains);
                            assert_eq!((after.capital, after.pnl), (mixed.capital, mixed.pnl));
                            assert_eq!(world.env.market_state().1.assets, group.assets);
                            let bucket = world.env.market_state().1.source_backing_buckets[domain];
                            assert_eq!(bucket.status, BackingBucketStatusV16::Expired);
                            assert_eq!(bucket.fresh_unliened_backing_num, 0);
                            unbacked.check(&world);
                            assert!(!unbacked.booked && !unbacked.charged && !unbacked.settled);
                        }
                        let expected = if expired {
                            unbacked.payouts()
                        } else {
                            fresh.payouts()
                        };
                        let mut deleted = [false; 5];
                        let mut staged = [false; 5];
                        for _ in 0..16 {
                            let round = world.frame();
                            for actor in order {
                                if deleted[actor] {
                                    continue;
                                }
                                if !resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    let instruction = payout(&world, actor, false);
                                    if !staged[actor] {
                                        peak = peak.max(land(
                                            &mut world,
                                            &[instruction.clone(), denied.clone()],
                                            &[],
                                            &[],
                                            Some(1),
                                        ));
                                        staged[actor] = true;
                                        rollbacks += 1;
                                    }
                                    let before = world.frame();
                                    match world.payout(actor, false) {
                                        Ok(cu) => {
                                            assert_cu_within(
                                                "mixed source expiry continuation",
                                                cu,
                                                CUSTODY_CU_LIMIT,
                                            );
                                            peak = peak.max(cu);
                                            assert_ne!(world.frame(), before);
                                        }
                                        Err(error) => {
                                            assert!(
                                                is_engine_non_progress_error(&error),
                                                "{error}"
                                            );
                                            assert_eq!(world.frame(), before);
                                            waits += 1;
                                        }
                                    }
                                    for (key, account) in before {
                                        if ![
                                            market,
                                            vault,
                                            world.actors[actor].portfolio,
                                            world.actors[actor].token,
                                        ]
                                        .contains(&key)
                                        {
                                            assert_eq!(world.env.svm.get_account(&key), account);
                                        }
                                    }
                                }
                                if expired {
                                    unbacked.check(&world);
                                } else {
                                    fresh.check(&world);
                                }
                                if resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    assert_eq!(
                                        world.env.token_amount(world.actors[actor].token) as u128,
                                        expected[actor]
                                    );
                                    if resolved_receipt(
                                        &world.env.portfolio_state(world.actors[actor].portfolio),
                                    )
                                    .present
                                    {
                                        let retry = payout(&world, actor, true);
                                        peak = peak.max(land(&mut world, &[retry], &[], &[], None));
                                        retries += 1;
                                    }
                                    let owner = world.actors[actor].owner.insecure_clone();
                                    let portfolio = world.actors[actor].portfolio;
                                    let rent =
                                        world.env.svm.get_account(&portfolio).unwrap().lamports;
                                    let market_rent =
                                        world.env.svm.get_account(&market).unwrap().lamports;
                                    let close = deletion(&world, actor, true);
                                    peak = peak.max(land(
                                        &mut world,
                                        &[close],
                                        &[&owner],
                                        &[market, portfolio],
                                        None,
                                    ));
                                    assert_eq!(
                                        world.env.svm.get_account(&market).unwrap().lamports,
                                        market_rent + rent
                                    );
                                    deleted[actor] = true;
                                    unbacked.deleted[actor] = true;
                                    fresh.deleted[actor] = true;
                                    if expired {
                                        unbacked.check(&world);
                                    } else {
                                        fresh.check(&world);
                                    }
                                }
                            }
                            if deleted.iter().all(|x| *x) {
                                break;
                            }
                            assert_ne!(
                                world.frame(),
                                round,
                                "a nonterminal round must make public progress"
                            );
                        }
                        assert!(deleted.iter().all(|x| *x), "bounded terminal progress");
                        let group = world.env.market_state().1;
                        assert_eq!(
                            (
                                group.c_tot,
                                group.pnl_pos_tot,
                                group.materialized_portfolio_count
                            ),
                            (0, 0, 0)
                        );
                        assert_eq!(
                            group.vault,
                            DEPOSITS.iter().sum::<u128>() - expected.iter().sum::<u128>()
                        );
                        if expired {
                            assert!(unbacked.booked && unbacked.charged && unbacked.settled);
                            assert_eq!(group.vault, 0);
                            if let Some(exact) = exact_expiry_outcome {
                                assert_eq!(expected, exact);
                            }
                            exact_expiry_outcome = Some(expected);
                        } else {
                            assert!(fresh.booked && fresh.charged && fresh.debt_settled);
                            assert_eq!(unbacked.payouts()[0] - expected[0], fresh.face_discount());
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert_eq!(normalizations, 32);
    assert_eq!(rollbacks, 272);
    assert!(waits > 0);
    assert_eq!(retries, 64);
    println!("INV-039 unsettled source expiry: {worlds} worlds, {normalizations} normalization-only steps, {rollbacks} prefix rollbacks, {waits} waiting rollbacks, {retries} receipt retries; peak CU={peak}");
}
