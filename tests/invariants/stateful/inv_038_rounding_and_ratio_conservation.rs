//! INV-038 - Rounding and ratio conservation.
//!
//! Normative obligation: Every rounded allocation plus explicit residue equals its exact source amount.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_composite_scale_matrix_preserves_exact_composition` holds the exact rational
//! composite price constant while changing its factorization at large and micro scales. It then
//! requires wrapper target, engine mark, liquidation eligibility, and extracted reward to agree
//! with exact single-round arithmetic.
//! `v16_program_selected_observation_omission_rejects_and_preserves_rounded_transfer` compares identical
//! public worlds with and without the selected asset observation after an unrelated epoch advance;
//! omission must reject with exact rollback, after which the observed continuation must preserve
//! funding indexes and terminal payouts exactly.
//! `v16_program_fractional_max_dt_cranks_reach_target_and_preserve_terminal_value` repeatedly executes the
//! bounded public crank at maximum elapsed time and requires fractional cap residue to accumulate
//! until the target is reached. Its public trace attempts both crank and stale-resolution routes,
//! terminalizes every actor, and binds any stalled-price short underpayment to the long's exact
//! terminal overpayment.
//! `v16_program_resolved_topups_preserve_exact_floor_remainders` creates a real underfunded terminal
//! receipt and raises its common payout rate twice through public backing expiry. An independent
//! shift/add oracle reconstructs each full-width quotient and remainder, then requires the public
//! payout and cumulative receipt payment to equal the quotient exactly. The second claim computes
//! from immutable face rather than from the prior floor, so retained fractional entitlement cannot
//! disappear across top-ups.
//! `v16_program_trade_driven_ewma_partitions_cannot_buy_unfunded_mark_movement` compares one
//! aggregate fill with a two-fill partition over every single/batch CPI/no-CPI route. Each mark
//! segment must be paid from its independently reconstructed externality notional; a same-slot
//! retry rolls back exactly, and bounded public catch-up makes the next authenticated slot usable.
//! `v16_program_zero_move_dust_prefix_cannot_consume_later_ewma_capacity` proves a one-quantum
//! prefix that produces no mark movement also consumes no movement fee, lock, or discovery slot;
//! the remainder reaches the same mark and custody state as the aggregate fill.
//! `v16_program_b_carry_survives_admitted_owner_weight_changes` adds one bounded public
//! booking/settlement/reduction history on the short loss side. A close-loss-origin ledger checks
//! scaled B allocations, retained liability
//! carry, actual owner PnL and unchanged capital at every B-phase transaction through zero OI.
//! It distinguishes a successful reduction that retains weight from a later nonzero denominator
//! change with nonzero market and leg carry. Before B, the fixed seed's public mark/quantity
//! inputs independently determine both cohort owners' K allocations and complementary residues;
//! each settlement and same-state rejection is observed. Initialization and nonzero funding
//! remain separately owned; this is not a mixed funding/receipt generator or a cash-residue
//! classification proof.
//! `v16_program_fresh_b_booking_after_weight_change_preserves_value_and_encumbrance_attribution`
//! then changes the eligible B denominator through an admitted owner reduction before creating a
//! second close. The public history independently checks the fresh quotient/remainder, inherited
//! carry, insurance spend, residual partition, stock, encumbrance, SPL custody, and unchanged value
//! attribution across booking and settlement.
//! `v16_program_generated_receipt_histories_preserve_deferred_rounding` compares eager and
//! deferred claims across generated backing releases, expiry spacing and mixed claim/close/crank
//! schedules. Every suffix transaction checks immutable-face entitlement, explicit floor residue,
//! per-owner SPL payout, custody and exact rejected frames. The common endpoint remains partial;
//! this is claim-cadence equivalence, not a permutation of authenticated expiry events.
//! A third schedule defers both backing normalizations until the second expiry, preserving the
//! same authenticated events and checking each bounded release before mixed-route payout.
//! INV-068's generated terminal-drain test reuses this history with eager or terminal-only
//! top-ups and both owner/continuation orders. Its independent checkpoint observer checks
//! terminal entitlements through all five public portfolio closes and stale-claim rollback.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: all listed matrices are fixed-pin public-route certifications. The EWMA
//! partition products are complete for the four deployed trade transports and the two boundary
//! partitions above; they do not close unrelated social-loss or backing-ratio products.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    },
    v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{ix::CrankObservationHint, state};

const TRADE_ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

#[derive(Clone)]
struct BHistorySnapshot {
    group: state::MarketGroupV16,
    accounts: Vec<state::PortfolioAccountV16>,
}

impl BHistorySnapshot {
    fn read(env: &V16Svm) -> Self {
        Self {
            group: env.primary_market_state().1,
            accounts: (0..env.actors.len())
                .map(|actor| env.primary_portfolio(actor))
                .collect(),
        }
    }

    fn leg(&self, actor: usize) -> Option<percolator::PortfolioLegV16> {
        self.accounts[actor]
            .legs
            .iter()
            .map(|leg| leg.try_to_runtime().unwrap())
            .find(|leg| leg.active && leg.asset_index == 0)
    }

    // Both carries are liability numerators, not collateral atoms or payout entitlements.
    fn side(&self, side: usize) -> (u128, u128, u128, u128, u128) {
        let asset = self.group.assets[0];
        if side == 0 {
            (
                asset.loss_weight_sum_long,
                asset.b_long_num,
                asset.social_loss_remainder_long_num,
                asset.social_loss_dust_long_num,
                asset.explicit_unallocated_loss_long,
            )
        } else {
            (
                asset.loss_weight_sum_short,
                asset.b_short_num,
                asset.social_loss_remainder_short_num,
                asset.social_loss_dust_short_num,
                asset.explicit_unallocated_loss_short,
            )
        }
    }
}

fn b_side(side: percolator::SideV16) -> usize {
    usize::from(side == percolator::SideV16::Short)
}

fn b_account_value(account: &state::PortfolioAccountV16) -> i128 {
    i128::try_from(account.capital.get())
        .unwrap()
        .checked_add(account.pnl.get())
        .unwrap()
}

/// One-asset observer reusable across public crank/reduction schedules after K/F settlement.
/// The origin is the close's outstanding loss, not custody minus observed senior stocks.
struct PublicBHistoryObserver {
    inherited_outstanding_num: [u128; 2],
    booked: [u128; 2],
    allocated: [u128; 2],
    capital: Vec<u128>,
    values: Vec<i128>,
    close_owner: usize,
    steps: usize,
    rejected: usize,
    bookings: usize,
    settlements: usize,
    health_checks: usize,
}

impl PublicBHistoryObserver {
    fn outstanding_num(snapshot: &BHistorySnapshot, side: usize) -> Result<u128, String> {
        let (_, index, remainder, dust, explicit) = snapshot.side(side);
        let explicit_num = explicit
            .checked_mul(percolator::SOCIAL_LOSS_DEN)
            .ok_or("B explicit-loss numerator overflow")?;
        let mut outstanding = remainder
            .checked_add(dust)
            .and_then(|value| value.checked_add(explicit_num))
            .ok_or("B market outstanding numerator overflow")?;
        for actor in 0..snapshot.accounts.len() {
            if let Some(leg) = snapshot.leg(actor).filter(|leg| b_side(leg.side) == side) {
                if leg.b_rem >= percolator::SOCIAL_LOSS_DEN {
                    return Err("B leg carry escaped its collateral-atom denominator".into());
                }
                let asset = snapshot.group.assets[0];
                let (epoch, prior_index) = if side == 0 {
                    (asset.epoch_long, asset.b_epoch_start_long_num)
                } else {
                    (asset.epoch_short, asset.b_epoch_start_short_num)
                };
                let target = if leg.b_epoch_snap == epoch {
                    index
                } else {
                    if leg.b_epoch_snap.checked_add(1) != Some(epoch) {
                        return Err(
                            "B observer only admits current or immediate reset epochs".into()
                        );
                    }
                    prior_index
                };
                let gap = target
                    .checked_sub(leg.b_snap)
                    .ok_or("B snapshot passed target")?;
                let leg_outstanding = leg
                    .loss_weight
                    .checked_mul(gap)
                    .and_then(|value| value.checked_add(leg.b_rem))
                    .ok_or("B leg outstanding numerator overflow")?;
                outstanding = outstanding
                    .checked_add(leg_outstanding)
                    .ok_or("B aggregate outstanding numerator overflow")?;
            }
        }
        Ok(outstanding)
    }

    fn new(env: &V16Svm, close_owner: usize) -> Self {
        let snapshot = BHistorySnapshot::read(env);
        let out = Self {
            inherited_outstanding_num: [
                Self::outstanding_num(&snapshot, 0).expect("long-side B checkpoint"),
                Self::outstanding_num(&snapshot, 1).expect("short-side B checkpoint"),
            ],
            booked: [0; 2],
            allocated: [0; 2],
            capital: snapshot
                .accounts
                .iter()
                .map(|account| account.capital.get())
                .collect(),
            values: snapshot.accounts.iter().map(b_account_value).collect(),
            close_owner,
            steps: 0,
            rejected: 0,
            bookings: 0,
            settlements: 0,
            health_checks: 0,
        };
        out.verify(&snapshot).expect("B checkpoint origin");
        out
    }

    fn verify(&self, snapshot: &BHistorySnapshot) -> Result<(), String> {
        for (actor, account) in snapshot.accounts.iter().enumerate() {
            if b_account_value(account) != self.values[actor]
                || account.capital.get() != self.capital[actor]
            {
                return Err(format!(
                    "B actor {actor} value: actual={}, expected={}",
                    b_account_value(account),
                    self.values[actor]
                ));
            }
        }
        for side in 0..2 {
            let (_, _, _, dust, _) = snapshot.side(side);
            if dust >= percolator::SOCIAL_LOSS_DEN {
                return Err("B dust escaped its collateral-atom denominator".into());
            }
            let outstanding = Self::outstanding_num(snapshot, side)?;
            let source_num = self.inherited_outstanding_num[side]
                .checked_add(
                    self.booked[side]
                        .checked_mul(percolator::SOCIAL_LOSS_DEN)
                        .ok_or("B booked numerator overflow")?,
                )
                .ok_or("B source numerator overflow")?;
            let attributed_num = self.allocated[side]
                .checked_mul(percolator::SOCIAL_LOSS_DEN)
                .and_then(|value| value.checked_add(outstanding))
                .ok_or("B attributed numerator overflow")?;
            if source_num != attributed_num {
                return Err(format!(
                    "B side {side}: inherited_num={}, booked={}, allocated={}, outstanding_num={outstanding}",
                    self.inherited_outstanding_num[side], self.booked[side], self.allocated[side]
                ));
            }
        }
        Ok(())
    }

