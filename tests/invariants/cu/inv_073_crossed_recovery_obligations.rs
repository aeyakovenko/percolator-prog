//! INV-073: Recovery obligations on two assets, crossed, do not lock the resolved market.
//!
//! A leg forfeited on a shut-down asset stays attached with a zero basis and its loss weight
//! while the other side of that asset still holds a real position. Two accounts that each
//! forfeit one leg and keep a real position on the other asset hold each other's obligation
//! up. Before the engine fix, resolved close kept every leg of such an account attached, so
//! neither account could ever close, and because an attached leg blocks every positive payout,
//! a winner on an unrelated asset was never paid either.
//!
//! Public instructions only. Nobody is insolvent and the two shut-down assets never move.

use super::*;

const PRICE: u64 = 1_000_000;
const DEPOSIT: u128 = 200_000;
/// R is long one unit of asset 0 against S; asset 0 rises 1%.
const GAIN: u128 = 10_000;

struct Actor {
    owner: Keypair,
    portfolio: Pubkey,
}

struct CrossedWorld {
    env: V16CuEnv,
    p: Actor,
    q: Actor,
    r: Actor,
    s: Actor,
    /// The two shut-down assets.
    assets: [u16; 2],
}

fn actor(env: &mut V16CuEnv) -> Actor {
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, DEPOSIT);
    Actor { owner, portfolio }
}

/// Who shuts the two assets down.
#[derive(Clone, Copy, PartialEq)]
enum Shutdown {
    /// The market authority shuts down two of its own assets.
    Admin,
    /// An asset creator without any market role activates two assets for a fee and
    /// shuts them down again; P and Q are its own accounts.
    Creator,
}

/// On the first shut-down asset P is long one unit against Q; on the second Q is long one
/// unit against P. On asset 0 R is long one unit against S, and asset 0 rises 1%. The two
/// assets are then shut down; `forfeits` lists who gives up which of them (`true` is P).
fn crossed_world(shutdown: Shutdown, forfeits: &[(bool, usize)]) -> CrossedWorld {
    let label = "crossed Recovery obligations";
    let mut params = production_risk_params();
    // No funding: R's gain is exactly the price move.
    params.max_abs_funding_e9_per_slot = 0;
    let creator = Keypair::new();
    let (mut env, assets) = match shutdown {
        Shutdown::Admin => {
            params.max_portfolio_assets = 3;
            let mut env = V16CuEnv::new_with_init_params(params);
            env.configure_permissionless_resolve_with_cu(100, 5);
            for asset in 0..3 {
                env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
            }
            (env, [1u16, 2])
        }
        Shutdown::Creator => {
            params.max_portfolio_assets = 2;
            let mut env = V16CuEnv::new_with_init_params(params);
            env.configure_permissionless_resolve_with_cu(100, 5);
            env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
            env.update_market_init_fee_policy_with_cu(1);
            let key = creator.pubkey();
            for (asset, slot) in [(2u16, 1u64), (3, 2)] {
                env.svm.warp_to_slot(slot);
                env.activate_permissionless_asset_with_fee(
                    &creator, asset, slot, PRICE, key, key, key, key, 1,
                );
                env.configure_auth_mark_for_asset_with_authority(asset, &creator, slot, PRICE);
            }
            (env, [2u16, 3])
        }
    };
    let p = actor(&mut env);
    let q = actor(&mut env);
    let r = actor(&mut env);
    let s = actor(&mut env);
    let unit = POS_SCALE as i128;
    env.trade_asset_with_cu(assets[0], &p.owner, p.portfolio, &q.owner, q.portfolio, unit, PRICE, 0);
    env.trade_asset_with_cu(assets[1], &q.owner, q.portfolio, &p.owner, p.portfolio, unit, PRICE, 0);
    env.trade_asset_with_cu(0, &r.owner, r.portfolio, &s.owner, s.portfolio, unit, PRICE, 0);

    env.svm.warp_to_slot(20);
    env.push_auth_mark_for_asset_as_admin(0, 20, PRICE + PRICE / 100);
    for portfolio in [r.portfolio, s.portfolio] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations(0),
            },
        );
    }

    for asset in assets {
        match shutdown {
            Shutdown::Admin => {
                env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, asset, 20, 0);
            }
            Shutdown::Creator => {
                env.try_shutdown_asset_with_authority(&creator, asset, 20)
                    .expect("a creator shuts down its own asset");
            }
        }
        assert_eq!(
            env.market_state().1.assets[asset as usize].lifecycle,
            AssetLifecycleV16::Recovery
        );
    }
    for &(is_p, which) in forfeits {
        let asset = assets[which];
        let who = if is_p { &p } else { &q };
        let weight = active_leg_for_asset(&env.portfolio_state(who.portfolio), asset as usize).loss_weight;
        let cu = env.forfeit_recovery_leg_with_cu(&who.owner, who.portfolio, asset, u128::MAX);
        assert_cu_within(label, cu, CRANK_CU_LIMIT);
        let retained = active_leg_for_asset(&env.portfolio_state(who.portfolio), asset as usize);
        assert_eq!(retained.basis_pos_q, 0, "{label}: the forfeited leg keeps no exposure");
        assert_eq!(retained.loss_weight, weight, "{label}: and keeps its loss weight");
    }
    CrossedWorld { env, p, q, r, s, assets }
}

