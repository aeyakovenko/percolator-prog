//! INV-039/041/067: same-owner pending-creditor and funding-debtor roles
//! resolve in an order-independent way.
//!
//! Existing mixed-role resolution coverage disables funding, while funded
//! pending-debt coverage keeps creditors and debtors in separate portfolios.
//! This public LiteSVM test composes the two: actor 0 retains a zero-basis,
//! nonzero-loss-weight creditor claim on asset 1 while also carrying an
//! unsettled debtor leg on asset 2 after the asset has nonzero funding. The
//! invariant oracle is path independence over terminal resolution orders plus
//! exact custody conservation and bounded cleanup. The solvent Recovery-forfeit
//! variant also checks an input-derived owner ledger after every continuation.
//! Neither fixture proves arbitrary mixed-role funding entitlement histories.

use super::*;

const ENTRY: i128 = 1_000_000;
const TARGET_MOVE: i128 = 200_000;
const SETTLE_SLOT: u64 = 6;
const SETTLE_MOVE: i128 = 60_000;
const FUNDING_RATE_E9: u64 = 1_000;
const CREDITOR_LOTS_Q: i128 = 7 * POS_SCALE as i128 / 2;
const DEBTOR_LOTS_Q: i128 = 2 * POS_SCALE as i128;

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    paid: [u128; 5],
    vault: u128,
}

fn setup(reverse: bool) -> AttributionWorld {
    setup_with_inputs(reverse, DEPOSITS, CREDITOR_LOTS_Q, false)
}

fn setup_with_inputs(
    reverse: bool,
    deposits: [u128; 5],
    creditor_lots_q: i128,
    recovery_forfeit: bool,
) -> AttributionWorld {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: ENTRY as u64,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
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
    let debtor_q = DEBTOR_LOTS_Q * sign;
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

    for phase in 0..if recovery_forfeit { 2 } else { 1 } {
        let assets: &[u16] = match (recovery_forfeit, phase) {
            (false, _) => &[1, 2],
            (true, 0) => &[1],
            (true, _) => &[2],
        };
        for &asset in assets {
            world.env.push_auth_mark_for_asset_as_admin(
                asset,
                phase * SETTLE_SLOT + 1,
                (ENTRY + sign * TARGET_MOVE) as u64,
            );
        }
        for step in 1..=SETTLE_SLOT {
            let slot = phase * SETTLE_SLOT + step;
            world.env.svm.warp_to_slot(slot);
            world.env.crank(
                world.actors[4].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_for_assets(if phase == 0 {
                        &[1, 2]
                    } else {
                        &[2]
                    }),
                },
            );
        }
        if recovery_forfeit && phase == 0 {
            // Book the credit before forfeit; move the other asset only after
            // this refresh so the mixed owner's second debt stays unbooked.
            world.env.crank(
                world.actors[0].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: SETTLE_SLOT,
                    observations: crank_observations_for_assets(&[1, 2]),
                },
            );
            world.env.update_asset_lifecycle_as_admin_with_cu(
                processor::ASSET_ACTION_SHUTDOWN,
                1,
                SETTLE_SLOT,
                0,
            );
            world.env.forfeit_recovery_leg_with_cu(
                &world.actors[0].owner,
                world.actors[0].portfolio,
                1,
                u128::MAX,
            );
        }
    }
    let settle_price = ENTRY + sign * SETTLE_MOVE;
    for asset in [1usize, 2] {
        assert_eq!(
            world.env.market_state().1.assets[asset].effective_price as i128,
            settle_price
        );
        assert_ne!(
            (
                world.env.market_state().1.assets[asset].f_long_num,
                world.env.market_state().1.assets[asset].f_short_num
            ),
            (0, 0),
            "asset {asset} must carry nonzero funding before the mixed close"
        );
    }

    if !recovery_forfeit {
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
    }

    let funded = world.env.market_state().1.assets[2];
    assert_eq!(funded.effective_price as i128, settle_price);
    assert_ne!((funded.f_long_num, funded.f_short_num), (0, 0));

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
    assert_eq!(debtor.loss_weight, DEBTOR_LOTS_Q as u128);
    if recovery_forfeit {
        assert_eq!(
            (debtor.k_snap, debtor.f_snap),
            (0, 0),
            "mixed debt is unbooked"
        );
    }
    assert_eq!(legs.len(), 2);
    world
}