    fn step(&mut self, env: &mut V16Svm, actor: usize, reduce_q: Option<u128>) -> bool {
        use crate::support::fuzz_model::assert_current_certificate_matches_snapshot_full_refresh;
        use crate::support::reference_math;

        let before = BHistorySnapshot::read(env);
        self.verify(&before).expect("B prefix before transaction");
        let tokens = env.all_token_account_data();
        let keys = std::iter::once(env.market)
            .chain(env.actors.iter().map(|actor| actor.portfolio))
            .chain([env.foreign_market, env.foreign_actor.portfolio])
            .collect::<Vec<_>>();
        let raw = keys
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        let result = if let Some(q) = reduce_q {
            env.rebalance_reduce(actor, 0, q).map(Some)
        } else {
            env.crank_if_actionable(
                actor,
                env.current_slot(),
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
        };
        self.steps += 1;
        let after = BHistorySnapshot::read(env);
        assert_eq!(
            env.all_token_account_data(),
            tokens,
            "B history cannot move SPL value"
        );
        let landed = result.unwrap_or_else(|error| {
            panic!(
                "B step {} actor {actor} reduce={reduce_q:?}: {error}",
                self.steps
            )
        });
        for (index, key) in keys.iter().enumerate() {
            if landed.is_none() || (*key != env.market && *key != env.actors[actor].portfolio) {
                assert_eq!(
                    env.svm.get_account(key),
                    raw[index],
                    "B rejected/unrelated account frame"
                );
            }
        }
        if let Some(tx) = landed {
            assert!(tx.compute_units < TX_CU_LIMIT);
        } else {
            self.rejected += 1;
            return false;
        }
        assert_eq!(after.group.vault, before.group.vault);
        assert_eq!(after.group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(after.group.insurance, before.group.insurance);
        assert_eq!(
            after.group.insurance_domain_budget,
            before.group.insurance_domain_budget
        );
        assert_eq!(
            after.group.backing_provider_earnings_total,
            before.group.backing_provider_earnings_total
        );
        assert_eq!(&after.group.assets[1..], &before.group.assets[1..]);

        let old_close = before.accounts[self.close_owner]
            .close_progress
            .try_to_runtime()
            .unwrap();
        let new_close = after.accounts[self.close_owner]
            .close_progress
            .try_to_runtime()
            .unwrap();
        let booked = old_close
            .residual_remaining
            .checked_sub(new_close.residual_remaining)
            .unwrap();
        assert_eq!(new_close.b_loss_booked - old_close.b_loss_booked, booked);
        assert_eq!(new_close.close_id, old_close.close_id);
        assert_eq!(new_close.domain_side, old_close.domain_side);
        assert_eq!(new_close.market_id, old_close.market_id);
        assert_eq!(
            new_close.gross_loss_at_close_start,
            old_close.gross_loss_at_close_start
        );
        assert_eq!(
            (
                new_close.support_consumed,
                new_close.junior_face_burned,
                new_close.insurance_spent,
                new_close.explicit_loss_assigned,
                new_close.drift_consumed
            ),
            (
                old_close.support_consumed,
                old_close.junior_face_burned,
                old_close.insurance_spent,
                old_close.explicit_loss_assigned,
                old_close.drift_consumed
            )
        );
        let loss_side = b_side(old_close.domain_side);
        if booked != 0 {
            assert_eq!(actor, self.close_owner);
            assert_eq!(
                booked,
                old_close
                    .residual_remaining
                    .min(before.group.config.public_b_chunk_atoms)
            );
            let (weight, index, carry, _, _) = before.side(loss_side);
            let (new_weight, new_index, new_carry, _, _) = after.side(loss_side);
            assert_eq!(weight, new_weight);
            let (q, r) = reference_math::mul_div_floor_with_remainder(
                booked,
                percolator::SOCIAL_LOSS_DEN,
                weight,
            )
            .unwrap();
            assert_eq!(new_index - index, q + (r + carry) / weight);
            assert_eq!(new_carry, (r + carry) % weight);
            self.booked[loss_side] += booked;
            self.values[self.close_owner] += i128::try_from(booked).unwrap();
            assert!(
                self.values[self.close_owner] <= 0,
                "booking may clear the close's loss, not create a claim"
            );
            self.bookings += 1;
        }

        for owner in 0..before.accounts.len() {
            let mut settled = 0;
            if let Some(leg) = before.leg(owner) {
                let next = after.leg(owner);
                let delta = next
                    .map_or(before.side(b_side(leg.side)).1, |leg| leg.b_snap)
                    .checked_sub(leg.b_snap)
                    .expect("monotonic B account snapshot");
                let (q, r) = reference_math::mul_div_floor_with_remainder(
                    leg.loss_weight,
                    delta,
                    percolator::SOCIAL_LOSS_DEN,
                )
                .unwrap();
                settled = q + (r + leg.b_rem) / percolator::SOCIAL_LOSS_DEN;
                if let Some(next) = next {
                    assert_eq!(next.b_rem, (r + leg.b_rem) % percolator::SOCIAL_LOSS_DEN);
                }
                self.allocated[b_side(leg.side)] += settled;
                self.settlements += usize::from(settled != 0);
            }
            self.values[owner] -= i128::try_from(settled).unwrap();
            assert!(
                after.accounts[owner].capital.get() <= before.accounts[owner].capital.get(),
                "B carry cannot credit senior capital"
            );
            self.health_checks += usize::from(
                assert_current_certificate_matches_snapshot_full_refresh(
                    "INV-038 B prefix",
                    &env.svm.get_account(&env.market).unwrap().data,
                    &env.primary_portfolio_data(owner),
                )
                .unwrap(),
            );
        }
        if reduce_q.is_some() {
            assert_eq!(
                after.group.source_credit, before.group.source_credit,
                "carry-only reduction cannot credit claims/backing"
            );
            assert_eq!(
                after.group.source_backing_buckets,
                before.group.source_backing_buckets
            );
        }
        self.verify(&after).unwrap_or_else(|error| {
            panic!(
                "B step {} actor {actor} reduce={reduce_q:?}: {error}",
                self.steps
            )
        });
        assert_public_stock_census("INV-038 B prefix", env).unwrap();
        assert_public_encumbrance_census("INV-038 B prefix", env).unwrap();
        true
    }

    fn settle(&mut self, env: &mut V16Svm, actors: &[usize]) {
        for &actor in actors {
            let mut complete = false;
            for _ in 0..16 {
                if !self.step(env, actor, None) {
                    complete = true;
                    break;
                }
            }
            assert!(
                complete,
                "B actor {actor} exceeded the 16-call continuation bound"
            );
        }
    }
}

fn verify_pre_b_mark_origin(
    snapshot: &BHistorySnapshot,
    cohort_q: u128,
    settled: [bool; 2],
) -> Result<[u128; 2], String> {
    const PRINCIPAL: u128 = 1_000_000;
    // public_b_close_seed(Short) submits twenty authenticated -500 bps marks,
    // with no cohort resize, ADL or funding before this checkpoint.
    let mark = (0..20).fold(INITIAL_PRICE, |price, _| price * 9_500 / 10_000);
    let price_delta = u128::from(INITIAL_PRICE - mark);
    let exact_num = i128::try_from(
        cohort_q
            .checked_mul(price_delta)
            .ok_or("K origin overflow")?,
    )
    .map_err(|_| "K origin exceeds signed range")?;
    let (gain, remainder) = crate::support::reference_math::mul_div_floor_with_remainder(
        cohort_q,
        price_delta,
        POS_SCALE,
    )?;
    if gain == 0 || remainder == 0 {
        return Err("K origin must exercise nonzero allocations and both rounding residues".into());
    }
    let residues = [remainder, POS_SCALE - remainder];
    let target_k = i128::try_from(price_delta * percolator::ADL_ONE).unwrap();
    let asset = snapshot.group.assets[0];
    if asset.effective_price != mark
        || (asset.a_long, asset.a_short) != (percolator::ADL_ONE, percolator::ADL_ONE)
        || (asset.k_long, asset.k_short) != (-target_k, target_k)
        || (asset.f_long_num, asset.f_short_num) != (0, 0)
    {
        return Err("K checkpoint differs from the public seed's mark/quantity origin".into());
    }
    for side in 0..2 {
        let (_, index, carry, dust, explicit) = snapshot.side(side);
        if (index, carry, dust, explicit) != (0, 0, 0, 0) {
            return Err("pre-B K settlement cannot book or allocate social loss".into());
        }
    }
    for (index, actor) in [2, 3].into_iter().enumerate() {
        let account = &snapshot.accounts[actor];
        let leg = snapshot.leg(actor).ok_or("K origin lost its cohort leg")?;
        let sign = if actor == 2 { 1 } else { -1 };
        let expected_capital = if actor == 3 && settled[index] {
            PRINCIPAL
                .checked_sub(gain + 1)
                .ok_or("K loss exceeds principal")?
        } else {
            PRINCIPAL
        };
        let allocated_num = (b_account_value(account) - PRINCIPAL as i128) * POS_SCALE as i128;
        let pending_num = if settled[index] { 0 } else { sign * exact_num };
        let residue = if settled[index] { residues[index] } else { 0 };
        if account.capital.get() != expected_capital
            || allocated_num + pending_num + residue as i128 != sign * exact_num
            || leg.basis_pos_q != -sign * cohort_q as i128
            || b_side(leg.side) != usize::from(actor == 2)
            || leg.a_basis != percolator::ADL_ONE
            || leg.loss_weight != cohort_q
            || (leg.b_snap, leg.b_rem) != (0, 0)
            || leg.k_snap != if settled[index] { sign * target_k } else { 0 }
            || leg.f_snap != 0
        {
            return Err(format!(
                "K owner {actor}: allocation/pending/residue or capital mismatch"
            ));
        }
    }
    Ok(residues)
}

#[test]
fn v16_program_b_carry_survives_admitted_owner_weight_changes() {
    use crate::support::fuzz_model::public_b_close_seed;
    use percolator::SideV16;

    let side = SideV16::Short;
    let chunk = 250_007;
    let order = [3, 2, 0];
    let cohort_q = POS_SCALE / 2 + 3;
    let mut env = public_b_close_seed(side, cohort_q, chunk).unwrap();
    env.begin_public_trace();
    let mut settled = [false; 2];
    let residues =
        verify_pre_b_mark_origin(&BHistorySnapshot::read(&env), cohort_q, settled).unwrap();
    assert_eq!(residues.iter().sum::<u128>(), POS_SCALE);
    let keys = std::iter::once(env.market)
        .chain(env.actors.iter().map(|actor| actor.portfolio))
        .chain([env.foreign_market, env.foreign_actor.portfolio])
        .collect::<Vec<_>>();
    for actor in [2, 3] {
        for expected_success in [true, false] {
            let raw = keys
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>();
            let tokens = env.all_token_account_data();
            let before = BHistorySnapshot::read(&env);
            let tx = env
                .crank_if_actionable(
                    actor,
                    env.current_slot(),
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .unwrap();
            assert_eq!(
                tx.is_some(),
                expected_success,
                "K settlement then exact NonProgress retry"
            );
            if let Some(tx) = tx {
                assert!(tx.compute_units < TX_CU_LIMIT);
                settled[actor - 2] = true;
            }
            let after = BHistorySnapshot::read(&env);
            assert_eq!(
                verify_pre_b_mark_origin(&after, cohort_q, settled).unwrap(),
                residues
            );
            assert_eq!(env.all_token_account_data(), tokens);
            assert_eq!(after.group.vault, before.group.vault);
            assert_eq!(after.group.vault, u128::from(env.token_amount(env.vault)));
            assert_eq!(after.group.insurance, before.group.insurance);
            assert_eq!(
                after.group.insurance_domain_budget,
                before.group.insurance_domain_budget
            );
            assert_eq!(
                after.group.backing_provider_earnings_total,
                before.group.backing_provider_earnings_total
            );
            assert_eq!(after.accounts[actor].owner, before.accounts[actor].owner);
            assert_eq!(
                after.accounts[actor].provenance_header,
                before.accounts[actor].provenance_header
            );
            for (index, key) in keys.iter().enumerate() {
                if !expected_success || (*key != env.market && *key != env.actors[actor].portfolio)
                {
                    assert_eq!(
                        env.svm.get_account(key),
                        raw[index],
                        "K rejected/unrelated frame"
                    );
                }
            }
            assert_public_stock_census("INV-038 pre-B K/F settlement", &env).unwrap();
        }
    }
    // Mutate observations only: a missing ceiling atom, senior reclassification,
    // and a conserved wrong-owner transfer must all fail the origin oracle.
    let checkpoint = BHistorySnapshot::read(&env);
    let mut wrong = checkpoint.clone();
    wrong.accounts[3].capital = percolator::V16PodU128::new(wrong.accounts[3].capital.get() + 1);
    assert!(verify_pre_b_mark_origin(&wrong, cohort_q, settled).is_err());
    let mut wrong = checkpoint.clone();
    wrong.accounts[2].capital = percolator::V16PodU128::new(wrong.accounts[2].capital.get() + 1);
    wrong.accounts[2].pnl = percolator::V16PodI128::new(wrong.accounts[2].pnl.get() - 1);
    assert!(verify_pre_b_mark_origin(&wrong, cohort_q, settled).is_err());
    let mut wrong = checkpoint;
    wrong.accounts[2].pnl = percolator::V16PodI128::new(wrong.accounts[2].pnl.get() + 1);
    wrong.accounts[3].pnl = percolator::V16PodI128::new(wrong.accounts[3].pnl.get() - 1);
    assert!(verify_pre_b_mark_origin(&wrong, cohort_q, settled).is_err());
    eprintln!(
        "INV-038 pre-B K: steps=4, rejected=2, residue_nums={residues:?}, denominator={POS_SCALE}"
    );
    let mut observer = PublicBHistoryObserver::new(&env, 1);
    assert!(observer.step(&mut env, 1, None));
    observer.settle(&mut env, &order);
    let before = BHistorySnapshot::read(&env);
    assert!(observer.step(&mut env, 2, Some(POS_SCALE / 11)));
    assert_eq!(
        before.leg(2).unwrap().basis_pos_q.unsigned_abs()
            - BHistorySnapshot::read(&env)
                .leg(2)
                .unwrap()
                .basis_pos_q
                .unsigned_abs(),
        POS_SCALE / 11
    );
    assert_eq!(
        BHistorySnapshot::read(&env).side(b_side(side)).0,
        before.side(b_side(side)).0,
        "active close retains loss weight despite position reduction"
    );
    for _ in 0..16 {
        if env
            .primary_portfolio(1)
            .close_progress
            .try_to_runtime()
            .unwrap()
            .finalized
        {
            break;
        }
        assert!(observer.step(&mut env, 1, None));
        observer.settle(&mut env, &order);
    }
    assert!(
        env.primary_portfolio(1)
            .close_progress
            .try_to_runtime()
            .unwrap()
            .finalized
    );
    let before = BHistorySnapshot::read(&env);
    assert!(before.side(b_side(side)).2 > 0);
    assert!(before.leg(2).unwrap().b_rem > 0);
    assert!(observer.step(&mut env, 2, Some(POS_SCALE / 7)));
    let after = BHistorySnapshot::read(&env);
    let remaining_weight = crate::support::reference_math::mul_div_ceil(
        after.leg(2).unwrap().basis_pos_q.unsigned_abs(),
        percolator::SOCIAL_WEIGHT_SCALE,
        after.leg(2).unwrap().a_basis,
    )
    .unwrap();
    assert_eq!(after.leg(2).unwrap().loss_weight, remaining_weight);
    assert_eq!(
        after.side(b_side(side)).0,
        before.side(b_side(side)).0 - before.leg(2).unwrap().loss_weight + remaining_weight
    );
    assert!(after.side(b_side(side)).0 < before.side(b_side(side)).0);
    assert!(after.side(b_side(side)).0 > 0);
    assert_eq!(after.side(b_side(side)).2, before.side(b_side(side)).2);
    assert_eq!(after.leg(2).unwrap().b_rem, before.leg(2).unwrap().b_rem);
    let remaining_q = after.leg(2).unwrap().basis_pos_q.unsigned_abs();
    assert!(observer.step(&mut env, 2, Some(remaining_q)));
    observer.settle(&mut env, &order);
    observer.settle(&mut env, &[1]);
    let terminal = BHistorySnapshot::read(&env);
    assert_eq!(terminal.group.assets[0].oi_eff_long_q, 0);
    assert_eq!(terminal.group.assets[0].oi_eff_short_q, 0);
    assert_eq!(terminal.side(0).0, 0);
    assert_eq!(terminal.side(1).0, 0);
    assert!((0..env.actors.len()).all(|actor| terminal.leg(actor).is_none()));
    assert_eq!(
        terminal.side(b_side(side)).2,
        after.side(b_side(side)).2,
        "the live-side booking numerator remains an explicit liability at zero OI"
    );
    // Corrupt observations only, never deployed accounts. Conservation alone must not
    // accept moving a claim atom into senior capital or transferring it to another owner.
    let mut wrong = terminal.clone();
    wrong.group.assets[0].social_loss_dust_long_num += 1;
    assert!(observer.verify(&wrong).is_err());
    let mut wrong = terminal.clone();
    wrong.accounts[2].capital = percolator::V16PodU128::new(wrong.accounts[2].capital.get() + 1);
    wrong.accounts[2].pnl = percolator::V16PodI128::new(wrong.accounts[2].pnl.get() - 1);
    assert!(observer.verify(&wrong).is_err());
    let mut wrong = terminal.clone();
    wrong.accounts[2].pnl = percolator::V16PodI128::new(wrong.accounts[2].pnl.get() + 1);
    wrong.accounts[0].pnl = percolator::V16PodI128::new(wrong.accounts[0].pnl.get() - 1);
    assert!(observer.verify(&wrong).is_err());
    eprintln!("INV-038 {side:?} chunk={chunk} order={order:?}: steps={}, rejected={}, bookings={}, settlements={}, health={}", observer.steps, observer.rejected, observer.bookings, observer.settlements, observer.health_checks);
    assert!(observer.bookings > 1 && observer.settlements > 0 && observer.health_checks > 0);
    assert!(observer.rejected > 0);
    let trace = env.finish_public_trace();
    trace.validate_public_execution().unwrap();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert_eq!(trace.steps.len(), observer.steps + 4);
    assert_eq!(
        trace.steps.iter().filter(|step| !step.succeeded).count(),
        observer.rejected + 2
    );
}

fn assert_inv038_attribution_censuses(label: &str, env: &V16Svm) {
    assert_public_stock_census(label, env).unwrap_or_else(|error| panic!("{label}: {error}"));
    assert_public_encumbrance_census(label, env).unwrap_or_else(|error| panic!("{label}: {error}"));
}

fn settle_inv038_actor(env: &mut V16Svm, actor: usize, label: &str) -> usize {
    for step in 0..16 {
        let landed = env
            .crank_if_actionable(
                actor,
                env.current_slot(),
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
            .unwrap_or_else(|error| panic!("{label} actor {actor} step {step}: {error}"));
        assert_inv038_attribution_censuses(&format!("{label} actor {actor} step {step}"), env);
        if landed.is_none() {
            return step;
        }
    }
    panic!("{label} actor {actor} exceeded the 16-call bound")
}

fn assert_inv038_close_partition(label: &str, close: &percolator::CloseProgressLedgerV16) {
    let left = close
        .gross_loss_at_close_start
        .checked_add(close.drift_consumed)
        .expect("close partition left side");
    let right = close
        .support_consumed
        .checked_add(close.insurance_spent)
        .and_then(|value| value.checked_add(close.b_loss_booked))
        .and_then(|value| value.checked_add(close.explicit_loss_assigned))
        .and_then(|value| value.checked_add(close.residual_remaining))
        .expect("close partition right side");
    assert_eq!(left, right, "{label}: {close:?}");
}

#[test]
fn v16_program_fresh_b_booking_after_weight_change_preserves_value_and_encumbrance_attribution() {
    use percolator::SideV16;

    const COHORT_OWNER: usize = 2;
    const SHARED_LOSER: usize = 3;
    const FIRST_WINNER: usize = 0;
    const FIRST_LOSER: usize = 1;
    const SECOND_WINNER: usize = 4;
    const LOSS_ASSET: u16 = 0;
    const INSURANCE_DOMAIN: usize = 1;
    const COHORT_Q: i128 = POS_SCALE as i128 / 2 + 3;
    const COHORT_REDUCTION_Q: u128 = POS_SCALE / 7;
    const CLOSE_Q: i128 = 3 * POS_SCALE as i128 / 4;
    const B_CHUNK: u128 = 100_003;
    const INSURANCE_ATOMS: u128 = 17;

    let mut env = V16Svm::new(
        [0x39; 32],
        MarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            actor_deposits: [1_000_000, 161_600, 1_000_000, 700_000, 1_000_000],
            actor_token_balances: [1_000_000, 161_600, 1_000_000, 700_000, 1_000_000],
            public_b_chunk_atoms: B_CHUNK,
            ..MarketConfig::default()
        },
    );
    let supply = env.token_supply_observed();
    env.begin_public_trace();
    assert_inv038_attribution_censuses("INV-038 rebooking initial", &env);

    execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        COHORT_OWNER,
        SHARED_LOSER,
        LOSS_ASSET,
        -COHORT_Q,
        INITIAL_PRICE,
        0,
    )
    .expect("independent B cohort opens publicly");
    let cohort_checkpoint = BHistorySnapshot::read(&env);
    let cohort_leg = cohort_checkpoint
        .leg(COHORT_OWNER)
        .expect("independent cohort leg");
    assert_eq!(cohort_leg.side, SideV16::Short);
    assert_eq!(cohort_leg.basis_pos_q, -COHORT_Q);
    assert_eq!(cohort_leg.loss_weight, COHORT_Q.unsigned_abs());
    assert_eq!(
        cohort_checkpoint.side(b_side(SideV16::Short)).0,
        COHORT_Q.unsigned_abs()
    );
    assert_inv038_attribution_censuses("INV-038 independent cohort admission", &env);

    for (winner, loser) in [(FIRST_WINNER, FIRST_LOSER), (SECOND_WINNER, SHARED_LOSER)] {
        execute_trade_route(
            &mut env,
            TradeRoute::NoCpi,
            winner,
            loser,
            LOSS_ASSET,
            -CLOSE_Q,
            INITIAL_PRICE,
            0,
        )
        .unwrap_or_else(|error| panic!("close pair {winner}/{loser} admission: {error}"));
        assert_inv038_attribution_censuses(
            &format!("INV-038 pre-admitted close pair {winner}/{loser}"),
            &env,
        );
    }

    let mut loss_mark = INITIAL_PRICE;
    for step in 0..20 {
        let slot = env.current_slot() + 1;
        loss_mark = loss_mark * 9_500 / 10_000;
        env.warp_to_slot(slot);
        env.push_auth_mark(LOSS_ASSET, slot, loss_mark)
            .unwrap_or_else(|error| panic!("loss mark {step}: {error}"));
        assert_inv038_attribution_censuses(&format!("INV-038 loss mark {step}"), &env);
        env.crank(
            FIRST_WINNER,
            slot,
            vec![CrankObservationHint {
                asset_index: LOSS_ASSET,
                oracle_accounts: 0,
            }],
        )
        .unwrap_or_else(|error| panic!("loss mark application {step}: {error}"));
        assert_inv038_attribution_censuses(&format!("INV-038 loss mark application {step}"), &env);
    }
    assert_eq!(
        env.primary_market_state().1.assets[LOSS_ASSET as usize].effective_price,
        loss_mark
    );
    execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        FIRST_WINNER,
        FIRST_LOSER,
        LOSS_ASSET,
        CLOSE_Q,
        loss_mark,
        0,
    )
    .expect("first pre-admitted winner closes publicly");
    let first_close = env
        .primary_portfolio(FIRST_LOSER)
        .close_progress
        .try_to_runtime()
        .unwrap();
    assert!(first_close.active && !first_close.finalized && first_close.residual_remaining > 0);
    assert_inv038_close_partition("INV-038 first close origin", &first_close);
    assert_inv038_attribution_censuses("INV-038 first close origin", &env);

    for actor in [COHORT_OWNER, SECOND_WINNER, FIRST_WINNER] {
        settle_inv038_actor(&mut env, actor, "INV-038 first-close K/B preparation");
    }
    let mut first_observer = PublicBHistoryObserver::new(&env, FIRST_LOSER);
    for step in 0..16 {
        if env
            .primary_portfolio(FIRST_LOSER)
            .close_progress
            .try_to_runtime()
            .unwrap()
            .finalized
        {
            break;
        }
        assert!(
            first_observer.step(&mut env, FIRST_LOSER, None),
            "first booking {step}"
        );
        first_observer.settle(&mut env, &[COHORT_OWNER, SECOND_WINNER, FIRST_WINNER]);
    }
    let first_final = env
        .primary_portfolio(FIRST_LOSER)
        .close_progress
        .try_to_runtime()
        .unwrap();
    assert!(first_final.finalized && first_final.b_loss_booked > 0);
    assert_inv038_close_partition("INV-038 first close final", &first_final);

    let before_reduction = BHistorySnapshot::read(&env);
    let before_leg = before_reduction
        .leg(COHORT_OWNER)
        .expect("independent cohort leg");
    assert!(first_observer.step(&mut env, COHORT_OWNER, Some(COHORT_REDUCTION_Q)));
    let after_reduction = BHistorySnapshot::read(&env);
    let after_leg = after_reduction
        .leg(COHORT_OWNER)
        .expect("reduced independent cohort leg");
    assert_eq!(
        before_leg.basis_pos_q.unsigned_abs() - after_leg.basis_pos_q.unsigned_abs(),
        COHORT_REDUCTION_Q
    );
    let expected_weight = crate::support::reference_math::mul_div_ceil(
        after_leg.basis_pos_q.unsigned_abs(),
        percolator::SOCIAL_WEIGHT_SCALE,
        after_leg.a_basis,
    )
    .unwrap();
    assert_eq!(after_leg.loss_weight, expected_weight);
    assert_eq!(
        after_reduction.side(b_side(SideV16::Short)).0,
        before_reduction.side(b_side(SideV16::Short)).0 - before_leg.loss_weight + expected_weight
    );

    let insurance_provider_before = env.token_amount(env.provider_source_token);
    let insurance_vault_before = env.token_amount(env.vault);
    let insurance_before = env.primary_market_state().1.insurance;
    env.top_up_insurance_domain(INSURANCE_DOMAIN as u16, INSURANCE_ATOMS)
        .expect("second close insurance is funded publicly");
    let (_, after_insurance_top_up) = env.primary_market_state();
    assert_eq!(
        env.token_amount(env.provider_source_token),
        insurance_provider_before - INSURANCE_ATOMS as u64
    );
    assert_eq!(
        env.token_amount(env.vault),
        insurance_vault_before + INSURANCE_ATOMS as u64
    );
    assert_eq!(
        after_insurance_top_up.insurance,
        insurance_before + INSURANCE_ATOMS
    );
    assert_eq!(
        after_insurance_top_up.insurance_domain_budget[INSURANCE_DOMAIN],
        INSURANCE_ATOMS
    );
    assert_inv038_attribution_censuses("INV-038 second-close insurance top-up", &env);

    execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        SECOND_WINNER,
        SHARED_LOSER,
        LOSS_ASSET,
        CLOSE_Q,
        loss_mark,
        0,
    )
    .expect("second pre-admitted winner reduces publicly after the lock");
    assert_inv038_attribution_censuses("INV-038 second winner exit", &env);
    let initial_second_close = env
        .primary_portfolio(SHARED_LOSER)
        .close_progress
        .try_to_runtime()
        .unwrap();
    assert!(initial_second_close.is_empty());
    let activation_tokens = env.all_token_account_data();
    let mut activation_before = None;
    let mut activation_steps = 0usize;
    for step in 0..8 {
        let before = BHistorySnapshot::read(&env);
        let landed = env
            .crank_if_actionable(
                SHARED_LOSER,
                env.current_slot(),
                vec![CrankObservationHint {
                    asset_index: LOSS_ASSET,
                    oracle_accounts: 0,
                }],
            )
            .unwrap_or_else(|error| panic!("second close setup {step}: {error}"));
        assert!(
            landed.is_some(),
            "second close setup stalled at step {step}"
        );
        activation_steps += 1;
        assert_inv038_attribution_censuses(&format!("INV-038 second close setup {step}"), &env);
        let close = env
            .primary_portfolio(SHARED_LOSER)
            .close_progress
            .try_to_runtime()
            .unwrap();
        if close.active {
            activation_before = Some(before);
            break;
        }
    }
    let before_fresh_booking = activation_before.expect("second close activates boundedly");
    let after_fresh_booking = BHistorySnapshot::read(&env);
    let second_origin = after_fresh_booking.accounts[SHARED_LOSER]
        .close_progress
        .try_to_runtime()
        .unwrap();
    assert!(
        second_origin.active && !second_origin.finalized && second_origin.residual_remaining > 0
    );
    assert_eq!(second_origin.domain_side, SideV16::Short);
    assert_eq!(
        (
            second_origin.support_consumed,
            second_origin.junior_face_burned,
            second_origin.insurance_spent,
            second_origin.explicit_loss_assigned,
            second_origin.drift_consumed,
        ),
        (0, 0, INSURANCE_ATOMS, 0, 0)
    );
    let freshly_booked = second_origin.b_loss_booked;
    assert_eq!(freshly_booked, B_CHUNK);
    assert_eq!(
        second_origin.gross_loss_at_close_start,
        second_origin.insurance_spent
            + second_origin.b_loss_booked
            + second_origin.residual_remaining
    );
    assert_inv038_close_partition("INV-038 second close origin", &second_origin);
    assert_eq!(after_fresh_booking.group.insurance, insurance_before);
    assert_eq!(
        after_fresh_booking.group.insurance_domain_spent[INSURANCE_DOMAIN],
        INSURANCE_ATOMS
    );
    assert_eq!(
        b_account_value(&after_fresh_booking.accounts[SHARED_LOSER]),
        b_account_value(&before_fresh_booking.accounts[SHARED_LOSER])
            + i128::try_from(INSURANCE_ATOMS + freshly_booked).unwrap()
    );
    assert_eq!(env.all_token_account_data(), activation_tokens);
    assert_inv038_attribution_censuses("INV-038 attributed fresh B booking", &env);

