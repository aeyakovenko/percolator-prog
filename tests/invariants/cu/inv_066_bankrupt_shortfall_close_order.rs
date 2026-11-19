//! INV-066: a bankrupt account's shortfall does not depend on who closes first.
//!
//! The mark moves by more than an account's margin between its last crank and resolution,
//! so the account reaches resolution owing more than it has. What it cannot pay is shared
//! by the positions on the other side of its asset, by loss weight, when it is booked.
//! Before the engine fix a winner that closed first detached its leg with its whole gain,
//! and its share fell on the winners still attached, their capital included: here a
//! bystander with a 0.2-unit position lost four fifths of its deposit.
//!
//! The fix keeps a position attached until the other side of its asset has settled. The
//! second test pins how long that can take: inside the owner window only its owner can
//! crank an account, so an owner that stays silent holds the positions that wait for it to
//! the end of the window, and no longer.
//!
//! Public instructions only, production risk parameters.

use super::*;

const PRICE: u64 = 1_000_000;
/// T is long 1,000.2 units on 5.2% of margin.
const T_DEPOSIT: u128 = 52_000_000;
const W_DEPOSIT: u128 = 200_000_000;
const X_DEPOSIT: u128 = 50_000_000;
const W_UNITS: u128 = 1_000;
/// X is short a fifth of a unit.
const X_FIFTHS: u128 = 1;
const T: usize = 0;
const W: usize = 1;
const X: usize = 2;
/// L, in the world that has it, is long one unit against W and stays solvent.
const L: usize = 3;
const L_DEPOSIT: u128 = 10_000_000;
/// Slots after resolution in which only its owner can crank an account.
const OWNER_WINDOW: u64 = 5;

struct GapWorld {
    env: V16CuEnv,
    actors: Vec<(Keypair, Pubkey)>,
    /// The mark after the gap.
    mark: u64,
    last_push: u64,
}

/// T opens against W and X at the initial price. The mark then falls by 4.7% in each of
/// two accrual windows and nobody cranks T.
fn gap_world() -> GapWorld {
    gap_world_with(OWNER_WINDOW, false)
}

/// The same world with an owner window of `owner_window` slots and, when `solvent_long`, a
/// fourth account L that is long one unit against W.
fn gap_world_with(owner_window: u64, solvent_long: bool) -> GapWorld {
    let mut params = production_risk_params();
    // No funding: gains and losses are exactly the price move.
    params.max_abs_funding_e9_per_slot = 0;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, owner_window);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    let mut actors = Vec::new();
    let deposits = [T_DEPOSIT, W_DEPOSIT, X_DEPOSIT, L_DEPOSIT];
    for &deposit in &deposits[..if solvent_long { 4 } else { 3 }] {
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        env.deposit(&owner, portfolio, deposit);
        actors.push((owner, portfolio));
    }
    let unit = POS_SCALE as i128;
    env.trade_asset_with_cu(
        0,
        &actors[T].0,
        actors[T].1,
        &actors[W].0,
        actors[W].1,
        W_UNITS as i128 * unit,
        PRICE,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &actors[T].0,
        actors[T].1,
        &actors[X].0,
        actors[X].1,
        X_FIFTHS as i128 * unit / 5,
        PRICE,
        0,
    );
    if solvent_long {
        env.trade_asset_with_cu(
            0,
            &actors[L].0,
            actors[L].1,
            &actors[W].0,
            actors[W].1,
            unit,
            PRICE,
            0,
        );
    }
    let mut mark = PRICE;
    let mut last_push = 0;
    for slot in [20u64, 40] {
        env.svm.warp_to_slot(slot);
        mark -= mark * 47 / 1000;
        env.push_auth_mark_for_asset_as_admin(0, slot, mark);
        last_push = slot;
    }
    GapWorld {
        env,
        actors,
        mark,
        last_push,
    }
}