fn close_all(mut world: AttributionWorld, order: [usize; 5], peak: &mut u64) -> Outcome {
    let (outcome, expiries) = close_all_checked(&mut world, order, peak, |_, _| {});
    assert_eq!(
        expiries, 1,
        "the last funding receipt must survive until backing expiry"
    );
    outcome
}

fn expire_last_receipt_backing(world: &mut AttributionWorld, deleted: [bool; 5], peak: &mut u64) {
    assert_eq!(deleted, [false, true, true, true, true]);
    let portfolio = world.actors[0].portfolio;
    let receipt = resolved_receipt(&world.env.portfolio_state(portfolio));
    assert!(receipt.present && !receipt.finalized);
    assert_eq!(
        (receipt.terminal_positive_claim_face, receipt.paid_effective),
        (2, 0)
    );
    let group = world.env.market_state().1;
    let domains: Vec<_> = group
        .source_credit
        .iter()
        .enumerate()
        .filter(|(_, credit)| credit.fresh_reserved_backing_num != 0)
        .map(|(domain, credit)| {
            assert_eq!(credit.fresh_reserved_backing_num, BOUND_SCALE);
            domain
        })
        .collect();
    assert_eq!(
        domains.len(),
        1,
        "one input-created rounding atom remains reserved"
    );
    let domain = domains[0];
    let expiry = group.source_backing_buckets[domain].expiry_slot;
    assert!(expiry > world.env.svm.get_sysvar::<Clock>().slot);
    let crank = ProgInstruction::PermissionlessCrank {
        now_slot: expiry,
        observations: vec![CrankObservationHint {
            asset_index: (domain / 2) as u16,
            oracle_accounts: 0,
        }],
    };
    let accounts = vec![
        AccountMeta::new_readonly(world.actors[0].owner.pubkey(), false),
        AccountMeta::new(world.env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    // The hint cannot make committed backing expire before authenticated time.
    let before = world.frame();
    world.env.svm.expire_blockhash();
    let error = world
        .env
        .send(crank.clone(), accounts.clone(), &[])
        .expect_err("future expiry hint cannot release backing early");
    assert!(is_engine_non_progress_error(&error), "{error}");
    assert_eq!(world.frame(), before);

    world.env.svm.warp_to_slot(expiry);
    world.env.svm.expire_blockhash();
    let cu = world
        .env
        .send(crank, accounts, &[])
        .expect("hinted committed backing expiry");
    assert_cu_within("mixed funding retained receipt expiry", cu, CRANK_CU_LIMIT);
    *peak = (*peak).max(cu);
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    assert_eq!(
        world.env.market_state().1.source_credit[domain].fresh_reserved_backing_num,
        0
    );
    let paid = world.env.token_amount(world.actors[0].token);
    let vault = world.env.token_amount(world.env.vault);
    let cu = world
        .payout(0, true)
        .expect("retained funding receipt claims released backing");
    assert_cu_within("mixed funding positive receipt topup", cu, CUSTODY_CU_LIMIT);
    *peak = (*peak).max(cu);
    assert_eq!(world.env.token_amount(world.actors[0].token), paid + 1);
    assert_eq!(world.env.token_amount(world.env.vault), vault - 1);
    let after = resolved_receipt(&world.env.portfolio_state(portfolio));
    assert_eq!(
        after.terminal_positive_claim_face,
        receipt.terminal_positive_claim_face
    );
    assert_eq!(after.paid_effective, 1);
    println!("mixed funding: retained 2-atom receipt, exact expiry {expiry}, 1-atom topup");
}

fn close_all_checked(
    world: &mut AttributionWorld,
    order: [usize; 5],
    peak: &mut u64,
    check: impl Fn(&AttributionWorld, [bool; 5]),
) -> (Outcome, usize) {
    check(world, [false; 5]);
    let before = world.frame();
    *peak = (*peak).max(world.env.resolve());
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }

    world
        .env
        .svm
        .warp_to_slot(world.env.market_state().1.resolved_slot + 6);
    let mut deleted = [false; 5];
    check(world, deleted);
    let mut waiting_rejections = 0usize;
    let mut backing_expiries = 0;
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
                check(world, deleted);
            }
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                if resolved_receipt(&world.env.portfolio_state(world.actors[actor].portfolio))
                    .present
                {
                    let before = world.frame();
                    *peak = (*peak).max(world.payout(actor, true).expect("paid receipt retry"));
                    assert_eq!(world.frame(), before);
                    check(world, deleted);
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
                check(world, deleted);
            }
        }
        if deleted.iter().all(|value| *value) {
            break;
        }
        if !progressed {
            expire_last_receipt_backing(world, deleted, peak);
            backing_expiries += 1;
            check(world, deleted);
        }
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
        (0, 0, 0, 0)
    );
    let paid = std::array::from_fn(|i| world.env.token_amount(world.actors[i].token) as u128);
    let vault = world.env.token_amount(world.env.vault) as u128;
    assert_eq!(vault, group.vault);
    assert_eq!(
        vault + paid.iter().sum::<u128>(),
        world.deposits.iter().sum::<u128>()
    );
    (Outcome { paid, vault }, backing_expiries)
}