    let post_weight = before_fresh_booking.side(b_side(SideV16::Short)).0;
    assert_eq!(
        post_weight,
        before_fresh_booking.leg(COHORT_OWNER).unwrap().loss_weight
    );
    assert_eq!(post_weight, expected_weight);
    assert_eq!(
        after_fresh_booking.side(b_side(SideV16::Short)).0,
        post_weight
    );
    assert!(post_weight < before_reduction.side(b_side(SideV16::Short)).0);
    let (expected_index_delta, expected_remainder) =
        crate::support::reference_math::mul_div_floor_with_remainder(
            freshly_booked,
            percolator::SOCIAL_LOSS_DEN,
            post_weight,
        )
        .unwrap();
    let before_side = before_fresh_booking.side(b_side(SideV16::Short));
    let after_side = after_fresh_booking.side(b_side(SideV16::Short));
    assert_eq!(
        after_side.1 - before_side.1,
        expected_index_delta + (expected_remainder + before_side.2) / post_weight
    );
    assert_eq!(
        after_side.2,
        (expected_remainder + before_side.2) % post_weight
    );
    assert!(
        after_side.2 > 0,
        "fresh booking must retain a nonzero market carry"
    );

    let mut second_observer = PublicBHistoryObserver::new(&env, SHARED_LOSER);
    assert!(
        second_observer.step(&mut env, COHORT_OWNER, None),
        "cohort owner settles the fresh booking"
    );
    assert!(
        second_observer.step(&mut env, SHARED_LOSER, None),
        "second close books its remaining residual"
    );
    let second_final = env
        .primary_portfolio(SHARED_LOSER)
        .close_progress
        .try_to_runtime()
        .unwrap();
    assert!(second_final.finalized && second_final.b_loss_booked > 0);
    assert_eq!(second_final.insurance_spent, INSURANCE_ATOMS);
    assert_inv038_close_partition("INV-038 second close final", &second_final);
    assert_eq!(env.token_supply_observed(), supply);
    assert_inv038_attribution_censuses("INV-038 rebooking terminal", &env);