/// One resolved crank of `portfolio`, with a discovery hint for asset 0, signed by the
/// owner when `signer` is given. Returns what it paid or the program's error. A call that
/// succeeds must change something.
fn send_crank(
    env: &mut V16CuEnv,
    owner: Pubkey,
    signer: Option<&Keypair>,
    portfolio: Pubkey,
    label: &str,
) -> Result<u128, String> {
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
    let now_slot = env.svm.get_sysvar::<solana_sdk::clock::Clock>().slot;
    let before = (
        env.svm.get_account(&env.market).unwrap(),
        env.svm.get_account(&portfolio).unwrap(),
        env.svm.get_account(&env.vault).unwrap(),
    );
    let signers: Vec<&Keypair> = signer.into_iter().collect();
    let cu = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new_readonly(owner, signer.is_some()),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &signers,
    )?;
    assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
    let paid = env.token_amount(destination) as u128;
    let after = (
        env.svm.get_account(&env.market).unwrap(),
        env.svm.get_account(&portfolio).unwrap(),
        env.svm.get_account(&env.vault).unwrap(),
    );
    assert!(
        before != after || paid != 0,
        "{label}: a successful call made no progress"
    );
    Ok(paid)
}

/// One unsigned resolved crank. Returns what it paid, or `None` when the program reports
/// that the call cannot progress yet.
fn crank(env: &mut V16CuEnv, owner: Pubkey, portfolio: Pubkey, label: &str) -> Option<u128> {
    match send_crank(env, owner, None, portfolio, label) {
        Ok(paid) => Some(paid),
        Err(error) if is_engine_non_progress_error(&error) => None,
        Err(error) => panic!("{label}: unexpected resolved crank error: {error}"),
    }
}

/// Drains the market with unsigned cranks in `order`, one call per account per round, and
/// adds what each account is paid to `paid`. A leg keeps its loss weight until the other
/// side has settled, so an account can be told to come back later. Rounding can leave an
/// atom in a backing bucket; the receipt that is owed it collects it once the bucket has
/// lapsed, so the clock is moved past the resolved backing deadline once when nothing else
/// can progress. Returns what is left in the vault.
fn drain_unsigned(
    env: &mut V16CuEnv,
    actors: &[(Keypair, Pubkey)],
    order: &[usize],
    paid: &mut [u128],
    label: &str,
) -> u64 {
    let mut lapsed = false;
    for _round in 0..64 {
        if actors
            .iter()
            .all(|(_, portfolio)| resolved_portfolio_is_terminal(env, *portfolio))
        {
            return env.token_amount(env.vault);
        }
        let mut progressed = false;
        for &actor in order {
            let (owner, portfolio) = (&actors[actor].0, actors[actor].1);
            if resolved_portfolio_is_terminal(env, portfolio) {
                continue;
            }
            if let Some(amount) = crank(env, owner.pubkey(), portfolio, label) {
                paid[actor] += amount;
                progressed = true;
            }
        }
        if !progressed {
            assert!(
                !lapsed,
                "{label}: nothing progresses after every bucket has lapsed"
            );
            let deadline = resolved_backing_deadline(&env.market_state().1);
            env.svm.warp_to_slot(deadline + 1);
            lapsed = true;
        }
    }
    panic!("{label}: the market did not drain in 64 rounds");
}

/// Resolves once the oracle is stale and, after the owner window, drains the market in
/// `order`. Returns what [T, W, X] were paid and what is left in the vault.
fn resolve_and_drain(order: [usize; 3]) -> ([u128; 3], u64) {
    let label = "bankrupt shortfall, close order";
    let GapWorld {
        mut env,
        actors,
        last_push,
        ..
    } = gap_world();
    let resolved = last_push + 111;
    let cu = env.resolve_stale_permissionless_with_cu(resolved);
    assert_cu_within(label, cu, CRANK_CU_LIMIT);
    env.svm.warp_to_slot(resolved + OWNER_WINDOW);
    let mut paid = [0u128; 3];
    let left = drain_unsigned(&mut env, &actors, &order, &mut paid, label);
    (paid, left)
}