fn resolve(world: &mut CrossedWorld) {
    let cu = world.env.resolve_stale_permissionless_with_cu(131);
    assert_cu_within("crossed Recovery obligations", cu, CRANK_CU_LIMIT);
    world.env.svm.warp_to_slot(136);
}

/// P gives up the first asset and Q the second. Each forfeited leg waits for the other
/// account's real position; each account still holds a real position. Everyone closes and
/// is paid exactly, in both keeper orders.
fn crossed_obligations_drain(shutdown: Shutdown) {
    for reversed in [false, true] {
        let label = if reversed {
            "crossed Recovery obligations, winner last"
        } else {
            "crossed Recovery obligations, winner first"
        };
        let mut world = crossed_world(shutdown, &[(true, 0), (false, 1)]);
        resolve(&mut world);
        let CrossedWorld { mut env, p, q, r, s, assets } = world;
        let mut cohort = vec![
            (&r.owner, r.portfolio),
            (&s.owner, s.portfolio),
            (&p.owner, p.portfolio),
            (&q.owner, q.portfolio),
        ];
        if reversed {
            cohort.reverse();
        }
        let mut payouts = drain_resolved_cohort(&mut env, &cohort, label);
        if reversed {
            payouts.reverse();
        }
        assert_eq!(
            payouts,
            vec![DEPOSIT + GAIN, DEPOSIT - GAIN, DEPOSIT, DEPOSIT],
            "{label}: paid as [R, S, P, Q]"
        );
        let terminal = env.market_state().1;
        assert_eq!(terminal.c_tot, 0);
        assert_eq!(terminal.pnl_pos_tot, 0);
        for asset in assets {
            let state = terminal.assets[asset as usize];
            assert_eq!(state.stored_pos_count_long + state.stored_pos_count_short, 0);
            assert_eq!(state.pending_obligation_count_long + state.pending_obligation_count_short, 0);
            assert_eq!(state.loss_weight_sum_long + state.loss_weight_sum_short, 0);
        }
        // Only the creator's two activation fees are left, where there are any.
        let fees = if shutdown == Shutdown::Creator { 2 } else { 0 };
        assert_eq!(env.token_amount(env.vault), fees, "{label}: vault");
    }
}

#[test]
fn v16_program_crossed_recovery_obligations_do_not_lock_the_resolved_market() {
    crossed_obligations_drain(Shutdown::Admin);
}

/// The same through an account without any market role: it activates two assets for the
/// listing fee, crosses two of its own accounts on them, shuts both down and forfeits one
/// leg on each. R and S, who never touched those assets, are paid.
#[test]
fn v16_program_asset_creator_cannot_lock_the_resolved_market_with_crossed_forfeits() {
    crossed_obligations_drain(Shutdown::Creator);
}

/// Control, the same on both engines: only P gives up a leg. Its obligation waits for Q's
/// real position on that asset and Q is free to close.
#[test]
fn v16_program_one_recovery_obligation_waits_and_the_resolved_market_drains() {
    let label = "one Recovery obligation";
    let mut world = crossed_world(Shutdown::Admin, &[(true, 0)]);
    resolve(&mut world);
    let CrossedWorld { mut env, p, q, r, s, .. } = world;
    let payouts = drain_resolved_cohort(
        &mut env,
        &[
            (&p.owner, p.portfolio),
            (&q.owner, q.portfolio),
            (&r.owner, r.portfolio),
            (&s.owner, s.portfolio),
        ],
        label,
    );
    assert_eq!(payouts, vec![DEPOSIT, DEPOSIT, DEPOSIT + GAIN, DEPOSIT - GAIN], "{label}");
    assert_eq!(env.token_amount(env.vault), 0);
}
