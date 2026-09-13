//! INV-039/024/037/041/048/067/073/081: a domain's retained weight can disappear
//! only with its own exact B debit, including when another bankruptcy is unresolved.
//! Existing dual-close locality tests stop at live residual progress; row419's
//! preemption test has one bankrupt cohort, and its two-domain resolution tests are
//! solvent. Here unequal bankrupt cohorts cross resolution with only one B booked.
//! A two-domain settlement prefix must roll back both debts together, and releasing
//! the booked cohort cannot change the other holder or discharge its close ledger.
//! No cure is involved. These finite, integral, fee/funding-free histories leave
//! row419 OPEN and do not establish arbitrary-history or INV-086 equivalence.

use super::*;
use close_preemption::{reject, terminal_instruction};

#[path = "inv_039_pending_loss_insured_resolution.rs"]
mod insured_resolution;

const GAINS: [u128; 2] = [5 * 40_000, 2 * 5 * 28_000];
const RESIDUALS: [u128; 2] = [
    GAINS[0] - ATTRIBUTION_DEPOSITS[1],
    GAINS[1] - ATTRIBUTION_DEPOSITS[3],
];
const PAYOUTS: [u128; 5] = [
    ATTRIBUTION_DEPOSITS[0] + ATTRIBUTION_DEPOSITS[1],
    0,
    ATTRIBUTION_DEPOSITS[2] + ATTRIBUTION_DEPOSITS[3],
    0,
    ATTRIBUTION_DEPOSITS[4],
];

fn setup(reverse: bool, peak: &mut u64) -> AttributionWorld {
    setup_with_deposits(reverse, peak, ATTRIBUTION_DEPOSITS)
}

fn setup_with_deposits(reverse: bool, peak: &mut u64, deposits: [u128; 5]) -> AttributionWorld {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..V16CuMarketParams::default()
        },
        deposits,
    );
    let mut basis = [0; 4];
    let mut pending = [false; 4];
    world.check(basis, pending);
    for pair in 0..2 {
        let holder = &world.actors[2 * pair];
        let debtor = &world.actors[2 * pair + 1];
        let cu = world.env.trade_asset_with_cu(
            (pair + 1) as u16,
            &holder.owner,
            holder.portfolio,
            &debtor.owner,
            debtor.portfolio,
            world.quantities[2 * pair],
            1_000_000,
            0,
        );
        assert_cu_within("INV-039 two-domain opening", cu, TRADE_CU_LIMIT);
        *peak = (*peak).max(cu);
        basis[2 * pair] = world.quantities[2 * pair];
        basis[2 * pair + 1] = world.quantities[2 * pair + 1];
        world.check(basis, pending);
    }
    let sign = if reverse { -1i64 } else { 1 };
    for slot in 1..=5 {
        world.env.svm.warp_to_slot(slot);
        for (pair, movement) in [40_000 * sign, -28_000 * sign].into_iter().enumerate() {
            world.env.push_auth_mark_for_asset_as_admin(
                (pair + 1) as u16,
                slot,
                (1_000_000 + movement * slot as i64) as u64,
            );
        }
        let cu = world.env.crank(
            world.actors[4].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations_for_assets(&[1, 2]),
            },
        );
        assert_cu_within("INV-039 two-domain accrual", cu, CRANK_CU_LIMIT);
        *peak = (*peak).max(cu);
        world.check(basis, pending);
    }
    for pair in 0..2 {
        let holder = &world.actors[2 * pair];
        let debtor = &world.actors[2 * pair + 1];
        let mark = world.env.market_state().1.assets[pair + 1].effective_price;
        assert_eq!(
            (i128::from(mark) - 1_000_000).unsigned_abs()
                * world.quantities[2 * pair].unsigned_abs()
                / POS_SCALE,
            GAINS[pair],
        );
        let before = world.frame();
        let allowed = [world.env.market, holder.portfolio, debtor.portfolio];
        let cu = world.env.trade_asset_with_cu(
            (pair + 1) as u16,
            &holder.owner,
            holder.portfolio,
            &debtor.owner,
            debtor.portfolio,
            -world.quantities[2 * pair],
            mark,
            0,
        );
        assert_cu_within("INV-039 two-domain bankruptcy", cu, TRADE_CU_LIMIT);
        *peak = (*peak).max(cu);
        basis[2 * pair] = 0;
        basis[2 * pair + 1] = 0;
        pending[2 * pair] = true;
        world.check(basis, pending);
        for (key, account) in before {
            if !allowed.contains(&key) {
                assert_eq!(world.env.svm.get_account(&key), account);
            }
        }
    }
    world
}

