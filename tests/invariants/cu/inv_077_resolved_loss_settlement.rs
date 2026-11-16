//! INV-077: losses first settled after resolution, at the source cap.
//!
//! A market can resolve while an account's legs still carry losses nobody has
//! refreshed. The resolved close then settles them, and with source claims in
//! the account each losing leg costs work for every claim its loss burns
//! through. Settled all in one call, fourteen legs over 28 claims did not fit
//! in a transaction: the account could never be closed, its legs stayed in the
//! market's payout blockers, and no account with a profit was paid, whoever its
//! counterparty was.
//!
//! The engine now settles such an account a bounded number of legs per call.
//! These tests close the largest shapes in both orders and require that every
//! call fits, that both accounts finish without waiting for any deadline, and
//! that everyone is paid what the books say.

use super::*;

/// Costliest resolved call measured over these shapes: 1,156,073.
const RESOLVED_CALL_CU_LIMIT: u64 = 1_250_000;
const LP: usize = 0;
const TAKER: usize = 1;

struct Closed {
    /// SPL tokens paid to the LP and to the taker.
    paid: [u128; 2],
    calls: [usize; 2],
    max_cu: u64,
    vault_left: u128,
}

fn resolved_crank(
    env: &mut V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    now_slot: u64,
) -> Result<(u64, u64), String> {
    let destination = Pubkey::new_unique();
    env.svm
        .set_account(
            destination,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let cu = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot,
            observations: vec![],
        },
        vec![
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )?;
    Ok((env.token_amount(destination), cu))
}

/// Cranks `portfolio` until it is closed out or has to wait for someone else.
/// A call may only succeed within the limit or report that it made no progress.
fn close_as_far_as_possible(
    env: &mut V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    now_slot: u64,
    label: &str,
) -> (u128, usize, u64) {
    let (mut paid, mut calls, mut max_cu) = (0u128, 0usize, 0u64);
    for _ in 0..128 {
        if resolved_portfolio_is_terminal(env, portfolio) {
            return (paid, calls, max_cu);
        }
        match resolved_crank(env, owner, portfolio, now_slot) {
            Ok((payout, cu)) => {
                assert_cu_within(label, cu, RESOLVED_CALL_CU_LIMIT);
                paid += u128::from(payout);
                calls += 1;
                max_cu = max_cu.max(cu);
            }
            Err(error) => {
                assert!(
                    is_engine_non_progress_error(&error),
                    "{label}: a resolved close call failed: {error}"
                );
                return (paid, calls, max_cu);
            }
        }
    }
    panic!("{label}: the resolved close did not finish in 128 calls");
}

/// Resolves the market where it stands and closes both accounts, the first as
/// far as it will go before the other is touched. Nobody waits for a deadline:
/// the clock stays at the slot after resolution.
fn resolve_and_close(mut pair: AllSourceLienPair, lp_first: bool, label: &str) -> Closed {
    pair.env.resolve();
    assert_eq!(pair.env.market_state().1.mode, MarketModeV16::Resolved);
    let now = pair.slot + 1;
    pair.env.svm.warp_to_slot(now);
    let actors = [
        (LP, pair.lp_owner.pubkey(), pair.lp),
        (TAKER, pair.taker_owner.pubkey(), pair.taker),
    ];
    let order = if lp_first { [LP, TAKER] } else { [TAKER, LP] };
    let mut closed = Closed {
        paid: [0; 2],
        calls: [0; 2],
        max_cu: 0,
        vault_left: 0,
    };
    for _round in 0..8 {
        let before = closed.calls;
        for who in order {
            let (index, owner, portfolio) = actors[who];
            let (paid, calls, max_cu) =
                close_as_far_as_possible(&mut pair.env, owner, portfolio, now, label);
            closed.paid[index] += paid;
            closed.calls[index] += calls;
            closed.max_cu = closed.max_cu.max(max_cu);
        }
        if actors
            .iter()
            .all(|(_, _, portfolio)| resolved_portfolio_is_terminal(&pair.env, *portfolio))
        {
            let group = pair.env.market_state().1;
            assert_eq!(group.vault as u64, pair.env.token_amount(pair.env.vault));
            closed.vault_left = group.vault;
            return closed;
        }
        assert_ne!(closed.calls, before, "{label}: neither account can move");
    }
    panic!("{label}: the accounts did not close");
}

/// The LP, flat and with its capital back, holding a `CLAIM_PER_SOURCE` claim
/// at par in each of the 28 domains and no lien, opens `units[i]` short on
/// asset `i`; every one of those assets then doubles. Nobody refreshes the LP
/// or the taker afterwards.
fn unsettled_losses_over_par_claims(units: &[i128]) -> AllSourceLienPair {
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    pair.release_liens_down_to(0);
    pair.open_shorts_one_by_one(units);
    pair.move_marks_against_the_lp(units.len());
    pair
}

fn loss(units: &[i128]) -> u128 {
    units.iter().sum::<i128>() as u128 * (WIPE_OUT_PRICE - PRICE_LOW) as u128
}

