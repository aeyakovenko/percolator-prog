//! INV-024 - attributed quote-value conservation.
//!
//! Aggregate vault and capital conservation cannot detect a route that debits
//! the correct loser but credits the wrong winner. This exhaustive public-SBF
//! matrix opens through each of the four trade routes, settles both possible
//! account-A sides, closes through each route, converts the exact released PnL,
//! and withdraws both owners. The shared oracle requires exact owner-level
//! capital, PnL, SPL payout, custody, claim cleanup, token supply, and unrelated
//! account frames at each economically distinct stage.
//! A separate bounded solvent history checks each transaction across early/end-only
//! and split/whole withdrawals, carrying payouts through later losses and deposits.
//! Both loser-first and winner-first settlement retain that per-owner history.
//! Seeded splits place a stale withdrawal before a freshly signed continuation in
//! either round; rejection preserves the ledger and rejection-free terminal payouts.
//! Conversion is atomic; this does not model partial conversion or impaired claims.
//! The terminal extension keeps that ledger across resolution, including prior live
//! payouts, a later loss, and unrelated fresh principal. It compares fully converted
//! capital with an unconverted final profit, then retries and deletes paid portfolios.

use super::*;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use solana_sdk::signature::Signer;
use support::fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census};
use support::v16_svm::{MarketConfig, V16Svm};

#[derive(Clone, Copy, Debug)]
struct Inv024PayoutHistory {
    principal: u128,
    gains: u128,
    losses: u128,
    fees: u128,
    converted: u128,
    terminal_pnl: u128,
    paid: u128,
    deleted: bool,
}

impl Inv024PayoutHistory {
    fn remaining(self) -> u128 {
        self.principal + self.gains - self.losses - self.fees - self.paid
    }

    fn observation(self) -> (u128, i128, u128) {
        let pnl = self.gains - self.converted - self.terminal_pnl;
        (self.remaining() - pnl, pnl as i128, self.paid)
    }
}

