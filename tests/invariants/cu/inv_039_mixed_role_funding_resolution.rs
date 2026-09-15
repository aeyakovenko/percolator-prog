//! INV-039/041/067: same-owner pending-creditor and funding-debtor roles
//! resolve in an order-independent way.
//!
//! Existing mixed-role resolution coverage disables funding, while funded
//! pending-debt coverage keeps creditors and debtors in separate portfolios.
//! This public LiteSVM test composes the two: actor 0 retains a zero-basis,
//! nonzero-loss-weight creditor claim on asset 1 while also carrying a
//! debtor leg on asset 2 after the asset has nonzero funding. The original
//! close settles that debtor leg before resolution and compares terminal orders.
//! The insurance child moves the debtor mark after the creditor close, requires
//! unsettled K/F snapshots at resolution, and checks an input-derived owner book.
//! These finite histories leave rows 419/435 and generic INV-086 equivalence open.

use super::*;

#[path = "inv_039_mixed_role_funding_insurance.rs"]
mod funding_insurance;

const ENTRY: i128 = 1_000_000;
const TARGET_MOVE: i128 = 200_000;
const SETTLE_SLOT: u64 = 6;
const SETTLE_MOVE: i128 = 60_000;
const FUNDING_RATE_E9: u64 = 1_000;
const CREDITOR_LOTS_Q: i128 = 7 * POS_SCALE as i128 / 2;
const DEBTOR_LOTS_Q: i128 = 2 * POS_SCALE as i128;

fn funding_from_inputs(sign: i128) -> i128 {
    // Each AuthMark target is committed before its six capped accrual steps.
    (1..=SETTLE_SLOT)
        .map(|slot| {
            let price = ENTRY + sign * ENTRY / 100 * i128::from(slot);
            (sign * i128::from(FUNDING_RATE_E9) * price).div_euclid(1_000_000_000)
        })
        .sum()
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    paid: [u128; 5],
    vault: u128,
}

fn setup(reverse: bool) -> AttributionWorld {
    setup_with_terms(
        reverse,
        CREDITOR_LOTS_Q,
        DEBTOR_LOTS_Q,
        DEPOSITS,
        500,
        false,
    )
}

fn setup_with_terms(
    reverse: bool,
    creditor_lots_q: i128,
    debtor_lots_q: i128,
    deposits: [u128; 5],
    margin_bps: u64,
    deferred_debtor: bool,
) -> AttributionWorld {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: ENTRY as u64,
            maintenance_margin_bps: margin_bps,
            initial_margin_bps: margin_bps,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: FUNDING_RATE_E9,
            liquidation_fee_bps: 0,
            max_price_move_bps_per_slot: 100,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..production_risk_params()
        },
        deposits,
    );
    let sign = if reverse { -1 } else { 1 };
    let creditor_q = creditor_lots_q * sign;
    let debtor_q = debtor_lots_q * sign;
    world.quantities = [creditor_q, -creditor_q, debtor_q, -debtor_q];

    world.env.trade_asset_with_cu(
        1,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        &world.actors[1].owner,
        world.actors[1].portfolio,
        creditor_q,
        ENTRY as u64,
        0,
    );
    world.env.trade_asset_with_cu(
        2,
        &world.actors[2].owner,
        world.actors[2].portfolio,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        debtor_q,
        ENTRY as u64,
        0,
    );
    let admin = world.env.admin.insecure_clone();
    send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &world.env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();

    for asset in [1, 2] {
        if deferred_debtor && asset == 2 {
            continue;
        }
        world
            .env
            .push_auth_mark_for_asset_as_admin(asset, 1, (ENTRY + sign * TARGET_MOVE) as u64);
    }
    for slot in 1..=SETTLE_SLOT {
        world.env.svm.warp_to_slot(slot);
        world.env.crank(
            world.actors[4].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations_for_assets(&[1, 2]),
            },
        );
    }
    let settle_price = ENTRY + sign * SETTLE_MOVE;
    let funding = funding_from_inputs(sign);
    for asset in [1usize, 2] {
        if deferred_debtor && asset == 2 {
            assert_eq!(
                world.env.market_state().1.assets[asset].effective_price,
                ENTRY as u64
            );
            continue;
        }
        assert_eq!(
            world.env.market_state().1.assets[asset].effective_price as i128,
            settle_price
        );
        assert_eq!(
            (
                world.env.market_state().1.assets[asset].f_long_num,
                world.env.market_state().1.assets[asset].f_short_num
            ),
            (-funding * ADL_ONE as i128, funding * ADL_ONE as i128),
            "asset {asset}: exact input-derived funding before the mixed close"
        );
    }

    world.env.trade_asset_with_cu(
        1,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        &world.actors[1].owner,
        world.actors[1].portfolio,
        -creditor_q,
        settle_price as u64,
        0,
    );

    if deferred_debtor {
        world.env.push_auth_mark_for_asset_as_admin(
            2,
            SETTLE_SLOT + 1,
            (ENTRY + sign * TARGET_MOVE) as u64,
        );
        for slot in SETTLE_SLOT + 1..=2 * SETTLE_SLOT {
            world.env.svm.warp_to_slot(slot);
            world.env.crank(
                world.actors[4].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(2),
                },
            );
        }
    }
    let funded = world.env.market_state().1.assets[2];
    assert_eq!(funded.effective_price as i128, settle_price);
    assert_eq!(
        (funded.f_long_num, funded.f_short_num),
        (-funding * ADL_ONE as i128, funding * ADL_ONE as i128)
    );

    let actor0 = world.env.portfolio_state(world.actors[0].portfolio);
    let legs: Vec<_> = actor0
        .legs
        .iter()
        .map(|leg| leg.try_to_runtime().unwrap())
        .filter(|leg| leg.active)
        .collect();
    let creditor = legs
        .iter()
        .find(|leg| leg.asset_index == 1)
        .expect("actor 0 retains the creditor pending leg");
    assert_eq!(creditor.basis_pos_q, 0);
    assert_eq!(creditor.loss_weight, creditor_lots_q as u128);
    let debtor = legs
        .iter()
        .find(|leg| leg.asset_index == 2)
        .expect("actor 0 retains the funding-bearing debtor leg");
    assert_eq!(debtor.basis_pos_q, -debtor_q);
    assert_eq!(debtor.loss_weight, debtor_lots_q as u128);
    if deferred_debtor {
        assert_eq!(
            (debtor.k_snap, debtor.f_snap),
            (0, 0),
            "cross-asset debt must still be unsettled at resolution"
        );
        assert_eq!(actor0.capital.get(), deposits[0]);
    }
    assert_eq!(legs.len(), 2);
    world
}