/// In every close order T is paid nothing and W and X share what T could not pay by the
/// size of their positions. X, whose position is worth 200,000, keeps its deposit.
#[test]
fn v16_program_bankrupt_shortfall_is_shared_by_weight_in_every_close_order() {
    let mark = gap_world().mark;
    let fall = u128::from(PRICE - mark);
    // Gains in atoms: W is short 1,000 units, X a fifth of a unit.
    let w_gain = W_UNITS * fall;
    let x_gain = X_FIFTHS * fall / 5;
    let shortfall = w_gain + x_gain - T_DEPOSIT;
    assert!(
        shortfall > 39_000_000,
        "T reaches resolution deeply bankrupt"
    );
    // Loss weights are 1,000 and 0.2 units: 5,000 and 1 fifths.
    let x_share = shortfall / (5 * W_UNITS + X_FIFTHS);
    let w_share = shortfall - x_share;
    let fair = [
        0,
        W_DEPOSIT + w_gain - w_share,
        X_DEPOSIT + x_gain - x_share,
    ];

    for order in [
        [T, W, X],
        [W, T, X],
        [X, T, W],
        [W, X, T],
        [X, W, T],
        [T, X, W],
    ] {
        let (paid, left) = resolve_and_drain(order);
        let case = format!("close order {order:?}: paid {paid:?}, {left} left in the vault");
        assert_eq!(paid[T], 0, "{case}");
        // To the atoms the shares and the payout rate round by.
        for who in [W, X] {
            assert!(
                paid[who].abs_diff(fair[who]) <= 2,
                "{case}: account {who} is owed {}",
                fair[who]
            );
        }
        assert!(paid[X] > X_DEPOSIT, "{case}: X keeps its deposit");
        assert_eq!(
            paid.iter().sum::<u128>() + u128::from(left),
            T_DEPOSIT + W_DEPOSIT + X_DEPOSIT,
            "{case}: conservation"
        );
        assert!(left <= 3, "{case}: only rounding dust is left");
    }
}

fn is_expected_signer_error(error: &str) -> bool {
    error.contains("Custom(6)") || error.contains("custom program error: 0x6")
}

/// The owner cranks its own account until the program reports that it can only wait. The
/// first call settles something and nothing is paid on the way.
fn settle_until_it_waits(env: &mut V16CuEnv, actor: &(Keypair, Pubkey), label: &str) {
    for calls in 0..8 {
        match send_crank(env, actor.0.pubkey(), Some(&actor.0), actor.1, label) {
            Ok(paid) => assert_eq!(paid, 0, "{label}: a settlement that waits pays nothing"),
            Err(error) if is_engine_non_progress_error(&error) => {
                assert_ne!(calls, 0, "{label}: the first call settles the account");
                return;
            }
            Err(error) => panic!("{label}: unexpected resolved crank error: {error}"),
        }
    }
    panic!("{label}: the account neither leaves nor waits after eight calls");
}

/// The owner's own crank can only wait: the program refuses it and changes nothing.
fn assert_waits(env: &mut V16CuEnv, actor: &(Keypair, Pubkey), label: &str) {
    let error = send_crank(env, actor.0.pubkey(), Some(&actor.0), actor.1, label)
        .expect_err("a crank that can only wait");
    assert!(is_engine_non_progress_error(&error), "{label}: {error}");
}

