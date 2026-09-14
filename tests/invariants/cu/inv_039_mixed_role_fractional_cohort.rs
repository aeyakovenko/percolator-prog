//! INV-039/024: mixed-role ownership through a fractional B cohort.
//! A holder that owns a fractional residual claim on one asset also owes an
//! exact adverse debt on another asset. The final route is compared with the
//! same public fractional-cohort baseline after normalizing the cross-asset
//! debt, so aggregate conservation cannot hide wrong-owner attribution.

use super::*;

const EXTRA_Q: i128 = 1_000;
const EXTRA_MOVE: u128 = 37_000;
const EXTRA_DEBT: u128 = EXTRA_Q as u128 * EXTRA_MOVE / POS_SCALE;

fn setup_with_extra_debt(input: Inputs, reverse: bool, peak: &mut u64) -> AttributionWorld {
    assert_eq!(EXTRA_Q as u128 * EXTRA_MOVE % POS_SCALE, 0);
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
        input.deposits(),
    );
    let sign = if reverse { -1 } else { 1 };
    world.quantities = [
        input.weights[0] as i128 * sign,
        -(input.weights[0] as i128) * sign,
        input.weights[1] as i128 * sign,
        -(input.weights[1] as i128) * sign,
    ];

    for pair in 0..2 {
        *peak = (*peak).max(world.env.trade_asset_with_cu(
            1,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            world.quantities[2 * pair],
            1_000_000,
            0,
        ));
    }

    *peak = (*peak).max(world.env.trade_asset_with_cu(
        2,
        &world.actors[4].owner,
        world.actors[4].portfolio,
        &world.actors[0].owner,
        world.actors[0].portfolio,
        EXTRA_Q * sign,
        1_000_000,
        0,
    ));

    for slot in 1..=5 {
        world.env.svm.warp_to_slot(slot);
        world.env.push_auth_mark_for_asset_as_admin(
            1,
            slot,
            (1_000_000 + 40_000 * slot as i128 * sign) as u64,
        );
        *peak = (*peak).max(world.env.crank(
            world.actors[4].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations_for_assets(&[1, 2]),
            },
        ));
    }

    let mark = (1_000_000 + Inputs::MOVEMENT as i128 * sign) as u64;
    assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
    for pair in 0..2 {
        *peak = (*peak).max(world.env.trade_asset_with_cu(
            1,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            -world.quantities[2 * pair],
            mark,
            0,
        ));
    }

    world.env.svm.warp_to_slot(6);
    world.env.push_auth_mark_for_asset_as_admin(
        2,
        6,
        (1_000_000 + EXTRA_MOVE as i128 * sign) as u64,
    );
    *peak = (*peak).max(world.env.crank(
        world.actors[4].portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations_for_assets(&[1, 2]),
        },
    ));

    let actor0 = world.env.portfolio_state(world.actors[0].portfolio);
    let mut has_fractional_claim = false;
    let mut has_cross_asset_debt = false;
    for leg in actor0
        .legs
        .iter()
        .map(|leg| leg.try_to_runtime().unwrap())
        .filter(|leg| leg.active)
    {
        if leg.asset_index == 1 {
            assert_eq!(leg.basis_pos_q, 0);
            assert_eq!(leg.loss_weight, input.weights[0]);
            assert_ne!(leg.loss_weight % POS_SCALE, 0);
            has_fractional_claim = true;
        }
        if leg.asset_index == 2 {
            assert_eq!(leg.basis_pos_q.unsigned_abs(), EXTRA_Q as u128);
            has_cross_asset_debt = true;
        }
    }
    assert!(
        has_fractional_claim && has_cross_asset_debt,
        "actor0 must simultaneously carry a fractional B claim and an adverse debt"
    );
    world
}