/// Both close orders of a solvent pair over par claims. The loss is `loss`; the
/// provider principal in the 28 domains is nobody's to take here and stays.
///
/// Loser first, everyone is paid exactly. Winner first, the winner is still paid
/// exactly; the LP's own claims in the domains it owes are then burned below
/// par (the engine's existing resolved rule), so it can end short, and what it
/// is short of stays in the vault: nothing is paid to anyone else.
fn close_both_orders(units: &[i128], label: &str) -> u64 {
    let claims = SOURCES as u128 * CLAIM_PER_SOURCE;
    let principal = SOURCES as u128 * PROVIDER_PER_SOURCE;
    let loss = loss(units);
    assert!(loss >= claims, "the losses burn through every claim");
    let mut max_cu = 0;
    for lp_first in [true, false] {
        let pair = unsettled_losses_over_par_claims(units);
        let lp_before = pair.env.portfolio_state(pair.lp);
        let taker_before = pair.env.portfolio_state(pair.taker);
        assert_eq!(lp_before.pnl.get(), claims as i128);
        assert_eq!(taker_before.pnl.get(), 0);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&lp_before)) as usize,
            units.len()
        );
        let lp_fair = lp_before.capital.get() + claims - loss;
        let taker_fair = taker_before.capital.get() + loss;

        let closed = resolve_and_close(pair, lp_first, label);
        assert_eq!(closed.paid[TAKER], taker_fair, "{label}: the winner");
        if lp_first {
            assert_eq!(
                closed.paid[LP], lp_fair,
                "{label}: the loser, closing first"
            );
        } else {
            assert!(closed.paid[LP] <= lp_fair, "{label}: the loser");
        }
        assert_eq!(
            closed.paid[LP] + closed.vault_left,
            lp_fair + principal,
            "{label}: what the loser is short of stays in the vault"
        );
        max_cu = max_cu.max(closed.max_cu);
        println!(
            "INV-077 resolved loss settlement, {label}, {}: LP {} calls, taker {} calls, max CU {}",
            if lp_first {
                "loser first"
            } else {
                "winner first"
            },
            closed.calls[LP],
            closed.calls[TAKER],
            closed.max_cu
        );
    }
    max_cu
}

/// Fourteen legs each lose through about two claims. Settled in one call this
/// never fit; the account, its counterparty and every other winner in the
/// market stayed unpaid.
#[test]
fn v16_program_fourteen_leg_loss_first_settled_after_resolution_closes_within_the_limit() {
    const UNITS: [i128; 14] = [15, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21];
    close_both_orders(&UNITS, "fourteen legs over 28 claims");
}

/// The costliest calls. Two legs burn through all 28 claims between them while
/// twelve more are open: once as the first chunk, and once as the legs the
/// order reaches last, which the ordinary settlement then takes together with
/// the rest of the close.
#[test]
fn v16_program_two_legs_burning_every_claim_at_the_fourteen_leg_shape_fit() {
    let first = close_both_orders(
        &[140, 141, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        "two big legs first",
    );
    let last = close_both_orders(
        &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 140, 141],
        "two big legs last",
    );
    assert!(
        last > first,
        "the ordinary settlement of the last legs is the costliest call"
    );
}

/// Claims in lapsed domains: the buckets behind all 28 claims have expired and
/// the LP holds exactly the capital the move costs. Its losses are paid from
/// capital leg by leg, and its claims are paid from the junior pool like any
/// other lapsed claim.
#[test]
fn v16_program_fourteen_leg_loss_over_lapsed_claims_first_settled_after_resolution_closes() {
    const BACKING_EXPIRY_SLOT: u64 = 60;
    const UNITS: [i128; 14] = [21; 14];
    let claims = SOURCES as u128 * CLAIM_PER_SOURCE;
    for lp_first in [true, false] {
        let mut pair = all_source_lien_pair_with_backing_expiry(BACKING_EXPIRY_SLOT);
        pair.redeposit_capital();
        pair.release_liens_down_to(0);
        pair.walk_the_clock_to(BACKING_EXPIRY_SLOT + 1);
        pair.crank_lp_until_idle();
        let spare = pair.env.portfolio_state(pair.lp).capital.get() - loss(&UNITS);
        let lp_owner = pair.lp_owner.insecure_clone();
        pair.env.withdraw_with_cu(&lp_owner, pair.lp, spare);
        pair.open_shorts_one_by_one(&UNITS);
        pair.move_marks_against_the_lp(UNITS.len());
        let taker_capital = pair.env.portfolio_state(pair.taker).capital.get();

        let closed = resolve_and_close(pair, lp_first, "fourteen legs over lapsed claims");
        assert_eq!(closed.paid[TAKER], taker_capital + loss(&UNITS));
        assert_eq!(closed.paid[LP], claims);
        println!(
            "INV-077 resolved loss settlement over lapsed claims, {}: max CU {}",
            if lp_first {
                "loser first"
            } else {
                "winner first"
            },
            closed.max_cu
        );
    }
}