#[test]
fn v16_program_solvent_mixed_funding_preserves_input_derived_owner_entitlements() {
    const DEPOSITS: [u128; 5] = [500_000, 500_000, 500_000, 250_000, 777];
    const LOTS: [i128; 2] = [4, 2];
    let mut peak = 0;
    let mut checkpoints = 0;
    for sign in [-1i128, 1] {
        // Every reported step has a premium beyond the configured rate cap.
        // Floor each signed one-slot payment before scaling the funding index.
        let funding: i128 = (1..=SETTLE_SLOT)
            .map(|slot| {
                let price = ENTRY + sign * i128::from(slot) * ENTRY / 100;
                (sign * FUNDING_RATE_E9 as i128 * price).div_euclid(1_000_000_000)
            })
            .sum();
        assert_eq!(funding, sign * 6);
        let per_lot = SETTLE_MOVE - sign * funding;
        let gains = LOTS.map(|lots| (lots * per_lot) as u128);
        let expected = [
            DEPOSITS[0] + gains[0] - gains[1],
            DEPOSITS[1] - gains[0],
            DEPOSITS[2] + gains[1],
            DEPOSITS[3],
            DEPOSITS[4],
        ];
        assert_eq!(expected.iter().sum::<u128>(), DEPOSITS.iter().sum());
        for order in [[0, 2, 1, 3, 4], [1, 2, 0, 4, 3]] {
            let mut world =
                setup_with_inputs(sign < 0, DEPOSITS, LOTS[0] * POS_SCALE as i128, true);
            let checks = std::cell::Cell::new(0);
            let check = |world: &AttributionWorld, deleted: [bool; 5]| {
                checks.set(checks.get() + 1);
                let env = &world.env;
                let group = env.market_state().1;
                let mut accounts = Vec::new();
                let mut actual = [0i128; 5];
                let mut oi = [[0u128; 2]; 3];
                let mut weights = oi;
                let mut counts = [[0u64; 2]; 3];
                let mut pending = counts;
                for (actor, owner) in world.actors.iter().enumerate() {
                    let paid = u128::from(env.token_amount(owner.token));
                    assert!(
                        paid <= expected[actor],
                        "owner {actor}: paid entitlement bound"
                    );
                    actual[actor] = paid as i128;
                    if deleted[actor] {
                        assert_eq!(paid, expected[actor]);
                        assert!(env
                            .svm
                            .get_account(&owner.portfolio)
                            .is_none_or(|a| { a.lamports == 0 && a.data.is_empty() }));
                        continue;
                    }
                    let account = env.portfolio_state(owner.portfolio);
                    assert_eq!(account.owner, owner.owner.pubkey().to_bytes());
                    assert_eq!(close_progress(&account), CloseProgressLedgerV16::EMPTY);
                    let receipt = resolved_receipt(&account);
                    let due = if receipt.present {
                        receipt
                            .terminal_positive_claim_face
                            .checked_sub(receipt.paid_effective)
                            .unwrap()
                    } else {
                        0
                    };
                    actual[actor] +=
                        account.capital.get() as i128 + account.pnl.get() + due as i128;
                    for leg in account
                        .legs
                        .iter()
                        .map(|l| l.try_to_runtime().unwrap())
                        .filter(|l| l.active)
                    {
                        let asset = leg.asset_index as usize;
                        assert!((1..=2).contains(&asset));
                        let holder = if asset == 1 { 0 } else { 2 };
                        let payer = if asset == 1 { 1 } else { 0 };
                        assert!(actor == holder || actor == payer, "foreign debt domain");
                        let direction = sign * if actor == holder { 1 } else { -1 };
                        let side = usize::from(direction < 0);
                        assert_eq!(
                            leg.side,
                            if side == 0 {
                                SideV16::Long
                            } else {
                                SideV16::Short
                            }
                        );
                        let q = LOTS[asset - 1] as u128 * POS_SCALE;
                        assert_eq!(leg.loss_weight, q);
                        assert!(leg.basis_pos_q == 0 || leg.basis_pos_q == direction * q as i128);
                        assert_eq!(leg.a_basis, ADL_ONE);
                        assert_eq!((leg.b_snap, leg.b_rem), (0, 0));
                        let k = direction * sign * SETTLE_MOVE * ADL_ONE as i128;
                        let f = -direction * funding * ADL_ONE as i128;
                        assert!(
                            (leg.k_snap, leg.f_snap) == (0, 0)
                                || (leg.k_snap, leg.f_snap) == (k, f),
                            "owner {actor}, asset {asset}: leg={leg:?}; expected k={k}, f={f}"
                        );
                        if leg.basis_pos_q != 0 && leg.k_snap == 0 {
                            actual[actor] += if actor == holder {
                                gains[asset - 1] as i128
                            } else {
                                -(gains[asset - 1] as i128)
                            };
                        }
                        oi[asset][side] += leg.basis_pos_q.unsigned_abs();
                        weights[asset][side] += q;
                        counts[asset][side] += 1;
                        pending[asset][side] += u64::from(leg.basis_pos_q == 0);
                    }
                    accounts.push(account);
                }
                assert_eq!(
                    actual,
                    expected.map(|amount| amount as i128),
                    "input-derived mixed funding entitlement"
                );
                for asset in 1..=2 {
                    let a = group.assets[asset];
                    assert_eq!(a.effective_price as i128, ENTRY + sign * SETTLE_MOVE);
                    assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
                    assert_eq!(
                        [a.k_long, a.k_short],
                        [
                            sign * SETTLE_MOVE * ADL_ONE as i128,
                            -sign * SETTLE_MOVE * ADL_ONE as i128
                        ]
                    );
                    assert_eq!(
                        [a.f_long_num, a.f_short_num],
                        [-funding * ADL_ONE as i128, funding * ADL_ONE as i128]
                    );
                    assert_eq!([a.b_long_num, a.b_short_num], [0; 2]);
                    assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
                    assert_eq!(
                        [a.loss_weight_sum_long, a.loss_weight_sum_short],
                        weights[asset]
                    );
                    assert_eq!(
                        [a.stored_pos_count_long, a.stored_pos_count_short],
                        counts[asset]
                    );
                    assert_eq!(
                        [
                            a.pending_obligation_count_long,
                            a.pending_obligation_count_short
                        ],
                        pending[asset]
                    );
                }
                let vault = u128::from(env.token_amount(env.vault));
                crate::support::fuzz_model::assert_market_stock_census(
                    "solvent mixed funding",
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    vault,
                )
                .unwrap();
                assert_eq!(group.materialized_portfolio_count as usize, accounts.len());
                assert_eq!(group.insurance, 0);
                let supply = DEPOSITS.iter().sum::<u128>();
                assert_eq!(
                    vault
                        + world
                            .actors
                            .iter()
                            .map(|a| u128::from(env.token_amount(a.token)))
                            .sum::<u128>(),
                    supply
                );
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(u128::from(mint.supply), supply);
                assert_eq!(
                    mint.mint_authority,
                    solana_program::program_option::COption::None
                );
            };
            let (outcome, expiries) = close_all_checked(&mut world, order, &mut peak, check);
            assert_eq!(expiries, 0, "solvent payouts need no backing expiry");
            assert_eq!(
                outcome,
                Outcome {
                    paid: expected,
                    vault: 0
                }
            );
            checkpoints += checks.get();
        }
    }
    println!("INV-039 solvent mixed funding: 4 worlds, {checkpoints} independent ledger checkpoints, 20 exact payouts/deletions; peak {peak} CU");
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
