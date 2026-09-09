//! INV-039: input-derived owner attribution across two pending resolved cohorts.
//!
//! Unlike the parent's pre-resolution release matrix and single-cohort detach witness,
//! these histories resolve with two retained holders and at least one unbooked debtor.
//! Every close prefix reconciles each owner's remaining capital, PnL, receipt and SPL
//! payout against independently computed debt. A disappearing leg is not debt payment.
//! This is solvent, integral-quantity, no-fee/funding evidence, not row419 closure or
//! bankruptcy-residual, exact-partition, crank-rank or arbitrary-history coverage.

use super::*;
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct History {
    reverse_sides: bool,
    lots: [u8; 2],
    price_moves: [u16; 2],
    early_debtor: Option<usize>,
    close_order: [usize; 5],
    extra_closes: Vec<usize>,
}

struct AttributionModel {
    debt: [u128; 2],
    basis: [i128; 4],
    pending: [bool; 4],
}

impl AttributionModel {
    fn assert_matches(&self, world: &AttributionWorld) {
        world.check(self.basis, self.pending);
        let group = world.env.market_state().1;
        for (actor, initial) in ATTRIBUTION_DEPOSITS.into_iter().enumerate() {
            let account = world.env.portfolio_state(world.actors[actor].portfolio);
            let receipt = resolved_receipt(&account);
            let receipt_due = if receipt.present {
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .expect("a solvent receipt cannot pay more than its own face")
            } else {
                0
            };
            let remaining = account.capital.get() as i128
                + account.pnl.get()
                + receipt_due as i128
                + world.env.token_amount(world.actors[actor].token) as i128;
            let expected = match actor {
                0 | 2 => initial + self.debt[actor / 2],
                1 | 3 if self.basis[actor] == 0 => initial - self.debt[actor / 2],
                _ => initial,
            };
            assert_eq!(
                remaining, expected as i128,
                "actor {actor}: debt attribution"
            );
            if actor < 4 && actor % 2 == 0 && self.basis[actor + 1] != 0 {
                assert_eq!(account.capital.get(), initial);
                assert_eq!(account.pnl.get(), self.debt[actor / 2] as i128);
                assert!(
                    !receipt.present,
                    "unbooked opposing debt cannot become a receipt"
                );
                assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
                assert!(!group.payout_snapshot_captured);
            }
        }
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize) {
        let before = world.frame();
        match world.payout(actor, false) {
            Ok(cu) => {
                assert_cu_within("INV-039 generated resolved close", cu, CUSTODY_CU_LIMIT);
                // One solvent leg, no B chunks, backing normalization or fee work.
                if actor < 4 {
                    self.basis[actor] = 0;
                    self.pending[actor] = false;
                }
            }
            Err(error) => {
                assert!(
                    is_engine_non_progress_error(&error),
                    "actor {actor}: {error}"
                );
                assert_eq!(
                    world.frame(),
                    before,
                    "rejected close must restore the whole frame"
                );
            }
        }
        for (key, account) in before {
            if ![
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ]
            .contains(&key)
            {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "foreign account {key}"
                );
            }
        }
        self.assert_matches(world);
    }
}