/// Shapes on the Live lien bound. Their resolved settlement used to fail until
/// the backing deadline had passed; it now completes at once.
#[test]
fn v16_program_losses_on_the_lien_bound_first_settled_after_resolution_close_at_once() {
    for (legs, liened) in [(7usize, 27usize), (11, 6)] {
        for lp_first in [true, false] {
            let units = costliest_loss_units(legs, liened);
            let mut pair = all_source_lien_pair();
            pair.redeposit_capital();
            pair.release_liens_down_to(liened);
            pair.open_shorts_one_by_one(&units);
            let mut prices = vec![SMALL_LOSS_PRICE; legs];
            prices[0] = WIPE_OUT_PRICE;
            pair.move_marks(&prices);
            let loss = units[0] as u128 * (WIPE_OUT_PRICE - PRICE_LOW) as u128
                + units[1..].iter().sum::<i128>() as u128 * (SMALL_LOSS_PRICE - PRICE_LOW) as u128;
            let taker_fair = pair.env.portfolio_state(pair.taker).capital.get() + loss;
            let lp_fair = pair.env.portfolio_state(pair.lp).capital.get()
                + SOURCES as u128 * CLAIM_PER_SOURCE
                - loss;

            let label = format!("{legs} legs over {liened} liened claims");
            let closed = resolve_and_close(pair, lp_first, &label);
            assert_eq!(closed.paid[TAKER], taker_fair, "{label}");
            if lp_first {
                assert_eq!(closed.paid[LP], lp_fair, "{label}");
            } else {
                assert!(closed.paid[LP] <= lp_fair, "{label}");
            }
        }
    }
}

/// Accounts that never traded with the stuck account. X won 1,000 from Y on
/// asset 0 and both settled before resolution; F only deposited. While the
/// fourteen-leg account could not settle, X was paid nothing, capital included.
#[test]
fn v16_program_a_fourteen_leg_loser_does_not_hold_up_unrelated_winners() {
    const UNITS: [i128; 14] = [15, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21];
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    pair.release_liens_down_to(0);
    pair.open_shorts_one_by_one(&UNITS);

    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let deposits = [100_000u128, 100_000, 50_000];
    let mut portfolios = Vec::new();
    for (owner, deposit) in owners.iter().zip(deposits) {
        let portfolio = pair.env.create_portfolio(owner);
        pair.env.deposit(owner, portfolio, deposit);
        portfolios.push(portfolio);
    }
    pair.env.svm.expire_blockhash();
    pair.env
        .try_trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            10 * POS_SCALE as i128,
            PRICE_LOW,
            0,
        )
        .expect("X buys ten units from Y");
    pair.move_marks_against_the_lp(UNITS.len());
    let gain = 10 * (WIPE_OUT_PRICE - PRICE_LOW) as u128;
    for _ in 0..4 {
        for index in [1usize, 0] {
            if !certificate_is_current(&pair.env, portfolios[index]) {
                pair.env.crank(
                    portfolios[index],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: pair.slot,
                        observations: vec![],
                    },
                );
            }
        }
    }
    let x = pair.env.portfolio_state(portfolios[0]);
    let y = pair.env.portfolio_state(portfolios[1]);
    assert_eq!((x.capital.get(), x.pnl.get()), (deposits[0], gain as i128));
    assert_eq!((y.capital.get(), y.pnl.get()), (deposits[1] - gain, 0));

    pair.env.resolve();
    let now = pair.slot + 1;
    pair.env.svm.warp_to_slot(now);
    let label = "bystanders beside a fourteen-leg loser";

    // Before the fourteen-leg account is touched: the loser and the flat account
    // close, and the winner waits for the market's unsettled legs.
    let mut paid = [0u128; 3];
    for index in [2usize, 1, 0] {
        paid[index] += close_as_far_as_possible(
            &mut pair.env,
            owners[index].pubkey(),
            portfolios[index],
            now,
            label,
        )
        .0;
    }
    assert_eq!(paid, [0, deposits[1] - gain, deposits[2]]);
    assert!(!resolved_portfolio_is_terminal(&pair.env, portfolios[0]));

    // The fourteen-leg account and its counterparty settle, in a bounded number
    // of calls, and then the winner is paid in full.
    for _ in 0..4 {
        for (owner, portfolio) in [
            (pair.lp_owner.pubkey(), pair.lp),
            (pair.taker_owner.pubkey(), pair.taker),
        ] {
            close_as_far_as_possible(&mut pair.env, owner, portfolio, now, label);
        }
    }
    assert!(resolved_portfolio_is_terminal(&pair.env, pair.lp));
    assert!(resolved_portfolio_is_terminal(&pair.env, pair.taker));
    paid[0] +=
        close_as_far_as_possible(&mut pair.env, owners[0].pubkey(), portfolios[0], now, label).0;
    assert_eq!(paid[0], deposits[0] + gain);
    for portfolio in portfolios {
        assert!(resolved_portfolio_is_terminal(&pair.env, portfolio));
    }
    let group = pair.env.market_state().1;
    assert_eq!(group.resolved_payout_blocker_count, 0);
    assert_eq!(group.vault, SOURCES as u128 * PROVIDER_PER_SOURCE);
}