fn inv024_check_payout_observation(
    history: &[Inv024PayoutHistory; 5],
    observed: &[(u128, i128, u128); 5],
) -> Result<(), String> {
    for actor in 0..5 {
        if observed[actor] != history[actor].observation() {
            return Err(format!(
                "actor {actor}: capital/pnl/paid {:?} != history {:?}; {:?}",
                observed[actor],
                history[actor].observation(),
                history[actor],
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Inv024PayoutExit {
    Live,
    ResolvedCapital,
    ResolvedPnl,
}

// Only fully funded one-unit rounds are admitted: losses debit capital, gains stay
// in PnL until one complete conversion, and no funding or maintenance fee accrues.
fn inv024_run_payout_prefix_history(
    routes: [TradeRoute; 4],
    first_winner: usize,
    payout_schedule: usize,
    withdrawal_order: [usize; 2],
    exit: Inv024PayoutExit,
    winner_settles_first: bool,
    split_seed: Option<u64>,
    retry_round: Option<usize>,
) -> Result<([u128; 2], usize, usize, usize), String> {
    use percolator::POS_SCALE;
    use percolator_prog::error::PercolatorError;
    use percolator_prog::ix::CrankObservationHint;
    use support::fuzz_model::execute_trade_route;

    const DEPOSIT: u128 = 2_000_000;
    const EXTRA: u128 = 17_003;
    const SENIOR_EXTRA: u128 = 37_009;
    const PRICES: [u64; 3] = [1_000_000, 1_100_001, 1_160_004];
    if retry_round
        .is_some_and(|round| round >= 2 || payout_schedule != 2 || exit != Inv024PayoutExit::Live)
    {
        return Err(
            "withdrawal retry requires a live split payout in one of the two rounds".into(),
        );
    }
    let mut split_rng = split_seed.map(XorShiftRng::seed_from_u64);
    let endowments = [
        (DEPOSIT + EXTRA) as u64,
        (DEPOSIT + EXTRA) as u64,
        1 + if exit == Inv024PayoutExit::Live {
            0
        } else {
            SENIOR_EXTRA as u64
        },
        1,
        1,
    ];
    let mut env = V16Svm::new(
        [0x24; 32],
        MarketConfig {
            initial_price: PRICES[0],
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            actor_deposits: [1; 5],
            actor_token_balances: endowments,
            ..MarketConfig::default()
        },
    );
    // V16Svm::new has already executed each one-atom public setup deposit.
    let mut history = [Inv024PayoutHistory {
        principal: 1,
        gains: 0,
        losses: 0,
        fees: 0,
        converted: 0,
        terminal_pnl: 0,
        paid: 0,
        deleted: false,
    }; 5];
    let owners = [0, 1, 2, 3, 4];
    let ids = owners.map(|actor| env.primary_portfolio_id(actor));
    let supply = env.token_supply_observed();
    let fixed_portfolios = [env.primary_portfolio_data(2), env.primary_portfolio_data(3)];
    let foreign = (env.market_data(true), env.foreign_portfolio_data());
    let token_frame = env.all_token_account_data();
    let mut tracked_accounts: Vec<_> = token_frame.iter().map(|(key, _)| *key).collect();
    tracked_accounts.extend([
        env.market,
        env.foreign_market,
        env.mint,
        env.backing_domain_ledger,
        env.foreign_actor.portfolio,
        env.foreign_actor.signer.pubkey(),
    ]);
    for actor in &env.actors {
        tracked_accounts.extend([
            actor.portfolio,
            actor.signer.pubkey(),
            actor.matcher_context,
        ]);
    }
    let snapshot = |env: &V16Svm| {
        tracked_accounts
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    let mut mutable_tokens = vec![env.vault];
    for actor in 0..if exit == Inv024PayoutExit::Live { 2 } else { 5 } {
        mutable_tokens.extend([
            env.actors[actor].source_token,
            env.actors[actor].destination_token,
        ]);
    }
    let observe = |env: &V16Svm, history: &[Inv024PayoutHistory; 5]| {
        owners.map(|actor| {
            let (capital, pnl) = if history[actor].deleted {
                (0, 0)
            } else {
                let portfolio = env.primary_portfolio(actor);
                (portfolio.capital.get(), portfolio.pnl.get())
            };
            (
                capital,
                pnl,
                u128::from(env.token_amount(env.actors[actor].destination_token)),
            )
        })
    };
    let check = |env: &V16Svm, history: &[Inv024PayoutHistory; 5]| -> Result<(), String> {
        inv024_check_payout_observation(history, &observe(env, history))?;
        assert_public_stock_census("INV-024 payout history", env)?;
        assert_public_encumbrance_census("INV-024 payout history", env)?;
        for actor in 0..5 {
            if u128::from(env.token_amount(env.actors[actor].source_token))
                != u128::from(endowments[actor]) - history[actor].principal
            {
                return Err(format!("actor {actor}: external principal drift"));
            }
            if history[actor].deleted {
                if history[actor].remaining() != 0
                    || env
                        .svm
                        .get_account(&env.actors[actor].portfolio)
                        .is_some_and(|account| account.lamports != 0 || !account.data.is_empty())
                {
                    return Err(format!(
                        "actor {actor}: deleted portfolio or entitlement reappeared"
                    ));
                }
                continue;
            }
            let (header, owner) = percolator_prog::state::read_portfolio_owner_preflight(
                &env.primary_portfolio_data(actor),
            )
            .map_err(|error| format!("actor {actor}: owner preflight {error:?}"))?;
            if env.primary_portfolio_id(actor) != ids[actor]
                || header.market_group_id != env.market.to_bytes()
                || owner != env.actors[actor].signer.pubkey().to_bytes()
            {
                return Err(format!(
                    "actor {actor}: incarnation or external principal drift"
                ));
            }
        }
        let (_, group) = env.primary_market_state();
        let expected_vault = history.iter().map(|h| h.principal).sum::<u128>()
            - history.iter().map(|h| h.paid).sum::<u128>();
        let expected_capital = history.iter().map(|h| h.observation().0).sum::<u128>();
        if group.vault != expected_vault
            || u128::from(env.token_amount(env.vault)) != expected_vault
            || group.c_tot != expected_capital
            || group.insurance != history.iter().map(|h| h.fees).sum::<u128>()
            || group.materialized_portfolio_count
                != history.iter().filter(|h| !h.deleted).count() as u64
            || env.token_supply_observed() != supply
        {
            return Err("history-derived capital, fee destination, or custody drift".into());
        }
        if (exit == Inv024PayoutExit::Live
            && [env.primary_portfolio_data(2), env.primary_portfolio_data(3)] != fixed_portfolios)
            || (env.market_data(true), env.foreign_portfolio_data()) != foreign
        {
            return Err("unrelated portfolio or market changed".into());
        }
        let tokens = env.all_token_account_data();
        if tokens.len() != token_frame.len()
            || token_frame
                .iter()
                .zip(tokens)
                .any(|((key, before), (after_key, after))| {
                    *key != after_key || (!mutable_tokens.contains(key) && *before != after)
                })
        {
            return Err("unrelated token frame changed".into());
        }
        Ok(())
    };
    let mut checked_steps = 0;
    let mut early_payouts = 0;
    let mut rejected_steps = 0;
    macro_rules! step {
        ($label:expr, $call:expr, $update:block) => {{
            env.begin_public_trace();
            let result = $call;
            let trace = env.finish_public_trace();
            result.map_err(|error| format!("step {checked_steps} {}: {error}", $label))?;
            trace.validate_public_execution()?;
            if trace.steps.len() != 1 {
                return Err(format!("{} hid an unchecked helper transaction", $label));
            }
            $update
            check(&env, &history)
                .map_err(|error| format!("step {checked_steps} {}: {error}", $label))?;
            checked_steps += 1;
        }};
    }
    check(&env, &history)?;
    for actor in 0..2 {
        step!("deposit", env.deposit_primary(actor, DEPOSIT - 1), {
            history[actor].principal += DEPOSIT - 1;
        });
    }
    for round in 0..2 {
        let winner = first_winner ^ round;
        let loser = 1 - winner;
        let gain = u128::from(PRICES[round + 1] - PRICES[round]);
        let size = if winner == 0 {
            POS_SCALE as i128
        } else {
            -(POS_SCALE as i128)
        };
        let fee_bps = [7, 13][round];
        for closing in [false, true] {
            let route = routes[round * 2 + usize::from(closing)];
            let price = PRICES[round + usize::from(closing)];
            if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                step!("matcher authorization", env.set_matcher_config(1, 1), {});
            }
            // CPI has a taker cap, not a bilateral fee. This fixture's base fee
            // and matcher spread are zero; only signed no-CPI fees are charged.
            let fee = if matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
                support::reference_math::mul_div_ceil(
                    u128::from(price),
                    u128::from(fee_bps),
                    10_000,
                )?
            } else {
                0
            };
            step!(
                "trade",
                execute_trade_route(
                    &mut env,
                    route,
                    0,
                    1,
                    0,
                    if closing { -size } else { size },
                    price,
                    fee_bps,
                ),
                {
                    for ledger in &mut history[..2] {
                        ledger.fees += fee;
                    }
                }
            );
            if closing {
                break;
            }
            let slot = round as u64 + 2;
            env.warp_to_slot(slot);
            step!(
                "authenticated mark",
                env.push_auth_mark(0, slot, PRICES[round + 1]),
                {}
            );
            for _ in 0..8 {
                if env.primary_market_state().1.assets[0].slot_last >= slot {
                    break;
                }
                step!(
                    "market crank",
                    env.crank(
                        4,
                        slot,
                        vec![CrankObservationHint {
                            asset_index: 0,
                            oracle_accounts: env.primary_profile(0).oracle_leg_count,
                        }]
                    ),
                    {}
                );
            }
            if env.primary_market_state().1.assets[0].slot_last != slot {
                return Err("market did not reach the authenticated settlement slot".into());
            }
            let settlement_order = if winner_settles_first {
                [winner, loser]
            } else {
                [loser, winner]
            };
            for actor in settlement_order {
                step!("account settlement", env.crank(actor, slot, vec![]), {
                    if actor == winner {
                        history[actor].gains += gain;
                    } else {
                        history[actor].losses += gain;
                    }
                });
            }
        }
        if exit != Inv024PayoutExit::Live && round == 1 {
            step!(
                "unrelated principal beside pending profit",
                env.deposit_primary(2, SENIOR_EXTRA),
                {
                    history[2].principal += SENIOR_EXTRA;
                }
            );
        }
        if gain > history[winner].remaining()
            || gain != history[winner].gains - history[winner].converted
        {
            return Err("conversion exceeds independently modeled claim or unconverted PnL".into());
        }
        if exit != Inv024PayoutExit::ResolvedPnl || round == 0 {
            step!("full conversion", env.convert_released_pnl(winner, gain), {
                history[winner].converted += gain;
            });
        }
        let schedule = if exit != Inv024PayoutExit::Live && round == 1 {
            0
        } else {
            payout_schedule
        };
        let chunks = match schedule {
            0 => vec![],
            1 => vec![gain],
            2 => {
                let first = split_rng
                    .as_mut()
                    .map_or(1, |rng| rng.gen_range(2..gain - 1));
                vec![first, gain - first]
            }
            _ => return Err("unsupported payout schedule".into()),
        };
        // Retain the second chunk before the first consumes the owner's sequence.
        let mut retained =
            (retry_round == Some(round)).then(|| env.build_retained_withdrawal(winner, chunks[1]));
        for (chunk, amount) in chunks.into_iter().enumerate() {
            if amount > history[winner].remaining() || amount > history[winner].observation().0 {
                return Err("requested payout exceeds independently modeled claim/capital".into());
            }
            if chunk == 1 {
                if let Some(retained) = retained.take() {
                    step!(
                        "stale second withdrawal chunk",
                        match env.land_retained(retained) {
                            Err(error)
                                if error.contains(&format!(
                                    "Custom({})",
                                    PercolatorError::EngineStale as u32
                                )) =>
                                Ok(()),
                            Err(error) => Err(format!("unexpected withdrawal rejection: {error}")),
                            Ok(_) => Err("stale withdrawal unexpectedly succeeded".into()),
                        },
                        {}
                    );
                    rejected_steps += 1;
                    let mut paid_on_error = history;
                    paid_on_error[winner].paid += amount;
                    assert!(
                        inv024_check_payout_observation(&paid_on_error, &observe(&env, &history))
                            .is_err(),
                        "a rejected attempt must not advance cumulative owner payouts"
                    );
                }
            }
            step!("early withdrawal", env.withdraw_primary(winner, amount), {
                history[winner].paid += amount;
            });
            early_payouts += 1;
            // Observation-only mutations, never program-state injection: global
            // conservation and a fresh episode must not erase the recipient bound.
            let actual = observe(&env, &history);
            let mut wrong_owner = actual;
            wrong_owner[winner].0 -= 1;
            wrong_owner[loser].0 += 1;
            assert!(inv024_check_payout_observation(&history, &wrong_owner).is_err());
            let mut forgotten_payout = history;
            forgotten_payout[winner].paid = 0;
            assert!(inv024_check_payout_observation(&forgotten_payout, &actual).is_err());
        }
        if round == 0 {
            step!("inter-round deposit", env.deposit_primary(winner, EXTRA), {
                history[winner].principal += EXTRA;
            });
        }
    }
    if exit != Inv024PayoutExit::Live {
        let unconverted = history.iter().map(|h| h.gains - h.converted).sum::<u128>();
        let group = env.primary_market_state().1;
        if (unconverted != 0) != (exit == Inv024PayoutExit::ResolvedPnl)
            || group.source_claim_bound_total_num != unconverted * percolator::BOUND_SCALE
            || group.vault - group.c_tot - group.insurance != unconverted
        {
            return Err(
                "terminal seed lacks its input-derived face and fully funded residual".into(),
            );
        }
        step!("resolve with prior owner payouts", env.resolve_market(), {});
        if env.primary_market_state().1.mode != percolator::MarketModeV16::Resolved {
            return Err("public resolution did not change mode".into());
        }
        // The senior precedes both traders in one order and follows them in the other.
        let order = if withdrawal_order == [0, 1] {
            [0, 1, 2, 3, 4]
        } else {
            [4, 3, 2, 1, 0]
        };
        for actor in order {
            let due = history[actor].remaining();
            let pnl = history[actor].gains - history[actor].converted;
            step!(
                "terminal owner payout",
                env.close_resolved_primary(actor),
                {
                    history[actor].paid += due;
                    history[actor].terminal_pnl += pnl;
                }
            );
            let portfolio = env.primary_portfolio(actor);
            let receipt = portfolio
                .resolved_payout_receipt
                .try_to_runtime()
                .map_err(|error| format!("decode terminal receipt: {error:?}"))?;
            if portfolio.reserved_pnl.get() != 0
                || portfolio
                    .source_domains
                    .iter()
                    .any(|source| source.is_occupied())
                || (receipt.present && !receipt.finalized)
            {
                return Err(format!(
                    "actor {actor}: paid claim did not reach terminal disposition"
                ));
            }
            let actual = observe(&env, &history);
            let mut wrong_owner = actual;
            wrong_owner[actor].2 -= 1;
            wrong_owner[(actor + 1) % 5].2 += 1;
            assert!(inv024_check_payout_observation(&history, &wrong_owner).is_err());
            let mut forgotten_live_payout = history;
            if actor == first_winner && payout_schedule != 0 {
                forgotten_live_payout[actor].paid -= u128::from(PRICES[1] - PRICES[0]);
                assert!(inv024_check_payout_observation(&forgotten_live_payout, &actual).is_err());
            }
            // Retries are fresh public transactions, not SVM signature-cache replays.
            for topup in [false, true] {
                let before = snapshot(&env);
                step!(
                    "terminal payout retry",
                    {
                        if topup && receipt.present {
                            env.claim_resolved_payout_topup_primary(actor)?;
                        } else {
                            use percolator_prog::error::PercolatorError;
                            let (result, expected) = if topup {
                                (
                                    env.claim_resolved_payout_topup_primary(actor),
                                    PercolatorError::EngineLockActive,
                                )
                            } else {
                                (
                                    env.close_resolved_primary(actor),
                                    PercolatorError::EngineNonProgress,
                                )
                            };
                            let error = result.expect_err("terminal route without work rejects");
                            let expected = format!("Custom({})", expected as u32);
                            if !error.contains(&expected) {
                                return Err(format!(
                                    "terminal retry returned the wrong error: {error}"
                                ));
                            }
                        }
                        if snapshot(&env) != before {
                            Err("retry changed the terminal fixed point".to_string())
                        } else {
                            Ok(())
                        }
                    },
                    {}
                );
            }
        }
        for actor in order {
            let owner = env.actors[actor].signer.pubkey();
            let rent = env
                .svm
                .get_account(&env.actors[actor].portfolio)
                .unwrap()
                .lamports;
            let owner_before = env.svm.get_account(&owner).unwrap().lamports;
            let slab_before = env.svm.get_account(&env.market).unwrap().lamports;
            step!(
                "delete paid portfolio",
                env.close_primary_portfolio(actor),
                {
                    history[actor].deleted = true;
                }
            );
            if env.svm.get_account(&owner).unwrap().lamports != owner_before
                || env.svm.get_account(&env.market).unwrap().lamports != slab_before + rent
            {
                return Err(format!(
                    "actor {actor}: portfolio rent was not returned to the slab exactly"
                ));
            }
        }
        if history.iter().any(|h| h.remaining() != 0 || !h.deleted) {
            return Err("terminal history left an unpaid or materialized owner".into());
        }
    } else {
        for actor in withdrawal_order {
            let amount = history[actor].remaining();
            if amount == 0 || amount != history[actor].observation().0 {
                return Err("final solvent entitlement is not positive spendable capital".into());
            }
            step!("final withdrawal", env.withdraw_primary(actor, amount), {
                history[actor].paid += amount;
            });
        }
    }
    if history[..2]
        .iter()
        .any(|h| h.remaining() != 0 || h.fees == 0)
        || env.primary_market_state().1.source_claim_bound_total_num != 0
    {
        return Err("terminal claim cleanup or nonzero fee witness missing".into());
    }
    Ok((
        [history[0].paid, history[1].paid],
        checked_steps,
        early_payouts,
        rejected_steps,
    ))
}

#[test]
fn v16_program_payout_prefix_histories_preserve_each_owners_entitlement() {
    let mut worlds = 0;
    let mut checked_steps = 0;
    let mut early_payouts = 0;
    let mut end_only_outcomes = [None; 2];
    for routes in [
        [
            TradeRoute::NoCpi,
            TradeRoute::BatchCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
        ],
        [
            TradeRoute::BatchNoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchCpi,
            TradeRoute::NoCpi,
        ],
    ] {
        for first_winner in 0..2 {
            for withdrawal_order in [[0, 1], [1, 0]] {
                for winner_settles_first in [false, true] {
                    for schedule in 0..3 {
                        let (paid, steps, early, rejected) = inv024_run_payout_prefix_history(
                            routes, first_winner, schedule, withdrawal_order,
                            Inv024PayoutExit::Live, winner_settles_first, None, None,
                        ).unwrap_or_else(|error| panic!(
                            "INV-024 routes={routes:?} first_winner={first_winner} schedule={schedule} order={withdrawal_order:?} winner_settles_first={winner_settles_first}: {error}"
                        ));
                        assert_eq!(early, [0, 2, 4][schedule]);
                        assert_eq!(rejected, 0);
                        assert_eq!(
                            *end_only_outcomes[first_winner].get_or_insert(paid),
                            paid,
                            "transport, settlement order, withdrawal timing, partition, or owner order changed total entitlement"
                        );
                        worlds += 1;
                        checked_steps += steps;
                        early_payouts += early;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert_eq!(early_payouts, 96);
    eprintln!("INV-024: {worlds} worlds, {checked_steps} checked transactions, {early_payouts} early payouts");
}

#[test]
fn v16_program_live_payout_histories_preserve_entitlement_through_resolution() {
    let mut worlds = 0;
    let mut checked_steps = 0;
    let mut early_payouts = 0;
    let mut outcomes = [None; 2];
    for routes in [
        [
            TradeRoute::NoCpi,
            TradeRoute::BatchCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
        ],
        [
            TradeRoute::BatchNoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchCpi,
            TradeRoute::NoCpi,
        ],
    ] {
        for first_winner in 0..2 {
            for order in [[0, 1], [1, 0]] {
                for schedule in 0..3 {
                    for exit in [
                        Inv024PayoutExit::ResolvedCapital,
                        Inv024PayoutExit::ResolvedPnl,
                    ] {
                        let (paid, steps, early, rejected) = inv024_run_payout_prefix_history(
                            routes, first_winner, schedule, order, exit, false, None, None,
                        ).unwrap_or_else(|error| panic!(
                            "INV-024/027/066/067 routes={routes:?} first_winner={first_winner} schedule={schedule} order={order:?} exit={exit:?}: {error}"
                        ));
                        assert_eq!(early, schedule);
                        assert_eq!(rejected, 0);
                        assert_eq!(
                            *outcomes[first_winner].get_or_insert(paid), paid,
                            "prior live payouts, conversion, transport or terminal order changed entitlement"
                        );
                        worlds += 1;
                        checked_steps += steps;
                        early_payouts += early;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert_eq!(early_payouts, 48);
    eprintln!("INV-024/027/066/067: {worlds} worlds, {checked_steps} checked transactions, {early_payouts} live payouts, {} terminal payouts and portfolio closes", worlds * 5);
}

#[test]
fn v16_program_seeded_withdrawal_retries_preserve_payout_prefix_entitlement() {
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::BatchCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
    ];
    let mut worlds = 0;
    let mut checked_steps = 0;
    let mut rejected_steps = 0;
    for seed in [0x24_u64, 0x427, 0x2026_0908] {
        for first_winner in 0..2 {
            let mut control = None;
            for retry_round in [None, Some(0), Some(1)] {
                let (paid, steps, early, rejected) = inv024_run_payout_prefix_history(
                    routes,
                    first_winner,
                    2,
                    [1 - first_winner, first_winner],
                    Inv024PayoutExit::Live,
                    false,
                    Some(seed),
                    retry_round,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "INV-024 seed={seed:#x} first_winner={first_winner} retry_round={retry_round:?}: {error}"
                    )
                });
                assert_eq!(early, 4);
                assert_eq!(rejected, usize::from(retry_round.is_some()));
                assert_eq!(
                    *control.get_or_insert((paid, steps - rejected)),
                    (paid, steps - rejected),
                    "rejected withdrawal changed owner entitlement or successful continuation"
                );
                worlds += 1;
                checked_steps += steps;
                rejected_steps += rejected;
            }
        }
    }
    assert_eq!(worlds, 18);
    assert_eq!(rejected_steps, 12);
    eprintln!("INV-024 seeded retries: {worlds} worlds, {checked_steps} checked transactions, {rejected_steps} rejected withdrawals");
}

#[test]
fn v16_program_all_trade_route_pairs_preserve_realized_pnl_owner_attribution() {
    const ROUTES: [TradeRoute; 4] = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut worlds = 0usize;
    for open_route in ROUTES {
        for close_route in ROUTES {
            for account_a_long in [false, true] {
                verify_attributed_pnl_roundtrip(
                    [0x24; 32],
                    open_route,
                    close_route,
                    account_a_long,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "INV-024 {open_route:?}/{close_route:?}/account_a_long={account_a_long}: {error}"
                    )
                });
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 4 * 4 * 2);
}

#[test]
fn v16_program_multi_episode_history_enforces_each_owners_exact_entitlement() {
    let route_orders = [
        [
            TradeRoute::NoCpi,
            TradeRoute::BatchCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
        ],
        [
            TradeRoute::BatchNoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchCpi,
            TradeRoute::NoCpi,
        ],
    ];
    for (index, routes) in route_orders.into_iter().enumerate() {
        let mut seed = [0x24; 32];
        seed[0] ^= index as u8;
        verify_multi_episode_entitlement_history(seed, routes)
            .unwrap_or_else(|error| panic!("INV-024 history {index}: {error}"));
    }
}

#[test]
fn v16_program_public_trace_enforces_authority_attributed_quote_flow() {
    let mut env =
        support::v16_svm::V16Svm::new([0x42; 32], support::v16_svm::MarketConfig::default());
    env.begin_public_trace();
    env.deposit_primary(0, 17)
        .expect("authenticated owner deposit");
    env.withdraw_primary(0, 3)
        .expect("authenticated owner withdrawal");
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("balanced owner/vault quote flows are valid public evidence");

    let source = env.actors[0].source_token;
    assert_eq!(
        trace
            .token_delta_for_accounts(&[source])
            .expect("source-token trace delta"),
        -17
    );
    assert!(
        trace.token_delta_for_accounts(&[source, source]).is_err(),
        "a duplicated payout-account query must fail closed"
    );

    let mut unbalanced = trace.clone();
    let source_delta = unbalanced.steps[0]
        .token_deltas
        .iter_mut()
        .find_map(|(key, delta)| (*key == source).then_some(delta))
        .expect("deposit source delta");
    *source_delta -= 1;
    assert!(
        unbalanced.validate_public_execution().is_err(),
        "a fabricated one-atom quote imbalance must not qualify as public evidence"
    );

    let mut duplicate_authority = trace.clone();
    let duplicated = duplicate_authority.steps[0].token_authorities[0];
    duplicate_authority.steps[0]
        .token_authorities
        .push(duplicated);
    assert!(
        duplicate_authority.validate_public_execution().is_err(),
        "duplicate SPL authority attribution must not qualify as public evidence"
    );

    let mut wrong_owner = trace;
    let source_authority = wrong_owner.steps[0]
        .token_authorities
        .iter_mut()
        .find_map(|(key, authority)| (*key == source).then_some(authority))
        .expect("deposit source authority");
    *source_authority = env.actors[1].signer.pubkey();
    assert!(
        wrong_owner.validate_public_execution().is_err(),
        "quote movement attributed to a different owner must not qualify as public evidence"
    );
}