fn close_all(mut world: AttributionWorld, order: [usize; 5], peak: &mut u64) -> ([u128; 5], u128) {
    world.env.svm.warp_to_slot(6);
    let before = world.frame();
    *peak = (*peak).max(world.env.resolve());
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    world.env.svm.warp_to_slot(12);

    let mut deleted = [false; 5];
    let mut waiting_rejections = 0usize;
    let mut receipt_retries = 0usize;
    for _ in 0..16 {
        let mut progressed = false;
        for actor in order {
            if deleted[actor] {
                continue;
            }
            if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                let before = world.frame();
                match world.payout(actor, false) {
                    Ok(cu) => {
                        assert_cu_within("mixed fractional resolved close", cu, CUSTODY_CU_LIMIT);
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
            }
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                if resolved_receipt(&world.env.portfolio_state(world.actors[actor].portfolio))
                    .present
                {
                    let before = world.frame();
                    *peak = (*peak).max(world.payout(actor, true).expect("paid receipt retry"));
                    assert_eq!(world.frame(), before);
                    receipt_retries += 1;
                }
                let a = &world.actors[actor];
                let before = world.frame();
                let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                assert_cu_within("mixed fractional portfolio close", cu, CUSTODY_CU_LIMIT);
                *peak = (*peak).max(cu);
                deleted[actor] = true;
                for (key, account) in before {
                    if ![world.env.market, a.portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                progressed = true;
            }
        }
        if deleted.iter().all(|x| *x) {
            break;
        }
        assert!(progressed, "nonterminal resolved state stopped progressing");
    }
    assert!(deleted.iter().all(|x| *x));
    let _ = (waiting_rejections, receipt_retries);

    let group = world.env.market_state().1;
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.insurance,
            group.materialized_portfolio_count
        ),
        (0, 0, 0, 0)
    );
    let paid = std::array::from_fn(|i| world.env.token_amount(world.actors[i].token) as u128);
    let wallets = paid.iter().sum::<u128>();
    let vault = world.env.token_amount(world.env.vault) as u128;
    assert_eq!(vault, group.vault);
    assert_eq!(vault + wallets, world.deposits.iter().sum::<u128>());
    (paid, group.vault)
}

fn baseline(
    input: Inputs,
    reverse: bool,
    live_booking: bool,
    order: [usize; 5],
    peak: &mut u64,
) -> ([u128; 5], u128) {
    let mut world = setup(input, reverse, false, peak);
    if live_booking {
        *peak = (*peak).max(world.env.crank(
            world.actors[1].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(1),
            },
        ));
    }
    close_all(world, order, peak)
}

#[test]
fn v16_program_mixed_role_fractional_b_cohort_preserves_normalized_owner_attribution() {
    let input = Inputs {
        weights: [450_003, 600_004],
        residual: 7,
    };
    assert!(input.weights.iter().all(|q| *q % POS_SCALE != 0));
    assert!(input.booking_remainder() > 0);
    assert!(input.carries().iter().all(|carry| *carry > 0));

    let mut worlds = 0usize;
    let mut peak = 0u64;
    for reverse in [false, true] {
        for live_booking in [false, true] {
            for order in [[0, 2, 1, 3, 4], [4, 0, 1, 2, 3]] {
                let (control_paid, control_vault) =
                    baseline(input, reverse, live_booking, order, &mut peak);
                let mut world = setup_with_extra_debt(input, reverse, &mut peak);
                if live_booking {
                    peak = peak.max(world.env.crank(
                        world.actors[1].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 6,
                            observations: crank_observations(1),
                        },
                    ));
                }
                let (mixed_paid, mixed_vault) = close_all(world, order, &mut peak);

                let mut normalized = mixed_paid;
                normalized[0] += EXTRA_DEBT;
                normalized[4] = normalized[4]
                    .checked_sub(EXTRA_DEBT)
                    .expect("the cross-asset creditor receives the exact adverse debt");
                let custody_residue = mixed_vault
                    .checked_sub(control_vault)
                    .expect("mixed route cannot consume baseline custody residue");
                assert!(
                    custody_residue <= 1,
                    "exact cross-asset debt may add only bounded protocol residue"
                );
                normalized[0] += custody_residue;
                assert_eq!(
                    normalized, control_paid,
                    "fractional B attribution must survive a same-owner cross-asset debt"
                );
                assert_eq!(
                    mixed_vault,
                    control_vault + custody_residue,
                    "exact cross-asset debt residue must stay in protocol custody"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    println!(
        "INV-039 mixed fractional cohort: {worlds} public worlds, exact debt={EXTRA_DEBT}, peak {peak} CU"
    );
}