    let terminal = BHistorySnapshot::read(&env);
    let mut wrong = terminal.clone();
    wrong.group.assets[LOSS_ASSET as usize].social_loss_remainder_short_num += 1;
    assert!(second_observer.verify(&wrong).is_err());
    let mut wrong = terminal;
    wrong.accounts[COHORT_OWNER].pnl =
        percolator::V16PodI128::new(wrong.accounts[COHORT_OWNER].pnl.get() + 1);
    wrong.accounts[FIRST_WINNER].pnl =
        percolator::V16PodI128::new(wrong.accounts[FIRST_WINNER].pnl.get() - 1);
    assert!(second_observer.verify(&wrong).is_err());

    let trace = env.finish_public_trace();
    trace.validate_public_execution().unwrap();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert!(first_observer.bookings > 1);
    assert!(second_observer.bookings > 0 && second_observer.settlements > 0);
    eprintln!(
        "INV-038 attributed rebooking: public_steps={}, rejected={}, activation_steps={activation_steps}, first_bookings={}, second_bookings={}, second_settlements={}",
        trace.steps.len(),
        trace.steps.iter().filter(|step| !step.succeeded).count(),
        first_observer.bookings,
        second_observer.bookings,
        second_observer.settlements,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EwmaFeeSegment {
    old_mark: u64,
    new_mark: u64,
    trade_notional: u128,
    externality_notional: u128,
    collected_fee: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EwmaPartitionOutcome {
    mark: u64,
    mark_last_slot: u64,
    raw_target: u64,
    insurance_delta: u128,
    taker_position_q: i128,
    maker_position_q: i128,
    oi_long_q: u128,
    oi_short_q: u128,
    vault: u128,
    c_tot: u128,
    insurance: u128,
    public_steps: usize,
    rejected_same_slot_retries: usize,
    successful_catchup_cranks: usize,
    segments: Vec<EwmaFeeSegment>,
}

fn inv038_position_for_asset(account: &state::PortfolioAccountV16, asset_index: usize) -> i128 {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .unwrap_or(0)
}

fn inv038_notional_ceil(size_q: u128, price_e6: u64) -> Result<u128, String> {
    let denominator = POS_SCALE;
    let numerator = size_q
        .checked_mul(u128::from(price_e6))
        .ok_or("INV-038 independent notional numerator overflow")?;
    (numerator / denominator)
        .checked_add(u128::from(numerator % denominator != 0))
        .ok_or_else(|| "INV-038 independent notional ceil overflow".to_string())
}

fn run_trade_driven_ewma_partition(
    seed: [u8; 32],
    route: TradeRoute,
    parts_q: &[i128],
) -> Result<EwmaPartitionOutcome, String> {
    const VICTIM_Q: i128 = 10 * POS_SCALE as i128;
    const BID_SPREAD_BPS: u64 = 1_000;
    const TRADE_SLOT: u64 = 2;

    fn relative_bid(env: &V16Svm) -> Result<u64, String> {
        env.primary_market_state().1.assets[0]
            .effective_price
            .checked_mul(10_000 - BID_SPREAD_BPS)
            .and_then(|value| value.checked_div(10_000))
            .filter(|price| *price != 0)
            .ok_or_else(|| "INV-038 relative bid arithmetic failed".to_string())
    }

    if parts_q.is_empty() || parts_q.iter().any(|part| *part >= 0) {
        return Err("INV-038 EWMA partition needs nonempty negative fills".into());
    }
    let total_q = parts_q
        .iter()
        .try_fold(0i128, |total, part| total.checked_add(*part))
        .ok_or("INV-038 EWMA partition size overflow")?;
    if total_q.unsigned_abs() > VICTIM_Q as u128 {
        return Err("INV-038 EWMA partition must stay below existing OI".into());
    }

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            max_price_move_bps_per_slot: BID_SPREAD_BPS,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, INITIAL_PRICE, 1, 0)
        .map_err(|error| format!("INV-038 configure trade-driven EWMA: {error}"))?;
    env.trade_no_cpi(0, 1, 0, VICTIM_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("INV-038 open externality-bearing position: {error}"))?;
    env.set_matcher_spreads(3, BID_SPREAD_BPS, 0)
        .map_err(|error| format!("INV-038 configure equivalent CPI bid: {error}"))?;
    env.warp_to_slot(TRADE_SLOT);

    let before = env.primary_market_state().1;
    let before_supply = env.token_supply_observed();
    env.begin_public_trace();
    let mut rejected_same_slot_retries = 0usize;
    let mut successful_catchup_cranks = 0usize;
    let mut segments = Vec::with_capacity(parts_q.len());
    let mut previous_fill_moved_mark = false;
    for (index, part_q) in parts_q.iter().copied().enumerate() {
        if index != 0 && previous_fill_moved_mark {
            let same_slot_price = relative_bid(&env)?;
            let same_slot =
                execute_trade_route(&mut env, route, 2, 3, 0, part_q, same_slot_price, 0)
                    .expect_err(
                        "a pending trade-driven funding boundary must reject same-slot reuse",
                    );
            if !same_slot.contains("Custom(21)")
                && !same_slot.contains("custom program error: 0x15")
            {
                return Err(format!(
                    "INV-038 {route:?} same-slot retry returned an unexpected error: {same_slot}"
                ));
            }
            rejected_same_slot_retries += 1;

            let catchup_slot = env
                .current_slot()
                .checked_add(1)
                .ok_or("INV-038 catch-up slot overflow")?;
            env.warp_to_slot(catchup_slot);
            // The staged mark creates one funding boundary over every exposed account. Settle the
            // complete bounded cohort, not just the two portfolios requesting the retry; otherwise
            // the domain lock correctly remains active.
            for actor in [0usize, 1, 2, 3, 4] {
                for attempt in 0..8 {
                    let observations = if attempt == 0 {
                        vec![CrankObservationHint {
                            asset_index: 0,
                            oracle_accounts: 0,
                        }]
                    } else {
                        Vec::new()
                    };
                    match env.crank(actor, catchup_slot, observations) {
                        Ok(landed) => {
                            if landed.compute_units >= TX_CU_LIMIT {
                                return Err(format!(
                                    "INV-038 {route:?} actor {actor} catch-up used {} CU",
                                    landed.compute_units
                                ));
                            }
                            successful_catchup_cranks += 1;
                        }
                        Err(error)
                            if error.contains("Custom(22)")
                                || error.contains("custom program error: 0x16") =>
                        {
                            break;
                        }
                        Err(error) => {
                            return Err(format!(
                                "INV-038 {route:?} actor {actor} catch-up {attempt}: {error}"
                            ));
                        }
                    }
                }
            }
        }
        let reported_price = relative_bid(&env)?;
        let before_fill_group = env.primary_market_state().1;
        let before_fill_profile = env.primary_profile(0);
        let max_side_oi_q = before_fill_group.assets[0]
            .oi_eff_long_q
            .max(before_fill_group.assets[0].oi_eff_short_q);
        let externality_price = before_fill_group.assets[0]
            .effective_price
            .max(before_fill_profile.mark_ewma_e6);
        let max_side_notional = inv038_notional_ceil(max_side_oi_q, externality_price)?;
        let trade_notional = inv038_notional_ceil(part_q.unsigned_abs(), reported_price)?;
        if trade_notional > max_side_notional {
            return Err(format!(
                "INV-038 fixture lost its OI-dominated externality: trade={trade_notional}, side={max_side_notional}"
            ));
        }
        let externality_notional = max_side_notional
            .checked_mul(2)
            .ok_or("INV-038 two-sided externality overflow")?;
        let landed = execute_trade_route(&mut env, route, 2, 3, 0, part_q, reported_price, 0)
            .map_err(|error| format!("INV-038 {route:?} partition fill {index}: {error}"))?;
        if landed.compute_units >= TX_CU_LIMIT {
            return Err(format!(
                "INV-038 {route:?} partition fill {index} used {} CU",
                landed.compute_units
            ));
        }
        let after_fill_group = env.primary_market_state().1;
        let new_mark = env.primary_profile(0).mark_ewma_e6;
        let collected_fee = after_fill_group
            .insurance
            .checked_sub(before_fill_group.insurance)
            .ok_or("INV-038 fill reduced insurance")?;
        segments.push(EwmaFeeSegment {
            old_mark: before_fill_profile.mark_ewma_e6,
            new_mark,
            trade_notional,
            externality_notional,
            collected_fee,
        });
        previous_fill_moved_mark = new_mark != before_fill_profile.mark_ewma_e6;
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("INV-038 {route:?} public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0 {
        return Err(format!(
            "INV-038 {route:?} partition used out-of-band economic mutation: {trace:?}"
        ));
    }
    let rejected_steps = trace.steps.iter().filter(|step| !step.succeeded).count();
    if rejected_steps < rejected_same_slot_retries {
        return Err(format!(
            "INV-038 {route:?} trace lost a same-slot rollback: trace={rejected_steps}, expected={rejected_same_slot_retries}"
        ));
    }

    let (cfg, group) = env.primary_market_state();
    let profile = env.primary_profile(0);
    let taker_position_q = inv038_position_for_asset(&env.primary_portfolio(2), 0);
    let maker_position_q = inv038_position_for_asset(&env.primary_portfolio(3), 0);
    if taker_position_q != total_q || maker_position_q != -total_q {
        return Err(format!(
            "INV-038 {route:?} position mismatch: {taker_position_q}/{maker_position_q}, expected {total_q}/{}",
            -total_q
        ));
    }
    if u128::from(env.token_amount(env.vault)) != group.vault
        || env.token_supply_observed() != before_supply
    {
        return Err(format!(
            "INV-038 {route:?} partition broke custody: vault={}, c_tot={}, insurance={}, SPL={}",
            group.vault,
            group.c_tot,
            group.insurance,
            env.token_amount(env.vault)
        ));
    }
    assert_public_stock_census(&format!("INV-038 {route:?} EWMA partition"), &env)?;
    if cfg.mark_ewma_e6 != profile.mark_ewma_e6
        || cfg.mark_ewma_last_slot != profile.mark_ewma_last_slot
        || profile.oracle_target_price_e6 != profile.mark_ewma_e6
        || group.assets[0].raw_oracle_target_price != profile.mark_ewma_e6
    {
        return Err(format!(
            "INV-038 {route:?} did not atomically stage one coherent EWMA target"
        ));
    }

    Ok(EwmaPartitionOutcome {
        mark: profile.mark_ewma_e6,
        mark_last_slot: profile.mark_ewma_last_slot,
        raw_target: group.assets[0].raw_oracle_target_price,
        insurance_delta: group
            .insurance
            .checked_sub(before.insurance)
            .ok_or("INV-038 trade-driven fee reduced insurance")?,
        taker_position_q,
        maker_position_q,
        oi_long_q: group.assets[0].oi_eff_long_q,
        oi_short_q: group.assets[0].oi_eff_short_q,
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
        public_steps: trace.steps.len(),
        rejected_same_slot_retries,
        successful_catchup_cranks,
        segments,
    })
}

fn inv038_price_move_bps_ceil(old: u64, new: u64) -> u64 {
    assert!(old != 0);
    let numerator = u128::from(old.abs_diff(new))
        .checked_mul(10_000)
        .expect("bounded INV-038 movement numerator");
    let quotient = numerator / u128::from(old);
    let remainder = numerator % u128::from(old);
    u64::try_from(quotient + u128::from(remainder != 0)).expect("bounded INV-038 movement bps")
}

fn assert_ewma_partition_segments(route: TradeRoute, label: &str, outcome: &EwmaPartitionOutcome) {
    assert!(!outcome.segments.is_empty());
    assert_eq!(outcome.segments.last().unwrap().new_mark, outcome.mark);
    assert_eq!(
        outcome
            .segments
            .iter()
            .map(|segment| segment.collected_fee)
            .sum::<u128>(),
        outcome.insurance_delta,
        "{route:?} {label}: per-fill fees must reconstruct the aggregate insurance delta"
    );
    for (index, segment) in outcome.segments.iter().enumerate() {
        if index != 0 {
            assert_eq!(
                segment.old_mark,
                outcome.segments[index - 1].new_mark,
                "{route:?} {label}: mark segments must compose without a hidden jump"
            );
        }
        assert!(segment.trade_notional > 0);
        assert!(
            segment.externality_notional
                >= segment
                    .trade_notional
                    .checked_mul(2)
                    .expect("bounded INV-038 two-sided trade notional")
        );
        let move_bps = inv038_price_move_bps_ceil(segment.old_mark, segment.new_mark);
        let exact_numerator = segment
            .externality_notional
            .checked_mul(move_bps as u128)
            .expect("bounded per-fill movement-fee numerator");
        let quotient = exact_numerator / 10_000;
        let remainder = exact_numerator % 10_000;
        let required_fee = quotient + u128::from(remainder != 0);
        assert!(
            segment.collected_fee >= required_fee,
            "{route:?} {label} segment {index} paid {} for {} bps over externality {}, below required {}",
            segment.collected_fee,
            move_bps,
            segment.externality_notional,
            required_fee
        );
        if move_bps != 0 {
            assert!(segment.collected_fee != 0);
        }
        assert!(
            required_fee
                .checked_mul(10_000)
                .and_then(|rounded| rounded.checked_sub(exact_numerator))
                .is_some_and(|rounding| rounding < 10_000),
            "{route:?} {label} segment {index}: ceil residue escaped its denominator"
        );
    }
}

#[test]
fn v16_program_trade_driven_ewma_partitions_cannot_buy_unfunded_mark_movement() {
    const TOTAL_Q: i128 = POS_SCALE as i128;
    const VICTIM_Q: u128 = 10 * POS_SCALE;

    let mut canonical = None;
    for (route_index, route) in TRADE_ROUTES.into_iter().enumerate() {
        let seed = [0x38u8.wrapping_add(route_index as u8); 32];
        let aggregate = run_trade_driven_ewma_partition(seed, route, &[-TOTAL_Q])
            .unwrap_or_else(|error| panic!("aggregate {route:?}: {error}"));
        let split = run_trade_driven_ewma_partition(
            seed,
            route,
            &[-(TOTAL_Q / 4), -(TOTAL_Q - TOTAL_Q / 4)],
        )
        .unwrap_or_else(|error| panic!("split {route:?}: {error}"));

        assert_ewma_partition_segments(route, "aggregate", &aggregate);
        assert_ewma_partition_segments(route, "split", &split);
        let aggregate_move_bps = inv038_price_move_bps_ceil(INITIAL_PRICE, aggregate.mark);
        let split_move_bps = inv038_price_move_bps_ceil(INITIAL_PRICE, split.mark);
        assert!(aggregate_move_bps > 0 && split_move_bps > 0);
        assert!(
            split_move_bps > aggregate_move_bps,
            "{route:?}: a second authenticated slot must make additional paid discovery progress"
        );
        assert!(split.insurance_delta > aggregate.insurance_delta);

        for outcome in [&aggregate, &split] {
            assert_eq!(outcome.raw_target, outcome.mark);
            assert_eq!(outcome.taker_position_q, -TOTAL_Q);
            assert_eq!(outcome.maker_position_q, TOTAL_Q);
            assert_eq!(outcome.oi_long_q, outcome.oi_short_q);
            assert_eq!(outcome.oi_long_q, VICTIM_Q + TOTAL_Q as u128);
            assert!(outcome.c_tot <= outcome.vault);
            assert!(outcome.insurance <= outcome.vault);
            assert!(outcome.public_steps >= 1);
        }
        assert_eq!(aggregate.mark_last_slot, 2);
        assert_eq!(split.mark_last_slot, 3);
        assert_eq!(aggregate.rejected_same_slot_retries, 0);
        assert_eq!(aggregate.successful_catchup_cranks, 0);
        assert_eq!(split.rejected_same_slot_retries, 1);
        assert!(split.successful_catchup_cranks > 0);

        let route_frame = (
            aggregate.mark,
            aggregate.insurance_delta,
            aggregate.segments.clone(),
            split.mark,
            split.insurance_delta,
            split.segments.clone(),
            aggregate.oi_long_q,
            aggregate.vault,
        );
        if let Some(expected) = &canonical {
            assert_eq!(
                &route_frame, expected,
                "{route:?}: route transport changed EWMA partition economics"
            );
        } else {
            canonical = Some(route_frame);
        }
    }
}

#[test]
fn v16_program_zero_move_dust_prefix_cannot_consume_later_ewma_capacity() {
    const TOTAL_Q: i128 = POS_SCALE as i128;
    let mut canonical = None;
    for route in TRADE_ROUTES {
        let aggregate = run_trade_driven_ewma_partition([0x39; 32], route, &[-TOTAL_Q])
            .unwrap_or_else(|error| panic!("aggregate {route:?}: {error}"));
        let dust_prefixed =
            run_trade_driven_ewma_partition([0x39; 32], route, &[-1, -(TOTAL_Q - 1)])
                .unwrap_or_else(|error| panic!("dust-prefixed {route:?}: {error}"));
        assert_ewma_partition_segments(route, "dust aggregate", &aggregate);
        assert_ewma_partition_segments(route, "dust-prefixed split", &dust_prefixed);

        assert_eq!(dust_prefixed.segments.len(), 2);
        let dust = dust_prefixed.segments[0];
        assert_eq!(dust.trade_notional, 1);
        assert_eq!(dust.old_mark, dust.new_mark);
        assert_eq!(dust.collected_fee, 0);
        assert_ne!(
            dust_prefixed.segments[1].old_mark,
            dust_prefixed.segments[1].new_mark
        );
        assert_eq!(dust_prefixed.mark, aggregate.mark);
        assert_eq!(dust_prefixed.mark_last_slot, 2);
        assert_eq!(dust_prefixed.rejected_same_slot_retries, 0);
        assert_eq!(dust_prefixed.successful_catchup_cranks, 0);

        let route_frame = (
            aggregate.mark,
            aggregate.insurance_delta,
            dust_prefixed.mark,
            dust_prefixed.insurance_delta,
            dust_prefixed.segments.clone(),
            dust_prefixed.oi_long_q,
            dust_prefixed.vault,
        );
        if let Some(expected) = &canonical {
            assert_eq!(
                &route_frame, expected,
                "{route:?}: route transport changed dust-prefix EWMA economics"
            );
        } else {
            canonical = Some(route_frame);
        }
    }
}

#[test]
fn v16_program_resolved_topups_preserve_exact_floor_remainders() {
    let evidence = verify_resolved_receipt_split_topups()
        .expect("public resolved top-ups must preserve exact floor partitions");

    assert!(evidence.first_payout > 0 && evidence.second_payout > 0);
    assert!(evidence.first_floor_remainder_den > 0);
    assert!(evidence.second_floor_remainder_den > 0);
    assert!(evidence.first_floor_remainder_num < evidence.first_floor_remainder_den);
    assert!(evidence.second_floor_remainder_num < evidence.second_floor_remainder_den);
    assert!(
        evidence.first_floor_remainder_num > 0 || evidence.second_floor_remainder_num > 0,
        "the public fixture must exercise a nonzero payout-floor remainder: {evidence:?}"
    );
    assert_eq!(
        evidence.first_paid - evidence.initial_paid,
        evidence.first_payout
    );
    assert_eq!(
        evidence.second_paid - evidence.first_paid,
        evidence.second_payout
    );
    assert!(evidence.second_paid < evidence.receipt_face);
    assert_eq!(evidence.final_engine_vault, evidence.final_spl_vault);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReceiptHistoryFrame {
    markets: [Vec<u8>; 2],
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matchers: Vec<Vec<u8>>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
    accounts: Vec<(
        solana_sdk::pubkey::Pubkey,
        Option<solana_sdk::account::Account>,
    )>,
}

impl ReceiptHistoryFrame {
    fn read(env: &V16Svm) -> Self {
        Self {
            markets: [env.market_data(false), env.market_data(true)],
            portfolios: env
                .actors
                .iter()
                .map(|actor| {
                    env.svm
                        .get_account(&actor.portfolio)
                        .unwrap_or_default()
                        .data
                })
                .collect(),
            foreign_portfolio: env.foreign_portfolio_data(),
            backing_ledger: env.backing_domain_ledger_data(),
            tokens: env.all_token_account_data(),
            matchers: env.all_matcher_context_data(),
            lamports: env.all_economic_account_lamports(),
            accounts: env
                .all_economic_account_lamports()
                .into_iter()
                .map(|(key, _)| (key, env.svm.get_account(&key)))
                .collect(),
        }
    }

    fn assert_unrelated(&self, after: &Self, env: &V16Svm, actor: usize) {
        assert_eq!(self.accounts.len(), after.accounts.len());
        for ((key, account), (after_key, after_account)) in
            self.accounts.iter().zip(&after.accounts)
        {
            assert_eq!(key, after_key);
            if ![
                env.market,
                env.actors[actor].portfolio,
                env.actors[actor].destination_token,
                env.vault,
            ]
            .contains(key)
            {
                assert_eq!(account, after_account, "unrelated receipt account {key}");
            }
        }
    }
}

struct ReceiptHistoryOracle {
    receipt: percolator::ResolvedPayoutReceiptV16,
    residual: u128,
    claim_bound: u128,
    vault: u128,
    capitals: Vec<u128>,
    destinations: Vec<u64>,
    paid: u128,
    released: u128,
    steps: usize,
    rejections: usize,
    payments: usize,
    nonzero_remainders: usize,
}

#[derive(Clone, Copy, Debug)]
enum ReceiptPayoutRoute {
    Claim,
    Close,
    Crank,
}

#[derive(Clone, Copy, Debug)]
enum ReceiptHistoryAction {
    Payout(ReceiptPayoutRoute),
    Release { domain: usize, amount: u128 },
    TryDelete,
}

impl ReceiptHistoryOracle {
    fn new(env: &V16Svm) -> Self {
        let group = env.primary_market_state().1;
        let receipt = env
            .primary_portfolio(0)
            .resolved_payout_receipt
            .try_to_runtime()
            .unwrap();
        assert!(receipt.present && !receipt.finalized);
        assert!(receipt.paid_effective < receipt.terminal_positive_claim_face);
        let ledger = group.resolved_payout_ledger;
        Self {
            receipt,
            residual: ledger.snapshot_residual,
            claim_bound: ledger.terminal_claim_exact_receipts_num
                + ledger.terminal_claim_bound_unreceipted_num,
            vault: group.vault,
            capitals: (0..env.actors.len())
                .map(|actor| env.primary_portfolio(actor).capital.get())
                .collect(),
            destinations: env
                .actors
                .iter()
                .map(|actor| env.token_amount(actor.destination_token))
                .collect(),
            paid: receipt.paid_effective,
            released: 0,
            steps: 0,
            rejections: 0,
            payments: 0,
            nonzero_remainders: 0,
        }
    }

    fn entitlement(&self) -> (u128, u128) {
        crate::support::reference_math::mul_div_floor_with_remainder(
            self.receipt.terminal_positive_claim_face,
            (self.residual + self.released) * percolator::BOUND_SCALE,
            self.claim_bound,
        )
        .unwrap()
    }

    fn verify_receipt(
        &self,
        mut receipt: percolator::ResolvedPayoutReceiptV16,
        destinations: &[u64],
    ) -> bool {
        if receipt.paid_effective != self.paid {
            return false;
        }
        receipt.paid_effective = self.receipt.paid_effective;
        receipt == self.receipt
            && destinations.len() == self.destinations.len()
            && destinations.iter().enumerate().all(|(actor, value)| {
                u128::from(*value)
                    == u128::from(self.destinations[actor])
                        + if actor == 0 {
                            self.paid - self.receipt.paid_effective
                        } else {
                            0
                        }
            })
    }

    fn verify(&self, env: &V16Svm) {
        let group = env.primary_market_state().1;
        let ledger = group.resolved_payout_ledger;
        let expected_residual = self.residual + self.released;
        assert!(expected_residual * percolator::BOUND_SCALE < self.claim_bound);
        assert_eq!(ledger.snapshot_residual, expected_residual);
        assert_eq!(group.payout_snapshot, expected_residual);
        assert_eq!(
            ledger.current_payout_rate_num,
            expected_residual * percolator::BOUND_SCALE
        );
        assert_eq!(ledger.current_payout_rate_den, self.claim_bound);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num + ledger.terminal_claim_bound_unreceipted_num,
            self.claim_bound,
        );
        let receipt = env
            .primary_portfolio(0)
            .resolved_payout_receipt
            .try_to_runtime()
            .unwrap();
        let destinations = env
            .actors
            .iter()
            .map(|actor| env.token_amount(actor.destination_token))
            .collect::<Vec<_>>();
        assert!(self.verify_receipt(receipt, &destinations));
        let (entitlement, remainder) = self.entitlement();
        assert!(self.paid <= entitlement && remainder < self.claim_bound);
        assert_eq!(
            receipt.terminal_positive_claim_face * expected_residual * percolator::BOUND_SCALE,
            entitlement * self.claim_bound + remainder,
        );
        assert_eq!(
            group.vault,
            self.vault - (self.paid - self.receipt.paid_effective)
        );
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        for (actor, capital) in self.capitals.iter().enumerate() {
            assert_eq!(env.primary_portfolio(actor).capital.get(), *capital);
        }
        assert_public_stock_census("INV-038 receipt history", env).unwrap();
        assert_public_encumbrance_census("INV-038 receipt history", env).unwrap();
    }

    fn step(&mut self, env: &mut V16Svm, action: ReceiptHistoryAction) {
        use ReceiptHistoryAction::{Payout, Release, TryDelete};

        self.verify(env);
        let before = ReceiptHistoryFrame::read(env);
        let before_group = env.primary_market_state().1;
        let expected_paid = if matches!(action, Payout(_)) {
            self.entitlement().0
        } else {
            self.paid
        };
        let actor = if matches!(action, Release { .. }) {
            2
        } else {
            0
        };
        let result = match action {
            Release { .. } | Payout(ReceiptPayoutRoute::Close) => {
                env.close_resolved_primary_signed(actor)
            }
            Payout(ReceiptPayoutRoute::Claim) => env.claim_resolved_payout_topup_primary(actor),
            Payout(ReceiptPayoutRoute::Crank) => {
                env.crank_resolved_primary_signed(actor, env.current_slot(), vec![])
            }
            TryDelete => env.close_primary_portfolio(actor),
        };
        self.steps += 1;
        let after = ReceiptHistoryFrame::read(env);
        if matches!(action, Payout(ReceiptPayoutRoute::Claim)) && expected_paid == self.paid {
            assert!(result.is_ok(), "current-state duplicate claim must land");
            assert_eq!(
                after, before,
                "duplicate partial claim must be an exact no-op"
            );
        }
        if matches!(action, TryDelete) {
            assert!(
                result.is_err(),
                "a partial receipt must block account deletion"
            );
        }
        match result {
            Err(error) => {
                assert!(
                    !matches!(action, Release { .. }) && expected_paid == self.paid,
                    "required receipt progress {action:?}: {error}"
                );
                assert_eq!(after, before, "rejected receipt history frame");
                self.rejections += 1;
            }
            Ok(tx) => {
                assert!(tx.compute_units < TX_CU_LIMIT);
                before.assert_unrelated(&after, env, actor);
                if let Release { domain, amount } = action {
                    assert_eq!(
                        before_group.source_backing_buckets[domain].status,
                        percolator::BackingBucketStatusV16::Fresh
                    );
                    assert_eq!(
                        before_group.source_credit[domain].fresh_reserved_backing_num,
                        amount * percolator::BOUND_SCALE
                    );
                    assert_ne!(
                        env.primary_market_state().1.source_backing_buckets[domain].status,
                        percolator::BackingBucketStatusV16::Fresh
                    );
                    self.released += amount;
                }
                self.payments += usize::from(expected_paid > self.paid);
                self.paid = expected_paid;
                self.nonzero_remainders += usize::from(self.entitlement().1 != 0);
                for (owner, data) in before.portfolios.iter().enumerate() {
                    if owner != actor {
                        assert_eq!(&after.portfolios[owner], data);
                    }
                }
                for ((key, data), (after_key, after_data)) in
                    before.tokens.iter().zip(&after.tokens)
                {
                    assert_eq!(key, after_key);
                    if *key != env.vault && *key != env.actors[actor].destination_token {
                        assert_eq!(data, after_data);
                    }
                }
                assert_eq!(before.tokens.len(), after.tokens.len());
                assert_eq!(before.markets[1], after.markets[1]);
                assert_eq!(before.foreign_portfolio, after.foreign_portfolio);
                assert_eq!(before.backing_ledger, after.backing_ledger);
                assert_eq!(before.matchers, after.matchers);
                assert_eq!(before.lamports, after.lamports);
                assert_eq!(
                    before_group.insurance,
                    env.primary_market_state().1.insurance
                );
                assert_eq!(
                    before_group.backing_provider_earnings_total,
                    env.primary_market_state().1.backing_provider_earnings_total
                );
            }
        }
        self.verify(env);
    }
}

fn drain_receipt_history(
    env: &mut V16Svm,
    history: &mut ReceiptHistoryOracle,
    routes: &[ReceiptPayoutRoute],
    reverse: bool,
) {
    use ReceiptPayoutRoute::{Claim, Close, Crank};

    // The public prefix earns 20 and 24 units across the same 100 -> 150 mark.
    // Expired backing cannot convert either remaining face into senior capital.
    let faces = [20 * 50, 0, 24 * 50, 0, 0];
    assert_eq!(faces[0], history.receipt.terminal_positive_claim_face);
    let bound = history.claim_bound;
    let residual = history.residual + history.released;
    assert_eq!(faces.iter().sum::<u128>() * percolator::BOUND_SCALE, bound);
    let targets = faces.map(|face| {
        crate::support::reference_math::mul_div_floor_with_remainder(
            face,
            residual * percolator::BOUND_SCALE,
            bound,
        )
        .unwrap()
        .0
    });
    let capitals = history.capitals.clone();
    let initial_paid = [history.paid, 0, 0, 0, 0];
    let destinations = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token))
        .collect::<Vec<_>>();
    let vault = env.primary_market_state().1.vault;
    let mut paid = initial_paid;
    let mut cleared = [false; 5];
    let mut check = |env: &V16Svm| {
        let group = env.primary_market_state().1;
        let ledger = group.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_residual, residual);
        assert_eq!(group.payout_snapshot, residual);
        assert_eq!(
            ledger.current_payout_rate_num,
            residual * percolator::BOUND_SCALE
        );
        assert_eq!(ledger.current_payout_rate_den, bound);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num + ledger.terminal_claim_bound_unreceipted_num,
            bound
        );
        let mut payout_total = 0;
        let mut unreceipted = 0;
        for actor in 0..5 {
            let account = env.primary_portfolio(actor);
            let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
            assert!(account.capital.get() <= capitals[actor]);
            assert!(account.active_bitmap.iter().all(|word| word.get() == 0));
            let next_paid = if receipt.present {
                assert!(!cleared[actor], "cleared receipt cannot reappear");
                assert_eq!(receipt.terminal_positive_claim_face, faces[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    faces[actor] * percolator::BOUND_SCALE
                );
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert!(!receipt.finalized, "this product stays underfunded");
                assert_eq!(account.pnl.get(), 0);
                receipt.paid_effective
            } else if account.pnl.get() != 0 {
                assert_eq!(account.pnl.get(), faces[actor] as i128);
                unreceipted += faces[actor] * percolator::BOUND_SCALE;
                0
            } else {
                if faces[actor] != 0 {
                    assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                }
                cleared[actor] = true;
                targets[actor]
            };
            assert!(paid[actor] <= next_paid && next_paid <= targets[actor]);
            paid[actor] = next_paid;
            let payout = capitals[actor] - account.capital.get() + next_paid - initial_paid[actor];
            assert_eq!(
                u128::from(env.token_amount(env.actors[actor].destination_token)),
                u128::from(destinations[actor]) + payout,
                "owner-local receipt entitlement"
            );
            payout_total += payout;
        }
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, unreceipted);
        assert_eq!(group.vault, vault - payout_total);
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        assert_public_stock_census("INV-066/067/068 receipt drain", env).unwrap();
        assert_public_encumbrance_census("INV-066/067/068 receipt drain", env).unwrap();
    };
    check(env);
    let mut step = |env: &mut V16Svm, actor: usize, route: ReceiptPayoutRoute| {
        let before = ReceiptHistoryFrame::read(env);
        let receipt = env
            .primary_portfolio(actor)
            .resolved_payout_receipt
            .try_to_runtime()
            .unwrap();
        let destination = env.actors[actor].destination_token;
        let before_amount = env.token_amount(destination);
        let result = match route {
            Claim => env.claim_resolved_payout_topup_primary(actor),
            Close => env.close_resolved_primary_signed(actor),
            Crank => env.crank_resolved_primary_signed(actor, env.current_slot(), vec![]),
        };
        history.steps += 1;
        let after = ReceiptHistoryFrame::read(env);
        match result {
            Err(error) => {
                assert!(
                    !matches!(route, Claim),
                    "current-state claim must land: {error}"
                );
                assert_eq!(after, before, "terminal route rollback");
                let account = env.primary_portfolio(actor);
                assert_eq!(account.capital.get(), 0);
                assert_eq!(account.pnl.get(), 0);
                assert!(account
                    .source_domains
                    .iter()
                    .all(|source| !source.is_occupied()));
                assert!(!receipt.present || receipt.paid_effective == targets[actor]);
                history.rejections += 1;
            }
            Ok(tx) => {
                assert!(tx.compute_units < TX_CU_LIMIT);
                let payout = env
                    .token_amount(destination)
                    .checked_sub(before_amount)
                    .unwrap();
                history.payments += usize::from(payout != 0);
                before.assert_unrelated(&after, env, actor);
                assert_eq!(before.lamports, after.lamports);
                if matches!(route, Claim) {
                    assert_eq!(
                        u128::from(payout),
                        if receipt.present {
                            targets[actor] - receipt.paid_effective
                        } else {
                            0
                        }
                    );
                    if receipt
                        == env
                            .primary_portfolio(actor)
                            .resolved_payout_receipt
                            .try_to_runtime()
                            .unwrap()
                    {
                        assert_eq!(after, before, "duplicate claim must be an exact no-op");
                    }
                }
            }
        }
        check(env);
    };
    let mut owners = (0..5).collect::<Vec<_>>();
    let mut continuation = [Claim, Close, Crank];
    if reverse {
        owners.reverse();
        continuation.reverse();
    }
    let mut converged = false;
    for _ in 0..8 {
        let before = ReceiptHistoryFrame::read(env);
        for &actor in &owners {
            for &route in routes.iter().chain(&continuation) {
                step(env, actor, route);
            }
        }
        if ReceiptHistoryFrame::read(env) == before {
            converged = true;
            break;
        }
    }
    assert!(
        converged,
        "eight sweeps must reach an exact terminal fixed point"
    );
    for actor in 0..5 {
        assert!(
            !env.primary_portfolio(actor)
                .resolved_payout_receipt
                .try_to_runtime()
                .unwrap()
                .present
        );
        let before = ReceiptHistoryFrame::read(env);
        step(env, actor, Claim);
        assert_eq!(ReceiptHistoryFrame::read(env), before);
        assert_eq!(
            u128::from(env.token_amount(env.actors[actor].destination_token)),
            u128::from(destinations[actor]) + capitals[actor] + targets[actor]
                - initial_paid[actor]
        );
    }
    for actor in owners {
        let before = ReceiptHistoryFrame::read(env);
        let (cfg, group) = env.primary_market_state();
        let portfolio = env.actors[actor].portfolio;
        let rent = env.account_lamports(portfolio);
        let market_rent = env.account_lamports(env.market);
        assert!(rent > 0);
        let tx = env
            .close_primary_portfolio(actor)
            .expect("paid owner must dematerialize");
        assert!(tx.compute_units < TX_CU_LIMIT);
        history.steps += 1;
        let after = ReceiptHistoryFrame::read(env);
        before.assert_unrelated(&after, env, actor);
        assert_eq!(
            before.tokens, after.tokens,
            "portfolio close cannot pay again"
        );
        assert_eq!(env.account_lamports(portfolio), 0);
        assert!(env
            .svm
            .get_account(&portfolio)
            .unwrap_or_default()
            .data
            .is_empty());
        assert_eq!(env.account_lamports(env.market), market_rent + rent);
        let (after_cfg, mut after_group) = env.primary_market_state();
        assert_eq!(cfg, after_cfg);
        assert_eq!(
            after_group.materialized_portfolio_count + 1,
            group.materialized_portfolio_count
        );
        after_group.materialized_portfolio_count = group.materialized_portfolio_count;
        assert_eq!(group, after_group);
        env.claim_resolved_payout_topup_primary(actor)
            .expect_err("closed-portfolio claim is stale");
        history.steps += 1;
        history.rejections += 1;
        assert_eq!(
            ReceiptHistoryFrame::read(env),
            after,
            "stale claim rollback"
        );
        assert_public_stock_census("INV-068 dematerialized receipt owner", env).unwrap();
        assert_public_encumbrance_census("INV-068 dematerialized receipt owner", env).unwrap();
    }
    assert_eq!(env.primary_market_state().1.materialized_portfolio_count, 0);
}

