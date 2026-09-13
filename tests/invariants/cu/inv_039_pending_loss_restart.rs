//! INV-039 / row419: pending debt survives another domain's Recovery restart.
//!
//! Two unequal, solvent obligations cross both sides, domain completion orders and
//! claimant orders. Indebted-asset restart rejects with live exposure, retained
//! weight or an old source claim. An unrelated asset restarts while both domains are
//! pending and again after only one debtor pays. Exact original-owner entitlements
//! survive the generation changes, delayed resolution and permissionless payout.

use super::*;

const SLOT: u64 = 20;
const RESTART_PRICE: u64 = 1_100_000;

struct RestartModel {
    basis: [i128; 4],
    pending: [bool; 4],
    paid: [bool; 2],
}

impl RestartModel {
    fn check(&self, world: &AttributionWorld) {
        world.check(self.basis, self.pending);
        for (i, actor) in world.actors.iter().enumerate() {
            let account = world.env.portfolio_state(actor.portfolio);
            let mut capital = ATTRIBUTION_DEPOSITS[i];
            let mut pnl = 0;
            if i < 4 {
                let pair = i / 2;
                let debt = world.debt(pair);
                if i % 2 == 0 {
                    pnl = debt as i128;
                } else if self.paid[pair] {
                    capital -= debt;
                }
            }
            assert_eq!(
                account.capital.get(),
                capital,
                "actor {i}: original capital/debt"
            );
            assert_eq!(
                account.pnl.get(),
                pnl,
                "actor {i}: original claim attribution"
            );
            assert!(!resolved_receipt(&account).present);
            assert_eq!(world.env.token_amount(actor.token), 0);
            let mut claim_num = 0;
            for source in &account.source_domains {
                if source.source_claim_bound_num.get() != 0 {
                    assert!(i < 4 && i % 2 == 0);
                    let asset = i / 2 + 1;
                    let domain = 2 * asset + usize::from(world.quantities[i + 1] < 0);
                    assert_eq!(source.domain.get() as usize, domain);
                    assert_eq!(
                        source.source_claim_market_id.get(),
                        world.env.asset_market_id(asset as u16)
                    );
                    claim_num += source.source_claim_bound_num.get();
                }
            }
            assert_eq!(
                claim_num,
                pnl as u128 * BOUND_SCALE,
                "actor {i}: exact original-domain source face"
            );
            for leg in account.legs.iter().map(|leg| leg.try_to_runtime().unwrap()) {
                if leg.active {
                    assert_eq!(
                        leg.market_id,
                        world
                            .env
                            .asset_market_id(leg.asset_index.try_into().unwrap()),
                        "a retained obligation belongs to its original generation"
                    );
                }
            }
        }
        assert_eq!(world.env.market_state().1.mode, MarketModeV16::Live);
    }
}

fn reject_restart(world: &mut AttributionWorld, asset: u16, checkpoint: &str) {
    let admin = world.env.admin.insecure_clone();
    world.env.svm.expire_blockhash();
    let before = world.frame();
    let error = world
        .env
        .try_restart_asset_oracle_with_authority(&admin, asset, SLOT, RESTART_PRICE)
        .expect_err(checkpoint);
    assert!(
        error.contains(&format!(
            "InstructionError(2, Custom({}))",
            PercolatorError::EngineLockActive as u32
        )),
        "{checkpoint}: must reach the economic restart gate: {error}"
    );
    assert_eq!(
        world.frame(),
        before,
        "{checkpoint}: complete Account rollback"
    );
}

fn check_other_domain(
    world: &AttributionWorld,
    before: &[(Pubkey, Option<Account>)],
    other: usize,
) {
    let data = &before
        .iter()
        .find(|(key, _)| *key == world.env.market)
        .unwrap()
        .1
        .as_ref()
        .unwrap()
        .data;
    let after = world.env.svm.get_account(&world.env.market).unwrap();
    let asset = other + 1;
    assert_eq!(
        market_engine_slot_bytes(&after.data, asset),
        market_engine_slot_bytes(data, asset),
        "other domain: complete engine slot, including old credit and obligation counters"
    );
    assert_eq!(
        state::read_asset_oracle_profile(&after.data, asset).unwrap(),
        state::read_asset_oracle_profile(data, asset).unwrap(),
        "other domain: oracle and authority profile"
    );
    for actor in [2 * other, 2 * other + 1, 4] {
        let a = &world.actors[actor];
        for key in [a.owner.pubkey(), a.portfolio, a.token] {
            assert_eq!(
                world.env.svm.get_account(&key),
                before.iter().find(|(k, _)| *k == key).unwrap().1,
                "other domain/bystander: complete Account frame"
            );
        }
    }
}

