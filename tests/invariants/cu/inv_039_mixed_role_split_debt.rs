//! INV-039 / rows 419, 435: two unequal debts share one pending creditor's
//! limited source support. Each debt must retain its own peer and source domain
//! through resolved settlement, even when the lower-index debt changes owner.

use super::*;

const DEBTS: [u128; 2] = [180_000, 90_000];
const DISCOUNT: u128 = GAIN - DEPOSITS[1];
const PAYOUTS: [u128; 5] = [
    DEPOSITS[0] + GAIN - RESIDUAL - DEBTS[0] - DEBTS[1] - DISCOUNT,
    0,
    DEPOSITS[2] + DEBTS[0],
    DEPOSITS[3] + DEBTS[1],
    DEPOSITS[4],
];

fn check(world: &AttributionWorld, debt_assets: [usize; 2]) {
    let env = &world.env;
    let g = env.market_state().1;
    let accounts: Vec<_> = world
        .actors
        .iter()
        .map(|a| env.portfolio_state(a.portfolio))
        .collect();
    let paid: [u128; 5] = std::array::from_fn(|i| env.token_amount(world.actors[i].token) as u128);
    let target_b = RESIDUAL * SOCIAL_LOSS_DEN / POS_SCALE;
    let mixed_legs: Vec<_> = accounts[0]
        .legs
        .iter()
        .map(|leg| leg.try_to_runtime().unwrap())
        .filter(|leg| leg.active)
        .collect();
    let charged = mixed_legs
        .iter()
        .find(|leg| leg.asset_index == 1)
        .is_none_or(|leg| leg.b_snap == target_b);
    let settled: [bool; 2] = std::array::from_fn(|i| {
        mixed_legs
            .iter()
            .find(|leg| leg.asset_index as usize == debt_assets[i])
            .is_none_or(|leg| leg.k_snap == -(90_000 * ADL_ONE as i128))
    });
    let debt: u128 = (0..2).filter(|i| settled[*i]).map(|i| DEBTS[i]).sum();
    let support = debt.min(DEPOSITS[1]);
    let discount = support * GAIN / DEPOSITS[1] - support;
    let mut expected = PAYOUTS.map(|x| x as i128);
    expected[0] =
        (DEPOSITS[0] + GAIN - debt - discount - if charged { RESIDUAL } else { 0 }) as i128;
    let close = close_progress(&accounts[1]);
    assert!(close.active && !close.canceled);
    assert_eq!(close.gross_loss_at_close_start, RESIDUAL);
    assert_eq!(
        close.b_loss_booked,
        if close.finalized { RESIDUAL } else { 0 }
    );
    assert_eq!(
        close.residual_remaining,
        if close.finalized { 0 } else { RESIDUAL }
    );
    expected[1] = -(close.residual_remaining as i128);
    assert!(!charged || close.finalized);
    if !charged || !settled.into_iter().all(|x| x) {
        assert_eq!(paid[0], 0, "both debts precede the mixed owner's payout");
    }
    for (i, p) in accounts.iter().enumerate() {
        assert_eq!(p.owner, world.actors[i].owner.pubkey().to_bytes());
        assert!(paid[i] <= PAYOUTS[i], "owner {i}: input-derived payout cap");
        verify_close_residual_partition("split mixed debt", &close_progress(p)).unwrap();
        let receipt = resolved_receipt(p);
        let unpaid = if receipt.present {
            receipt.terminal_positive_claim_face - receipt.paid_effective
        } else {
            0
        };
        assert_eq!(
            p.capital.get() as i128 + p.pnl.get() + unpaid as i128 + paid[i] as i128,
            expected[i],
            "owner {i}: debt cannot move between peers"
        );
        for source in p
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
        {
            let asset = match i {
                0 => 1,
                2 | 3 => debt_assets[i - 2],
                _ => panic!("unrelated owner acquired a source claim"),
            };
            assert_eq!(source.domain.get() as usize, 2 * asset + 1);
        }
    }
    assert_eq!(
        g.assets[1].b_long_num,
        if close.finalized { target_b } else { 0 }
    );
    assert_eq!(g.assets[1].b_short_num, 0);
    for asset in debt_assets {
        assert_eq!(
            [g.assets[asset].b_long_num, g.assets[asset].b_short_num],
            [0; 2]
        );
    }
    let vault = env.token_amount(env.vault) as u128;
    assert_eq!(
        vault + paid.iter().sum::<u128>(),
        DEPOSITS.iter().sum::<u128>()
    );
    assert_eq!(g.insurance, 0);
    assert_eq!(g.backing_provider_earnings_total, 0);
    assert_market_stock_census(
        "split mixed debt",
        &g,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("split mixed debt", &g, &accounts).unwrap();
}

#[test]
fn v16_program_mixed_role_two_debts_preserve_peer_attribution_and_terminal_progress() {
    let mut peak = 0;
    let mut calls = 0;
    let mut waits = 0;
    for debt_assets in [[0, 2], [2, 0]] {
        for peer_first in [false, true] {
            let mut world = AttributionWorld::new_with_deposits(
                false,
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
                DEPOSITS,
            );
            for (asset, holder, debtor, lots) in [
                (1, 0, 1, 1),
                (debt_assets[0], 2, 0, 2),
                (debt_assets[1], 3, 0, 1),
            ] {
                world.env.trade_asset_with_cu(
                    asset as u16,
                    &world.actors[holder].owner,
                    world.actors[holder].portfolio,
                    &world.actors[debtor].owner,
                    world.actors[debtor].portfolio,
                    lots * POS_SCALE as i128,
                    1_000_000,
                    0,
                );
            }
            for slot in 1..=10 {
                world.env.svm.warp_to_slot(slot);
                let marks = if slot <= 5 {
                    vec![(1, 1_000_000 + 40_000 * slot)]
                } else {
                    debt_assets
                        .map(|asset| (asset, 1_000_000 + 18_000 * (slot - 5)))
                        .to_vec()
                };
                for (asset, mark) in marks {
                    world
                        .env
                        .push_auth_mark_for_asset_as_admin(asset as u16, slot, mark);
                }
                world.env.crank(
                    world.actors[4].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[0, 1, 2]),
                    },
                );
                if slot == 5 {
                    world.env.trade_asset_with_cu(
                        1,
                        &world.actors[0].owner,
                        world.actors[0].portfolio,
                        &world.actors[1].owner,
                        world.actors[1].portfolio,
                        -(POS_SCALE as i128),
                        1_200_000,
                        0,
                    );
                }
            }
            for (i, asset) in debt_assets.into_iter().enumerate() {
                world.env.crank(
                    world.actors[i + 2].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 10,
                        observations: crank_observations(asset as u16),
                    },
                );
            }
            let mixed = world.env.portfolio_state(world.actors[0].portfolio);
            let legs: Vec<_> = mixed
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
                .collect();
            assert_eq!(legs.len(), 3);
            for leg in legs {
                if leg.asset_index == 1 {
                    assert_eq!(
                        (leg.basis_pos_q, leg.loss_weight, leg.b_snap),
                        (0, POS_SCALE, 0)
                    );
                } else {
                    assert_eq!(leg.k_snap, 0, "both debts remain unsettled at resolution");
                    assert_eq!(
                        world.env.market_state().1.assets[leg.asset_index as usize].k_short,
                        -(90_000 * ADL_ONE as i128)
                    );
                }
            }
            check(&world, debt_assets);
            world.env.resolve();
            world.env.svm.warp_to_slot(15);
            check(&world, debt_assets);
            let first = payout(&world, 0, false);
            let invalid = deletion(&world, 4, false);
            peak = peak.max(land(&mut world, &[first, invalid], &[], &[], Some(1)));
            check(&world, debt_assets);
            let order = if peer_first {
                [3, 2, 0, 1, 4]
            } else {
                [0, 1, 2, 3, 4]
            };
            for _ in 0..16 {
                for actor in order {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        continue;
                    }
                    let before = world.frame();
                    match world.payout(actor, false) {
                        Ok(cu) => {
                            assert_cu_within("split mixed debt terminal step", cu, 600_000);
                            peak = peak.max(cu);
                        }
                        Err(error) => {
                            assert!(is_engine_non_progress_error(&error), "{error}");
                            assert_eq!(world.frame(), before);
                            waits += 1;
                        }
                    }
                    let allowed = [
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        world.actors[actor].token,
                    ];
                    for (key, account) in before {
                        if !allowed.contains(&key) {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    calls += 1;
                    check(&world, debt_assets);
                }
                if world
                    .actors
                    .iter()
                    .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                {
                    break;
                }
            }
            for (actor, expected) in PAYOUTS.into_iter().enumerate() {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) as u128,
                    expected
                );
                if resolved_receipt(&world.env.portfolio_state(world.actors[actor].portfolio))
                    .present
                {
                    let before = world.frame();
                    world.payout(actor, true).unwrap();
                    assert_eq!(world.frame(), before, "paid receipt retry cannot move debt");
                }
            }
            let g = world.env.market_state().1;
            assert_eq!((g.c_tot, g.pnl_pos_tot, g.vault), (0, 0, DISCOUNT));
            for asset in g.assets.iter().take(3) {
                assert_eq!(
                    [
                        asset.pending_obligation_count_long,
                        asset.pending_obligation_count_short,
                        asset.stored_pos_count_long,
                        asset.stored_pos_count_short
                    ],
                    [0; 4]
                );
                assert_eq!(
                    [
                        asset.oi_eff_long_q,
                        asset.oi_eff_short_q,
                        asset.loss_weight_sum_long,
                        asset.loss_weight_sum_short
                    ],
                    [0; 4]
                );
            }
        }
    }
    println!("INV-039 split mixed debt: 4 histories, {calls} terminal calls, {waits} exact waits, 4 prefix rollbacks; peak CU={peak}");
}