fn run_history(history: &History) -> [u128; 5] {
    let mut world = AttributionWorld::new(history.reverse_sides);
    let sign = if history.reverse_sides { -1 } else { 1 };
    for pair in 0..2 {
        // Integral lots make the economic oracle independent of engine rounding helpers.
        let q = POS_SCALE as i128 * i128::from(history.lots[pair]) * sign;
        world.quantities[2 * pair] = q;
        world.quantities[2 * pair + 1] = -q;
        world.env.trade_asset_with_cu(
            (pair + 1) as u16,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            q,
            1_000_000,
            0,
        );
    }
    world.env.svm.warp_to_slot(20);
    for pair in 0..2 {
        world.env.push_auth_mark_for_asset_as_admin(
            (pair + 1) as u16,
            20,
            (1_000_000 + i128::from(history.price_moves[pair]) * sign) as u64,
        );
    }
    world.env.crank(
        world.actors[4].portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations_for_assets(&[1, 2]),
        },
    );
    let mut model = AttributionModel {
        debt: std::array::from_fn(|pair| {
            u128::from(history.lots[pair]) * u128::from(history.price_moves[pair])
        }),
        basis: world.quantities,
        pending: [false; 4],
    };
    for pair in 0..2 {
        let debtor_before = world
            .env
            .svm
            .get_account(&world.actors[2 * pair + 1].portfolio);
        world.env.crank(
            world.actors[2 * pair].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations((pair + 1) as u16),
            },
        );
        world.env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            (pair + 1) as u16,
            20,
            0,
        );
        world.forfeit(2 * pair);
        model.basis[2 * pair] = 0;
        model.pending[2 * pair] = true;
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.actors[2 * pair + 1].portfolio),
            debtor_before
        );
    }
    model.assert_matches(&world);
    if let Some(pair) = history.early_debtor {
        world.forfeit(2 * pair + 1);
        model.basis[2 * pair + 1] = 0;
        model.assert_matches(&world);
    }
    assert!(model.basis[1] != 0 || model.basis[3] != 0);
    assert_eq!(model.pending, [true, false, true, false]);
    let before_resolve = world.frame();
    let assets_before = world.env.market_state().1.assets;
    assert_cu_within(
        "INV-039 generated resolution",
        world.env.resolve(),
        CRANK_CU_LIMIT,
    );
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    assert_eq!(world.env.market_state().1.assets, assets_before);
    for (key, account) in before_resolve {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    model.assert_matches(&world);
    world.env.svm.warp_to_slot(25);
    for actor in history.extra_closes.iter().copied() {
        model.close(&mut world, actor);
    }
    for actor in history.close_order {
        model.close(&mut world, actor);
        model.close(&mut world, actor);
    }
    // A fixed suffix completes every generated prefix, without invoking the crank-rank oracle.
    for _ in 0..3 {
        for actor in history.close_order {
            if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                model.close(&mut world, actor);
            }
        }
    }
    let expected = std::array::from_fn(|actor| match actor {
        0 | 2 => ATTRIBUTION_DEPOSITS[actor] + model.debt[actor / 2],
        1 | 3 => ATTRIBUTION_DEPOSITS[actor] - model.debt[actor / 2],
        _ => ATTRIBUTION_DEPOSITS[actor],
    });
    assert_eq!(model.basis, [0; 4]);
    assert_eq!(model.pending, [false; 4]);
    assert_eq!(world.env.market_state().1.vault, 0);
    for actor in history.close_order.into_iter().rev() {
        let a = &world.actors[actor];
        assert!(resolved_portfolio_is_terminal(&world.env, a.portfolio));
        assert_eq!(world.env.token_amount(a.token) as u128, expected[actor]);
        let before = world.frame();
        world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
        for (key, account) in before {
            if ![world.env.market, a.owner.pubkey(), a.portfolio].contains(&key) {
                assert_eq!(world.env.svm.get_account(&key), account);
            }
        }
    }
    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    expected
}

#[test]
fn v16_program_pending_resolved_cohorts_preserve_attribution_in_every_close_order() {
    let mut worlds = 0;
    let mut baseline = None;
    for reverse_sides in [false, true] {
        for early_debtor in [None, Some(0), Some(1)] {
            for a in 0..4 {
                for b in 0..4 {
                    for c in 0..4 {
                        for d in 0..4 {
                            let mut order = [a, b, c, d];
                            order.sort_unstable();
                            if order != [0, 1, 2, 3] {
                                continue;
                            }
                            let mut close_order = vec![a, b, c, d];
                            close_order.insert(worlds % 5, 4);
                            let history = History {
                                reverse_sides,
                                lots: [1, 2],
                                price_moves: [1, 19_999],
                                early_debtor,
                                close_order: close_order.try_into().unwrap(),
                                extra_closes: Vec::new(),
                            };
                            let payout = run_history(&history);
                            assert_eq!(*baseline.get_or_insert(payout), payout, "{history:?}");
                            worlds += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 144);
    println!("INV-039: {worlds} public histories; 24 cohort close orders x 3 resolution boundaries x 2 sides; input-derived per-owner attribution at every close prefix");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PERCOLATOR_INV039_HISTORY_CASES")
            .ok().and_then(|value| value.parse().ok()).unwrap_or(16),
        max_shrink_iters: 64,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_resolved_histories.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pending_resolved_history_generator_preserves_owner_debt(
        reverse_sides in any::<bool>(),
        lots in proptest::array::uniform2(1u8..=3),
        price_moves in proptest::array::uniform2(1u16..=20_000),
        early in 0usize..3,
        priority in any::<[u8; 5]>(),
        extra_closes in proptest::collection::vec(0usize..5, 0..17),
    ) {
        let mut close_order = [0, 1, 2, 3, 4];
        close_order.sort_by_key(|actor| (priority[*actor], *actor));
        run_history(&History {
            reverse_sides,
            lots,
            price_moves,
            early_debtor: early.checked_sub(1),
            close_order,
            extra_closes,
        });
    }
}
