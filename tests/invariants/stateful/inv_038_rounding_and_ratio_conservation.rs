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
//! carry, actual owner PnL and unchanged capital at every post-seed transaction through zero OI.
//! It distinguishes a successful reduction that retains weight from a later nonzero denominator
//! change with nonzero market and leg carry. K/F seed construction remains separately owned;
//! this is not a mixed funding/receipt generator or a cash-residue classification proof.
//! Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: all listed matrices are fixed-pin public-route certifications. The EWMA
//! partition products are complete for the four deployed trade transports and the two boundary
//! partitions above; they do not close unrelated social-loss or backing-ratio products.

use super::*;
use crate::support::{
    fuzz_model::{assert_public_stock_census, execute_trade_route},
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
    booked: [u128; 2],
    allocated: [u128; 2],
    capital: Vec<u128>,
    values: Vec<i128>,
    steps: usize,
    rejected: usize,
    bookings: usize,
    settlements: usize,
    health_checks: usize,
}

impl PublicBHistoryObserver {
    fn new(env: &V16Svm) -> Self {
        let snapshot = BHistorySnapshot::read(env);
        let out = Self {
            booked: [0; 2],
            allocated: [0; 2],
            capital: snapshot
                .accounts
                .iter()
                .map(|account| account.capital.get())
                .collect(),
            values: snapshot.accounts.iter().map(b_account_value).collect(),
            steps: 0,
            rejected: 0,
            bookings: 0,
            settlements: 0,
            health_checks: 0,
        };
        out.verify(&snapshot).expect("zero-B origin");
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
            let (_, index, remainder, dust, explicit) = snapshot.side(side);
            if dust >= percolator::SOCIAL_LOSS_DEN {
                return Err("B dust escaped its collateral-atom denominator".into());
            }
            let mut outstanding = remainder + dust + explicit * percolator::SOCIAL_LOSS_DEN;
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
                    outstanding += leg.loss_weight * gap + leg.b_rem;
                }
            }
            if self.booked[side] * percolator::SOCIAL_LOSS_DEN
                != self.allocated[side] * percolator::SOCIAL_LOSS_DEN + outstanding
            {
                return Err(format!(
                    "B side {side}: origin={}, allocated={}, outstanding_num={outstanding}",
                    self.booked[side], self.allocated[side]
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

        let old_close = before.accounts[1].close_progress.try_to_runtime().unwrap();
        let new_close = after.accounts[1].close_progress.try_to_runtime().unwrap();
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
            (0, 0, 0, 0, 0)
        );
        let loss_side = b_side(old_close.domain_side);
        if booked != 0 {
            assert_eq!(actor, 1);
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
            self.values[1] += i128::try_from(booked).unwrap();
            assert!(
                self.values[1] <= 0,
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

#[test]
fn v16_program_b_carry_survives_admitted_owner_weight_changes() {
    use crate::support::fuzz_model::public_b_close_seed;
    use percolator::SideV16;

    let side = SideV16::Short;
    let chunk = 250_007;
    let order = [3, 2, 0];
    let mut env = public_b_close_seed(side, POS_SCALE / 2 + 3, chunk).unwrap();
    // K/F belongs to the existing seed owner. Start the B ledger only after it is current.
    for actor in [2, 3] {
        let mut current = false;
        for _ in 0..8 {
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
            assert_public_stock_census("INV-038 pre-B K/F settlement", &env).unwrap();
            if tx.is_none() {
                current = true;
                break;
            }
        }
        assert!(current);
    }
    env.begin_public_trace();
    let mut observer = PublicBHistoryObserver::new(&env);
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
    assert_eq!(trace.steps.len(), observer.steps);
    assert_eq!(
        trace.steps.iter().filter(|step| !step.succeeded).count(),
        observer.rejected
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