struct DebtModel {
    initial: [CloseProgressLedgerV16; 2],
    booked: [bool; 2],
    released: [bool; 2],
}

impl DebtModel {
    fn check(&self, world: &AttributionWorld) {
        world.check([0; 4], [!self.released[0], false, !self.released[1], false]);
        let group = world.env.market_state().1;
        assert_eq!(group.insurance, 0);
        for pair in 0..2 {
            let mut expected_close = self.initial[pair];
            if self.booked[pair] {
                expected_close.finalized = true;
                expected_close.b_loss_booked = RESIDUALS[pair];
                expected_close.residual_remaining = 0;
            }
            assert_eq!(
                close_progress(
                    &world
                        .env
                        .portfolio_state(world.actors[2 * pair + 1].portfolio)
                ),
                expected_close,
                "domain {pair}: only its debtor can book this residual"
            );
            assert_close_partition(expected_close, RESIDUALS[pair]);
            assert!(!self.released[pair] || self.booked[pair]);
        }
        for (actor, a) in world.actors.iter().enumerate() {
            let account = world.env.portfolio_state(a.portfolio);
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert!(actor == 0 || actor == 2);
                assert_eq!(
                    receipt.terminal_positive_claim_face,
                    ATTRIBUTION_DEPOSITS[actor + 1]
                );
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            let expected = match actor {
                0 | 2 if !self.released[actor / 2] => {
                    ATTRIBUTION_DEPOSITS[actor] + GAINS[actor / 2]
                }
                1 | 3 if !self.booked[actor / 2] => {
                    assert_eq!(account.capital.get(), 0);
                    assert_eq!(account.pnl.get(), -(RESIDUALS[actor / 2] as i128));
                    assert_eq!(world.env.token_amount(a.token), 0);
                    continue;
                }
                _ => PAYOUTS[actor],
            };
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + due as i128
                    + world.env.token_amount(a.token) as i128,
                expected as i128,
                "actor {actor}: exact entitlement after its own debit"
            );
            assert!(world.env.token_amount(a.token) as u128 <= PAYOUTS[actor]);
            if actor == 0 || actor == 2 {
                if !self.released[actor / 2] || self.booked.contains(&false) {
                    assert_eq!(world.env.token_amount(a.token), 0);
                    assert!(!receipt.present);
                    assert!(!group.payout_snapshot_captured);
                }
            }
        }
    }

    fn close(&self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        let before = world.frame();
        let cu = world
            .payout(actor, false)
            .expect("prescribed resolved step progresses");
        assert_cu_within("INV-039 two-domain resolved step", cu, CUSTODY_CU_LIMIT);
        *peak = (*peak).max(cu);
        assert_ne!(world.frame(), before);
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
                    "foreign Account {key}"
                );
            }
        }
    }
}