fn restart_sibling(world: &mut AttributionWorld, peak_cu: &mut u64) {
    let before = world.frame();
    let cu = world.env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        0,
        SLOT,
        0,
    );
    assert_cu_within("INV-039 sibling shutdown", cu, CRANK_CU_LIMIT);
    *peak_cu = (*peak_cu).max(cu);
    for pair in 0..2 {
        check_other_domain(world, &before, pair);
    }
    let group = world.env.market_state().1;
    assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    let old_id = group.assets[0].market_id;
    let next_id = group.next_market_id;
    let admin = world.env.admin.insecure_clone();
    world.env.svm.expire_blockhash();
    let cu = world
        .env
        .try_restart_asset_oracle_with_authority(&admin, 0, SLOT, RESTART_PRICE)
        .expect("an empty sibling can restart while old debt and source claims remain elsewhere");
    assert_cu_within("INV-039 sibling restart", cu, CRANK_CU_LIMIT);
    *peak_cu = (*peak_cu).max(cu);
    let restarted = world.env.market_state().1;
    assert_eq!(restarted.assets[0].market_id, next_id);
    assert!(next_id > old_id);
    assert_eq!(restarted.next_market_id, next_id + 1);
    assert_eq!(restarted.assets[0].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(restarted.assets[0].effective_price, RESTART_PRICE);
    for pair in 0..2 {
        check_other_domain(world, &before, pair);
    }
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(
                world.env.svm.get_account(&key),
                account,
                "sibling lifecycle cannot move owner value"
            );
        }
    }
}