fn run_receipt_rounding_history(
    amounts: [u128; 2],
    expiry_gap: u64,
    routes: &[ReceiptPayoutRoute],
    eager: bool,
    coalesce_releases: bool,
    drain: Option<bool>,
) -> (ReceiptHistoryFrame, bool) {
    use ReceiptHistoryAction::{Payout, Release, TryDelete};

    assert!(!coalesce_releases || (!eager && drain.is_none()));
    let mut env =
        crate::support::fuzz_model::public_resolved_receipt_seed(amounts, 13 + expiry_gap).unwrap();
    let mut oracle = ReceiptHistoryOracle::new(&env);
    assert_eq!(oracle.paid, oracle.entitlement().0);
    let mut separately_rounded_paid = oracle.paid;
    env.begin_public_trace();
    oracle.step(&mut env, TryDelete);
    for &route in routes {
        oracle.step(&mut env, Payout(route));
    }
    for (index, domain) in [3, 5].into_iter().enumerate() {
        env.warp_to_slot(
            13 + if index == 0 && !coalesce_releases {
                0
            } else {
                expiry_gap
            },
        );
        let coalesced_prefix = (coalesce_releases && index == 0).then(|| {
            let group = env.primary_market_state().1;
            for domain in [3, 5] {
                let bucket = group.source_backing_buckets[domain];
                assert_eq!(bucket.status, percolator::BackingBucketStatusV16::Fresh);
                assert!(bucket.expiry_slot <= env.current_slot());
            }
            (group.source_backing_buckets[5], group.source_credit[5])
        });
        // Seed capital backs domain 3 by 250 atoms. Domain 5 receives two solvent
        // five-price-atom moves over four units before its counterparty exhausts capital.
        let released = amounts[index] + [250, 2 * 5 * 4][index];
        separately_rounded_paid += crate::support::reference_math::mul_div_floor_with_remainder(
            oracle.receipt.terminal_positive_claim_face,
            released * percolator::BOUND_SCALE,
            oracle.claim_bound,
        )
        .unwrap()
        .0;
        oracle.step(
            &mut env,
            Release {
                domain,
                amount: released,
            },
        );
        if let Some(sibling) = coalesced_prefix {
            let group = env.primary_market_state().1;
            assert_eq!(
                (group.source_backing_buckets[5], group.source_credit[5]),
                sibling,
                "one bounded release must frame the other overdue source"
            );
        }
        oracle.step(&mut env, TryDelete);
        if eager || (index == 1 && drain.is_none()) {
            for &route in routes {
                oracle.step(&mut env, Payout(route));
            }
        }
    }
    let expected_payments = if eager {
        2
    } else {
        usize::from(drain.is_none())
    };
    assert_eq!(oracle.payments, expected_payments);
    assert!(oracle.nonzero_remainders > 0);
    let receipt = env
        .primary_portfolio(0)
        .resolved_payout_receipt
        .try_to_runtime()
        .unwrap();
    let destinations = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token))
        .collect::<Vec<_>>();
    let mut wrong = receipt;
    wrong.paid_effective += 1;
    assert!(!oracle.verify_receipt(wrong, &destinations));
    let mut wrong_owner = destinations.clone();
    wrong_owner[0] -= 1;
    wrong_owner[2] += 1;
    assert!(!oracle.verify_receipt(receipt, &wrong_owner));
    let lost_carry = oracle
        .entitlement()
        .0
        .checked_sub(separately_rounded_paid)
        .unwrap();
    assert!(lost_carry <= 2);
    if lost_carry != 0 && (eager || drain.is_none()) {
        let mut wrong = receipt;
        wrong.paid_effective = separately_rounded_paid;
        let mut wrong_payout = destinations.clone();
        wrong_payout[0] -= u64::try_from(lost_carry).unwrap();
        assert!(!oracle.verify_receipt(wrong, &wrong_payout));
    }
    if let Some(reverse) = drain {
        drain_receipt_history(&mut env, &mut oracle, routes, reverse);
    } else {
        assert_eq!(oracle.paid, oracle.entitlement().0);
    }
    let trace = env.finish_public_trace();
    trace.validate_public_execution().unwrap();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert_eq!(trace.steps.len(), oracle.steps);
    assert_eq!(
        trace.steps.iter().filter(|step| !step.succeeded).count(),
        oracle.rejections
    );
    eprintln!("INV-038/066/067/068 receipt eager={eager} coalesce_releases={coalesce_releases} drain={drain:?} amounts={amounts:?} gap={expiry_gap} routes={routes:?}: steps={}, rejections={}, payments={}, nonzero_remainders={}, lost_carry={lost_carry}", oracle.steps, oracle.rejections, oracle.payments, oracle.nonzero_remainders);
    (ReceiptHistoryFrame::read(&env), lost_carry != 0)
}

