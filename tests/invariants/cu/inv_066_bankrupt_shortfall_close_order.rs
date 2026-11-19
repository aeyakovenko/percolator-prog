//! INV-066: a bankrupt account's shortfall does not depend on who closes first.
//!
//! The mark moves by more than an account's margin between its last crank and resolution,
//! so the account reaches resolution owing more than it has. What it cannot pay is shared
//! by the positions on the other side of its asset, by loss weight, when it is booked.
//! Before the engine fix a winner that closed first detached its leg with its whole gain,
//! and its share fell on the winners still attached, their capital included: here a
//! bystander with a 0.2-unit position lost four fifths of its deposit.
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
    let mut params = production_risk_params();
    // No funding: gains and losses are exactly the price move.
    params.max_abs_funding_e9_per_slot = 0;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    let mut actors = Vec::new();
    for deposit in [T_DEPOSIT, W_DEPOSIT, X_DEPOSIT] {
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

/// One resolved crank of `portfolio`, with a discovery hint for asset 0. Returns what it
/// paid, or `None` when the program reports that the call cannot progress yet. A call that
/// succeeds must change something.
fn crank(env: &mut V16CuEnv, owner: Pubkey, portfolio: Pubkey, label: &str) -> Option<u128> {
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
    let result = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot,
            observations: crank_observations(0),
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
    );
    match result {
        Ok(cu) => {
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
            Some(paid)
        }
        Err(error) if is_engine_non_progress_error(&error) => None,
        Err(error) => panic!("{label}: unexpected resolved crank error: {error}"),
    }
}

/// Resolves once the oracle is stale and drains the market in `order`, one call per
/// account per round. A leg keeps its loss weight until the other side has settled, so an
/// account can be told to come back later. Rounding can leave an atom in a backing bucket;
/// the receipt that is owed it collects it once the bucket has lapsed, so the clock is
/// moved past the resolved backing deadline once when nothing else can progress. Returns
/// what [T, W, X] were paid and what is left in the vault.
fn resolve_and_drain(order: [usize; 3]) -> ([u128; 3], u64) {
    let label = "bankrupt shortfall, close order";
    let GapWorld {
        mut env,
        actors,
        last_push,
        ..
    } = gap_world();
    let cu = env.resolve_stale_permissionless_with_cu(last_push + 111);
    assert_cu_within(label, cu, CRANK_CU_LIMIT);
    env.svm.warp_to_slot(last_push + 116);
    let mut paid = [0u128; 3];
    let mut lapsed = false;
    for _round in 0..64 {
        if actors
            .iter()
            .all(|(_, portfolio)| resolved_portfolio_is_terminal(&env, *portfolio))
        {
            let left = env.token_amount(env.vault);
            return (paid, left);
        }
        let mut progressed = false;
        for &actor in &order {
            let (owner, portfolio) = (&actors[actor].0, actors[actor].1);
            if resolved_portfolio_is_terminal(&env, portfolio) {
                continue;
            }
            if let Some(amount) = crank(&mut env, owner.pubkey(), portfolio, label) {
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