fn close_all(world: AttributionWorld, order: [usize; 5], peak: &mut u64) -> Outcome {
    close_all_checked(world, order, peak, 0, |_, _| {})
}

fn close_all_checked(
    mut world: AttributionWorld,
    order: [usize; 5],
    peak: &mut u64,
    remaining_insurance: u128,
    check: impl Fn(&AttributionWorld, [bool; 5]),
) -> Outcome {
    let before = world.frame();
    *peak = (*peak).max(world.env.resolve());
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }

    let payout_slot = world.env.market_state().1.current_slot + 6;
    world.env.svm.warp_to_slot(payout_slot);
    let mut deleted = [false; 5];
    check(&world, deleted);
    let mut waiting_rejections = 0usize;
    for _ in 0..20 {
        let mut progressed = false;
        for actor in order {
            if deleted[actor] {
                continue;
            }
            if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                let before = world.frame();
                match world.payout(actor, false) {
                    Ok(cu) => {
                        assert_cu_within("mixed-role funded resolved close", cu, CUSTODY_CU_LIMIT);
                        *peak = (*peak).max(cu);
                        assert_ne!(world.frame(), before);
                        progressed = true;
                    }
                    Err(error) => {
                        assert!(is_engine_non_progress_error(&error), "{error}");
                        assert_eq!(world.frame(), before);
                        waiting_rejections += 1;
                    }
                }
                check(&world, deleted);
            }
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                if resolved_receipt(&world.env.portfolio_state(world.actors[actor].portfolio))
                    .present
                {
                    let before = world.frame();
                    *peak = (*peak).max(world.payout(actor, true).expect("paid receipt retry"));
                    assert_eq!(world.frame(), before);
                }
                let a = &world.actors[actor];
                let before = world.frame();
                let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                assert_cu_within("mixed-role funded portfolio close", cu, CUSTODY_CU_LIMIT);
                *peak = (*peak).max(cu);
                deleted[actor] = true;
                for (key, account) in before {
                    if ![world.env.market, a.portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                progressed = true;
                check(&world, deleted);
            }
        }
        if deleted.iter().all(|value| *value) {
            break;
        }
        assert!(
            progressed,
            "nonterminal funded resolved state stopped progressing"
        );
    }
    assert!(deleted.iter().all(|value| *value));
    assert!(
        waiting_rejections > 0,
        "mixed-role funding history must exercise waiting debt rollbacks"
    );
    let group = world.env.market_state().1;
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.insurance,
            group.materialized_portfolio_count
        ),
        (0, 0, remaining_insurance, 0)
    );
    let paid = std::array::from_fn(|i| world.env.token_amount(world.actors[i].token) as u128);
    let vault = world.env.token_amount(world.env.vault) as u128;
    assert_eq!(vault, group.vault);
    assert_eq!(
        vault + paid.iter().sum::<u128>(),
        world.deposits.iter().sum::<u128>()
    );
    Outcome { paid, vault }
}

#[test]
fn v16_program_mixed_roles_preserve_funding_attribution_through_resolution() {
    let mut worlds = 0usize;
    let mut peak = 0u64;
    for reverse in [false, true] {
        let first = close_all(setup(reverse), [0, 2, 1, 3, 4], &mut peak);
        let second = close_all(setup(reverse), [2, 0, 3, 1, 4], &mut peak);
        assert_eq!(
            first, second,
            "terminal owner attribution must not depend on mixed-role funding close order"
        );
        worlds += 2;
    }
    assert_eq!(worlds, 4);
    println!(
        "INV-039 mixed funding roles: {worlds} public worlds, same-owner pending creditor/debtor, peak {peak} CU"
    );
}