#[test]
fn v16_program_generated_receipt_histories_preserve_deferred_rounding() {
    use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
    use ReceiptPayoutRoute::{Claim, Close, Crank};

    let compare = |amounts, gap, routes: &[ReceiptPayoutRoute]| {
        let eager = run_receipt_rounding_history(amounts, gap, routes, true, false, None);
        for coalesce_releases in [false, true] {
            let deferred =
                run_receipt_rounding_history(amounts, gap, routes, false, coalesce_releases, None);
            assert!(
                eager.0 == deferred.0,
                "receipt-cadence endpoint mismatch: {amounts:?} gap={gap} routes={routes:?} coalesce_releases={coalesce_releases}"
            );
            assert_eq!(eager.1, deferred.1);
        }
        eager.1
    };
    let mut carry_witnesses = 0;
    for (amounts, gap, routes) in [
        ([1, 1], 1, [Claim, Close, Crank]),
        ([199, 199], 4, [Close, Crank, Claim]),
        ([127, 3], 2, [Crank, Claim, Close]),
    ] {
        carry_witnesses += usize::from(compare(amounts, gap, &routes));
    }
    assert!(
        carry_witnesses > 0,
        "boundary histories must distinguish cumulative from per-top-up floors"
    );

    let strategy = (
        prop::array::uniform2(1u128..=199),
        1u64..=4,
        prop::collection::vec(prop::sample::select(vec![Claim, Close, Crank]), 1..=6),
    );
    let config = ProptestConfig {
        cases: env_usize("PERCOLATOR_INV038_RECEIPT_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_receipt_history.txt",
            ),
        )),
        ..ProptestConfig::default()
    };
    let mut runner = TestRunner::new_with_rng(
        config,
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x38; 32]),
    );
    runner
        .run(&strategy, |(amounts, gap, routes)| {
            compare(amounts, gap, &routes);
            Ok(())
        })
        .unwrap();
}