/// Until `force_close_delay_slots` have passed since resolution only its owner can crank an
/// account, so inside that window a position leaves only once the owners on the other side
/// of its asset have settled. A solvent loser waits for the winners opposite it, and they
/// wait for the bankrupt account. An owner that stays silent holds them to the end of the
/// window and no longer: then anyone cranks its account and the market drains, with the
/// payouts of every other order.
#[test]
fn v16_program_silent_owner_holds_the_other_side_only_for_the_owner_window() {
    const WINDOW: u64 = 50;
    let label = "silent owner, owner window";
    let GapWorld {
        mut env,
        actors,
        mark,
        last_push,
    } = gap_world_with(WINDOW, true);
    let fall = u128::from(PRICE - mark);
    let total = T_DEPOSIT + W_DEPOSIT + X_DEPOSIT + L_DEPOSIT;
    let resolved = last_push + 111;
    let cu = env.resolve_stale_permissionless_with_cu(resolved);
    assert_cu_within(label, cu, CRANK_CU_LIMIT);
    env.svm.warp_to_slot(resolved + 1);
    assert!(
        resolved + WINDOW < resolved_backing_deadline(&env.market_state().1),
        "{label}: no backing bucket lapses inside the window"
    );
    let mut paid = [0u128; 4];

    // Inside the window nobody but its owner can crank an account.
    for (owner, portfolio) in &actors {
        let error = send_crank(&mut env, owner.pubkey(), None, *portfolio, label)
            .expect_err("an unsigned crank inside the owner window");
        assert!(is_expected_signer_error(&error), "{label}: {error}");
    }

    // L pays its loss at once and keeps its position: W and X, on the other side, have not
    // settled, so a bankruptcy could still be booked onto the long side.
    settle_until_it_waits(&mut env, &actors[L], label);
    let l = env.portfolio_state(actors[L].1);
    assert_eq!(
        l.capital.get(),
        L_DEPOSIT - fall,
        "{label}: L has paid its loss"
    );
    assert_eq!(l.pnl.get(), 0, "{label}: L owes nothing");
    // X settles. W stays silent, and L still waits.
    settle_until_it_waits(&mut env, &actors[X], label);
    assert_waits(&mut env, &actors[L], label);
    // W settles: nothing can be booked onto the long side any more, so L leaves with its
    // capital, inside the window and before T has been touched.
    settle_until_it_waits(&mut env, &actors[W], label);
    for _call in 0..8 {
        if resolved_portfolio_is_terminal(&env, actors[L].1) {
            break;
        }
        paid[L] += send_crank(
            &mut env,
            actors[L].0.pubkey(),
            Some(&actors[L].0),
            actors[L].1,
            label,
        )
        .expect("L leaves once the short side has settled");
    }
    assert!(
        resolved_portfolio_is_terminal(&env, actors[L].1),
        "{label}: L has left"
    );
    assert_eq!(paid[L], L_DEPOSIT - fall, "{label}: L is paid its capital");

    // W and X wait for T, whose owner stays silent, to the last slot of the window.
    env.svm.warp_to_slot(resolved + WINDOW - 1);
    for who in [W, X] {
        assert_waits(&mut env, &actors[who], label);
    }
    let error = send_crank(&mut env, actors[T].0.pubkey(), None, actors[T].1, label)
        .expect_err("an unsigned crank in the last slot of the owner window");
    assert!(is_expected_signer_error(&error), "{label}: {error}");
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        total - paid[L],
        "{label}: nothing else has left the vault"
    );

    // The window has passed. Anyone books T's loss, and the market drains unsigned.
    env.svm.warp_to_slot(resolved + WINDOW);
    let left = drain_unsigned(&mut env, &actors, &[W, X, T], &mut paid, label);
    let case = format!("{label}: paid {paid:?}, {left} left in the vault");
    // T's shortfall falls on the short side by weight: W is short 1,001 units, X a fifth.
    let x_gain = X_FIFTHS * fall / 5;
    let shortfall = W_UNITS * fall + x_gain - T_DEPOSIT;
    let x_share = shortfall / (5 * (W_UNITS + 1) + X_FIFTHS);
    let w_share = shortfall - x_share;
    let fair = [
        0,
        W_DEPOSIT + (W_UNITS + 1) * fall - w_share,
        X_DEPOSIT + x_gain - x_share,
        L_DEPOSIT - fall,
    ];
    assert_eq!(paid[T], 0, "{case}");
    assert_eq!(paid[L], fair[L], "{case}");
    for who in [W, X] {
        assert!(
            paid[who].abs_diff(fair[who]) <= 2,
            "{case}: account {who} is owed {}",
            fair[who]
        );
    }
    assert_eq!(
        paid.iter().sum::<u128>() + u128::from(left),
        total,
        "{case}: conservation"
    );
    assert!(left <= 3, "{case}: only rounding dust is left");
}