#[test]
fn v16_program_pending_debt_survives_sibling_restart_and_delayed_resolution() {
    assert_certified_engine_pin("INV-039 pending debt and Recovery restart");
    let mut worlds = 0;
    let mut rejected_restarts = 0;
    let mut peak_cu = 0;
    for reverse in [false, true] {
        for pair_order in [[0usize, 1], [1, 0]] {
            for reverse_payouts in [false, true] {
                let mut world = AttributionWorld::new(reverse);
                for pair in 0..2 {
                    let winner = &world.actors[2 * pair];
                    let debtor = &world.actors[2 * pair + 1];
                    let cu = world.env.trade_asset_with_cu(
                        (pair + 1) as u16,
                        &winner.owner,
                        winner.portfolio,
                        &debtor.owner,
                        debtor.portfolio,
                        world.quantities[2 * pair],
                        1_000_000,
                        0,
                    );
                    assert_cu_within("INV-039 restart opening", cu, TRADE_CU_LIMIT);
                    peak_cu = peak_cu.max(cu);
                }
                let debtors_before =
                    [1, 3].map(|i| world.env.svm.get_account(&world.actors[i].portfolio));
                world.env.svm.warp_to_slot(SLOT);
                for pair in 0..2 {
                    let mark = (1_000_000
                        + ATTRIBUTION_PRICE_MOVES[pair] * world.quantities[2 * pair].signum())
                        as u64;
                    world
                        .env
                        .push_auth_mark_for_asset_as_admin((pair + 1) as u16, SLOT, mark);
                }
                world.env.crank(
                    world.actors[4].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: SLOT,
                        observations: crank_observations_for_assets(&[1, 2]),
                    },
                );
                for pair in 0..2 {
                    world.env.crank(
                        world.actors[2 * pair].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: SLOT,
                            observations: crank_observations((pair + 1) as u16),
                        },
                    );
                    world.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        (pair + 1) as u16,
                        SLOT,
                        0,
                    );
                    world.forfeit(2 * pair);
                    assert_eq!(
                        world.env.market_state().1.assets[pair + 1].lifecycle,
                        AssetLifecycleV16::Recovery
                    );
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.actors[2 * pair + 1].portfolio),
                        debtors_before[pair]
                    );
                }
                let mut model = RestartModel {
                    basis: [0, world.quantities[1], 0, world.quantities[3]],
                    pending: [true, false, true, false],
                    paid: [false; 2],
                };
                model.check(&world);
                let original_ids = [1, 2].map(|asset| world.env.asset_market_id(asset));

                for pair in pair_order {
                    let asset = (pair + 1) as u16;
                    let winner = 2 * pair;
                    let debtor = winner + 1;
                    let other = 1 - pair;
                    assert!(!model.paid[pair]);
                    assert!(model.pending[winner]);
                    restart_sibling(&mut world, &mut peak_cu);
                    model.check(&world);
                    assert_eq!(
                        world.env.svm.get_account(&world.actors[debtor].portfolio),
                        debtors_before[pair]
                    );
                    assert_eq!(
                        [1, 2].map(|asset| world.env.asset_market_id(asset)),
                        original_ids
                    );
                    let sibling_before = world.frame();
                    reject_restart(
                        &mut world,
                        asset,
                        "unbooked opposing debt retains the old generation",
                    );
                    rejected_restarts += 1;
                    model.check(&world);

                    world.forfeit(debtor);
                    model.basis[debtor] = 0;
                    model.paid[pair] = true;
                    model.check(&world);
                    check_other_domain(&world, &sibling_before, other);
                    let a = world.env.market_state().1.assets[asset as usize];
                    assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], [0; 2]);
                    assert_eq!(
                        a.pending_obligation_count_long + a.pending_obligation_count_short,
                        1
                    );
                    reject_restart(
                        &mut world,
                        asset,
                        "zero OI does not clear retained loss weight",
                    );
                    rejected_restarts += 1;
                    model.check(&world);

                    let cu = world.env.crank(
                        world.actors[winner].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: SLOT,
                            observations: crank_observations(asset),
                        },
                    );
                    assert_cu_within("INV-039 restart pending release", cu, CRANK_CU_LIMIT);
                    peak_cu = peak_cu.max(cu);
                    model.pending[winner] = false;
                    model.check(&world);
                    check_other_domain(&world, &sibling_before, other);
                    reject_restart(
                        &mut world,
                        asset,
                        "detachment does not consume the original source claim",
                    );
                    rejected_restarts += 1;
                    model.check(&world);

                    if !model.paid[other] {
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.actors[2 * other + 1].portfolio),
                            debtors_before[other]
                        );
                        assert_eq!(
                            world.env.asset_market_id((other + 1) as u16),
                            original_ids[other]
                        );
                        assert!(model.pending[2 * other]);
                    }
                }

                let mut expected = ATTRIBUTION_DEPOSITS;
                for pair in 0..2 {
                    expected[2 * pair] += world.debt(pair);
                    expected[2 * pair + 1] -= world.debt(pair);
                }
                world.env.resolve();
                world.env.svm.warp_to_slot(SLOT + 5);
                let order = if reverse_payouts {
                    [4, 3, 2, 1, 0]
                } else {
                    [0, 1, 2, 3, 4]
                };
                for actor in order {
                    for _ in 0..4 {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            break;
                        }
                        let before = world.frame();
                        let cu = world
                            .payout(actor, false)
                            .expect("settled original debt has a bounded payout continuation");
                        assert_cu_within("INV-039 restarted history payout", cu, CUSTODY_CU_LIMIT);
                        peak_cu = peak_cu.max(cu);
                        assert_ne!(
                            world.frame(),
                            before,
                            "a successful continuation must progress"
                        );
                        world.check([0; 4], [false; 4]);
                        for (i, a) in world.actors.iter().enumerate() {
                            let account = world.env.portfolio_state(a.portfolio);
                            let receipt = resolved_receipt(&account);
                            let unpaid = if receipt.present {
                                receipt
                                    .terminal_positive_claim_face
                                    .checked_sub(receipt.paid_effective)
                                    .unwrap()
                            } else {
                                0
                            };
                            assert_eq!(
                                account.capital.get() as i128
                                    + account.pnl.get()
                                    + unpaid as i128
                                    + world.env.token_amount(a.token) as i128,
                                expected[i] as i128,
                                "actor {i}: exact entitlement through every payout prefix"
                            );
                            if i != actor {
                                for key in [a.portfolio, a.token] {
                                    assert_eq!(
                                        world.env.svm.get_account(&key),
                                        before.iter().find(|(k, _)| *k == key).unwrap().1
                                    );
                                }
                            }
                        }
                    }
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        expected[actor]
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                }
                assert_eq!(world.env.market_state().1.vault, 0);
                for actor in order {
                    let paid = world.frame();
                    world
                        .payout(actor, true)
                        .expect("settled receipt retry after all initial payouts");
                    assert_eq!(
                        world.frame(),
                        paid,
                        "payout is exactly once across generation changes"
                    );
                }
                for actor in order {
                    let a = &world.actors[actor];
                    let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_cu_within("INV-039 restarted history deletion", cu, CUSTODY_CU_LIMIT);
                    assert_eq!(world.env.token_amount(a.token) as u128, expected[actor]);
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rejected_restarts), (8, 48));
    eprintln!("INV-039 restart: {worlds} worlds, {rejected_restarts} complete restart rollbacks, 16 sibling restarts, 40 exact payouts/retries/deletions; peak CU={peak_cu}");
}