pub(super) fn verify_generated_receipt_terminal_drain_histories() {
    use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
    use ReceiptPayoutRoute::{Claim, Close, Crank};

    let compare = |amounts, gap, routes: &[ReceiptPayoutRoute]| {
        let mut endpoint = None;
        for eager in [true, false] {
            for reverse in [false, true] {
                let observed =
                    run_receipt_rounding_history(amounts, gap, routes, eager, false, Some(reverse));
                if let Some(expected) = &endpoint {
                    assert!(expected == &observed,
                        "terminal-drain endpoint mismatch: amounts={amounts:?} gap={gap} routes={routes:?} eager={eager} reverse={reverse}");
                } else {
                    endpoint = Some(observed);
                }
            }
        }
    };
    for (amounts, gap, routes) in [
        ([1, 1], 1, [Claim, Close, Crank]),
        ([199, 199], 4, [Close, Crank, Claim]),
        ([127, 3], 2, [Crank, Claim, Close]),
    ] {
        compare(amounts, gap, &routes);
    }
    let strategy = (
        prop::array::uniform2(1u128..=199),
        1u64..=4,
        prop::collection::vec(prop::sample::select(vec![Claim, Close, Crank]), 1..=6),
    );
    let config = ProptestConfig {
        cases: env_usize("PERCOLATOR_INV068_RECEIPT_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_068_receipt_terminal_history.txt",
            ),
        )),
        ..ProptestConfig::default()
    };
    TestRunner::new_with_rng(
        config,
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x68; 32]),
    )
    .run(&strategy, |(amounts, gap, routes)| {
        compare(amounts, gap, &routes);
        Ok(())
    })
    .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_fractional_movement_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_fractional_max_dt_cranks_reach_target_and_preserve_terminal_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = verify_fractional_movement_convergence(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!(
            "independent fractional movement: target={}, settled={}, cranks={}, stalls={}/{}",
            discovery.target_price,
            discovery.settlement_price,
            discovery.successful_cranks,
            discovery.rejected_stalls,
            discovery.nonmoving_stalls,
        );
        prop_assert!(
            discovery.preserves_fractional_settlement(),
            "fractional movement failed to converge and conserve value: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_observation_omission_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_selected_observation_omission_rejects_and_preserves_rounded_transfer(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_observation_omission_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent observation-omission verification: {discovery:?}");
        prop_assert!(
            discovery.preserves_rounded_transfer(),
            "observation omission did not reject and recover safely: {:?}",
            discovery
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_038_composite_rounding_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_composite_scale_matrix_preserves_exact_composition(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_composite_rounding_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), CompositeRoundingScale::ALL.len());
        for (expected, discovery) in CompositeRoundingScale::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.scale, expected);
        }
        for discovery in discoveries {
            prop_assert!(!discovery.is_violation(), "{discovery:?}");
            prop_assert!(
                discovery.certifies_exact_composition_and_exit(),
                "{discovery:?}"
            );
            prop_assert_eq!(discovery.rounded_target, discovery.exact_mark);
            prop_assert_eq!(discovery.rounded_mark, discovery.exact_mark);
            prop_assert_eq!(discovery.certified_liq_deficit, 0);
            prop_assert_eq!(discovery.victim_capital_loss, 0);
            prop_assert_eq!(discovery.oi_reduction_q, 0);
            prop_assert_eq!(discovery.cranker_reward, 0);
            prop_assert_eq!(discovery.extracted_tokens, 0);
            prop_assert_eq!(discovery.victim_loss, 0);
            prop_assert_eq!(discovery.cranker_excess, 0);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr329_pr381_composite_rounding_preservation_fuzz(
        (seed, case) in composite_rounding_strategy()
    ) {
        let reproduction = reproduce_composite_oracle_rounding(seed, case)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(reproduction.case, case);
        prop_assert_eq!(reproduction.rounded_target, reproduction.exact_mark);
        prop_assert_eq!(reproduction.rounded_mark, reproduction.exact_mark);
        prop_assert_eq!(reproduction.certified_liq_deficit, 0);
        prop_assert_eq!(reproduction.victim_capital_loss, 0);
        prop_assert_eq!(reproduction.oi_reduction_q, 0);
        prop_assert_eq!(reproduction.cranker_reward, 0);
        prop_assert_eq!(reproduction.extracted_tokens, 0);
    }

    #[test]
    fn v16_program_pr253_rounded_funding_omission_rejection_fuzz(
        seed in rounded_funding_seed_strategy()
    ) {
        let reproduction = reproduce_rounded_funding_omission(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(reproduction.omitted_rejected_nonprogress);
        prop_assert!(reproduction.omitted_exact_rollback);
        prop_assert_eq!(reproduction.attack_f_long_num, reproduction.control_f_long_num);
        prop_assert_eq!(reproduction.attack_f_short_num, reproduction.control_f_short_num);
        prop_assert_eq!(reproduction.victim_payout_loss, 0);
        prop_assert_eq!(reproduction.attacker_payout_gain, 0);
    }

    #[test]
    fn v16_program_pr365_fractional_cap_settlement_fuzz(
        seed in fractional_cap_settlement_seed_strategy()
    ) {
        let reproduction = reproduce_fractional_cap_settlement(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(reproduction.reached_target);
        prop_assert_eq!(reproduction.settlement_price, reproduction.target_price);
        prop_assert_eq!(reproduction.long_overpayment, 0);
        prop_assert_eq!(reproduction.short_underpayment, 0);
        prop_assert_eq!(
            u128::from(reproduction.long_payout) + u128::from(reproduction.short_payout),
            2_000_000
        );
    }
}