#[test]
fn v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order() {
    let mut peak_setup = 0;
    let mut peak_terminal = 0;
    let mut worlds = 0;
    let mut claim_waits = 0;
    for reverse in [false, true] {
        for early in 0..2 {
            for holders in [[0usize, 2], [2, 0]] {
                let mut world = setup(reverse, &mut peak_setup);
                let mut model = DebtModel {
                    initial: std::array::from_fn(|pair| {
                        close_progress(
                            &world
                                .env
                                .portfolio_state(world.actors[2 * pair + 1].portfolio),
                        )
                    }),
                    booked: [false; 2],
                    released: [false; 2],
                };
                for close in model.initial {
                    assert!(close.active && !close.finalized && !close.canceled);
                }
                model.check(&world);
                let late = 1 - early;
                let original_legs = [0usize, 2].map(|actor| {
                    active_leg_for_asset(
                        &world.env.portfolio_state(world.actors[actor].portfolio),
                        actor / 2 + 1,
                    )
                });
                let before = world.frame();
                let cu = world.env.crank(
                    world.actors[2 * early + 1].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 5,
                        observations: crank_observations((early + 1) as u16),
                    },
                );
                assert_cu_within("INV-039 first-domain live B booking", cu, CRANK_CU_LIMIT);
                peak_terminal = peak_terminal.max(cu);
                model.booked[early] = true;
                model.check(&world);
                for (key, account) in before {
                    if ![world.env.market, world.actors[2 * early + 1].portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                let before = world.frame();
                peak_terminal = peak_terminal.max(world.env.resolve());
                for (key, account) in before {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                world.env.svm.warp_to_slot(10);
                model.check(&world);

                // Both valid domain steps precede one rejected suffix. Neither the first
                // holder's B debit nor the second debtor's booking may survive that suffix.
                let mut suffix = close_instruction(&world, 4);
                suffix.accounts[0].is_signer = false;
                let bundle = [
                    terminal_instruction(&world, 2 * early),
                    terminal_instruction(&world, 2 * late + 1),
                    suffix.clone(),
                ];
                peak_terminal = peak_terminal.max(reject(
                    &mut world,
                    &bundle,
                    4,
                    PercolatorError::ExpectedSigner,
                ));
                model.check(&world);
                for pair in 0..2 {
                    assert_eq!(
                        active_leg_for_asset(
                            &world.env.portfolio_state(world.actors[2 * pair].portfolio),
                            pair + 1
                        ),
                        original_legs[pair]
                    );
                }

                model.close(&mut world, 2 * early, &mut peak_terminal);
                model.released[early] = true;
                model.check(&world);
                // A released first domain cannot authorize removal of the unbooked domain.
                model.close(&mut world, 2 * late, &mut peak_terminal);
                model.check(&world);
                assert_eq!(
                    active_leg_for_asset(
                        &world.env.portfolio_state(world.actors[2 * late].portfolio),
                        late + 1
                    ),
                    original_legs[late]
                );
                model.close(&mut world, 2 * late + 1, &mut peak_terminal);
                model.booked[late] = true;
                model.check(&world);
                assert_eq!(
                    active_leg_for_asset(
                        &world.env.portfolio_state(world.actors[2 * late].portfolio),
                        late + 1
                    ),
                    original_legs[late]
                );
                // Normalize the already-booked debtor and pay the independent senior owner
                // before either claimant, leaving both claim orders available for comparison.
                for actor in [2 * early + 1, 4] {
                    model.close(&mut world, actor, &mut peak_terminal);
                    model.check(&world);
                }
                for round in 0..2 {
                    for actor in holders {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        // Residual booking alone does not make the global payout census ready:
                        // the second holder must first absorb its own outstanding B debit.
                        if actor == 2 * early && !model.released[late] {
                            assert_eq!(round, 0);
                            let waiting = terminal_instruction(&world, actor);
                            peak_terminal = peak_terminal.max(reject(
                                &mut world,
                                &[waiting],
                                2,
                                PercolatorError::EngineNonProgress,
                            ));
                            claim_waits += 1;
                        } else {
                            let bundle = [terminal_instruction(&world, actor), suffix.clone()];
                            peak_terminal = peak_terminal.max(reject(
                                &mut world,
                                &bundle,
                                3,
                                PercolatorError::ExpectedSigner,
                            ));
                            model.check(&world);
                            model.close(&mut world, actor, &mut peak_terminal);
                            model.released[actor / 2] = true;
                        }
                        model.check(&world);
                    }
                }
                for (actor, expected) in PAYOUTS.into_iter().enumerate() {
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        expected
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    let retry = terminal_instruction(&world, actor);
                    peak_terminal = peak_terminal.max(reject(
                        &mut world,
                        &[retry],
                        2,
                        PercolatorError::EngineNonProgress,
                    ));
                }
                model.check(&world);
                let group = world.env.market_state().1;
                assert_eq!((group.vault, group.c_tot, group.pnl_pos_tot), (0, 0, 0));
                for a in &world.actors {
                    let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_cu_within("INV-039 two-domain terminal deletion", cu, CUSTODY_CU_LIMIT);
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_eq!(claim_waits, 4);
    println!("INV-039 two bankrupt domains: {worlds} worlds, 24 prefix rollbacks, {claim_waits} claim-gate rollbacks, 40 rejected terminal retries, 40 terminal deletions; peak CU setup={peak_setup}, terminal={peak_terminal}");
}
