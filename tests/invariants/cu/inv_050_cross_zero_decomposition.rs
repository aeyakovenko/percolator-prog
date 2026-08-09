//! INV-050 - Cross-zero decomposition.
//!
//! Normative obligation: A cross-zero operation reduces only real exposure and subjects the new open to normal gates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): lifecycle exact-close
//! controls, initial-margin flip admission, and an all-four-route post-liquidation matrix. The
//! matrix publicly creates partial ADL, then compares the identical cross-zero request with and
//! without unrelated auxiliary OI. The control rejects with exact rollback; auxiliary OI alone
//! admits the Flip branch and leaves fresh current-`A` legs larger than pooled effective OI by the
//! exact prior ADL haircut. This is a route/branch expansion of the known PR250/engine-134 basis-
//! reissue root cause, not a distinct finding. These tests exercise the deployed public wrapper
//! with real SBF/LiteSVM account construction and assert economic state, token, rollback,
//! liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the matrix demonstrates public reachability on the unfixed pin; it does
//! not certify the invariant. Certification requires inverting the counterexample on the fixed
//! engine pin and retaining the no-auxiliary exit control.

use super::*;

#[derive(Clone, Copy, Debug)]
enum AdlCrossZeroRoute {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

impl AdlCrossZeroRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchTradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeCpi,
    ];

    fn uses_cpi(self) -> bool {
        matches!(self, Self::TradeCpi | Self::BatchTradeCpi)
    }
}

#[allow(clippy::too_many_arguments)]
fn try_adl_cross_zero_route(
    env: &mut V16CuEnv,
    route: AdlCrossZeroRoute,
    taker_owner: &Keypair,
    taker: Pubkey,
    lp_owner: &Keypair,
    lp: Pubkey,
    size_q: i128,
    price: u64,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
) -> Result<u64, String> {
    match route {
        AdlCrossZeroRoute::TradeNoCpi => {
            env.try_trade_asset_with_cu(0, taker_owner, taker, lp_owner, lp, size_q, price, 0)
        }
        AdlCrossZeroRoute::BatchTradeNoCpi => env.send(
            env.batch_trade_no_cpi_ix(
                taker,
                lp,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    size_q,
                    exec_price: price,
                    fee_bps: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
            ],
            &[taker_owner, lp_owner],
        ),
        AdlCrossZeroRoute::TradeCpi => {
            let (matcher_program, matcher_context, matcher_delegate) = matcher.unwrap();
            env.try_trade_cpi_with_cu_on_asset(
                taker_owner,
                taker,
                lp_owner,
                lp,
                matcher_program,
                matcher_context,
                matcher_delegate,
                0,
                size_q,
                0,
            )
        }
        AdlCrossZeroRoute::BatchTradeCpi => {
            let (matcher_program, matcher_context, matcher_delegate) = matcher.unwrap();
            env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(matcher_context, false),
                    AccountMeta::new_readonly(matcher_delegate, false),
                ],
                &[taker_owner],
            )
        }
    }
}

fn run_partial_liquidation_cross_zero_world(
    route: AdlCrossZeroRoute,
    add_unrelated_oi: bool,
) -> Option<u128> {
    const OPEN_PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 500;
    const OPEN_Q: i128 = 2 * POS_SCALE as i128;
    const NEW_SIDE_Q: i128 = POS_SCALE as i128 / 4;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, OPEN_PRICE);

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let auxiliary_long_owner = Keypair::new();
    let auxiliary_short_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    let auxiliary_long = env.create_portfolio(&auxiliary_long_owner);
    let auxiliary_short = env.create_portfolio(&auxiliary_short_owner);
    let successor = env.create_portfolio(&successor_owner);
    for (owner, portfolio, deposit) in [
        (&winner_owner, winner, 1_000),
        (&loser_owner, loser, 900),
        (&auxiliary_long_owner, auxiliary_long, 1_000_000),
        (&auxiliary_short_owner, auxiliary_short, 1_000_000),
        (&successor_owner, successor, 1_000_000),
    ] {
        env.deposit(owner, portfolio, deposit);
    }
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        OPEN_Q,
        OPEN_PRICE,
        0,
    );

    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, ADVERSE_PRICE);
    for portfolio in [loser, winner] {
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        );
    }
    env.crank_steps_after_market_catchup(
        loser,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );

    let adl = env.market_state().1;
    let winner_leg = active_leg_for_asset(&env.portfolio_state(winner), 0);
    let raw_q = winner_leg.basis_pos_q.unsigned_abs();
    let effective_q = adl.assets[0].oi_eff_long_q;
    assert_eq!(raw_q, OPEN_Q.unsigned_abs());
    assert!(effective_q > 0 && effective_q < raw_q);
    assert!(adl.assets[0].a_long < ADL_ONE);
    assert_eq!(adl.assets[0].oi_eff_short_q, effective_q);
    let unrelated_q = raw_q - effective_q;

    if add_unrelated_oi {
        env.trade_asset_with_cu(
            0,
            &auxiliary_long_owner,
            auxiliary_long,
            &auxiliary_short_owner,
            auxiliary_short,
            unrelated_q as i128,
            ADVERSE_PRICE,
            0,
        );
        let with_auxiliary = env.market_state().1;
        assert_eq!(with_auxiliary.assets[0].oi_eff_long_q, raw_q);
        assert_eq!(with_auxiliary.assets[0].oi_eff_short_q, raw_q);
    }

    let matcher = route.uses_cpi().then(|| {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (context, delegate, _) =
            env.init_auth_matcher_context(matcher_program, &successor_owner, successor);
        (matcher_program, context, delegate)
    });
    let market_before = env.svm.get_account(&env.market).unwrap();
    let winner_before = env.svm.get_account(&winner).unwrap();
    let successor_before = env.svm.get_account(&successor).unwrap();
    let matcher_before = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before = env.token_amount(env.vault);
    env.svm.expire_blockhash();
    let result = try_adl_cross_zero_route(
        &mut env,
        route,
        &winner_owner,
        winner,
        &successor_owner,
        successor,
        -((raw_q as i128) + NEW_SIDE_Q),
        ADVERSE_PRICE,
        matcher,
    );

    if !add_unrelated_oi {
        assert!(
            result.is_err(),
            "{route:?} must reject when the account's raw reduction exceeds all pooled OI"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&winner).unwrap(), winner_before);
        assert_eq!(env.svm.get_account(&successor).unwrap(), successor_before);
        if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before) {
            assert_eq!(env.svm.get_account(&context).unwrap(), before);
        }
        assert_eq!(env.token_amount(env.vault), vault_before);
        return None;
    }

    let cu = result.expect("unrelated aggregate OI admits the vulnerable cross-zero flip");
    assert_cu_within(
        &format!("{route:?} post-ADL cross-zero"),
        cu,
        TRADE_CU_LIMIT,
    );
    let after = env.market_state().1;
    assert_eq!(
        after.assets[0].oi_eff_long_q,
        after.assets[0].oi_eff_short_q
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(winner), 0).basis_pos_q,
        -NEW_SIDE_Q
    );
    let successor_leg = active_leg_for_asset(&env.portfolio_state(successor), 0);
    let auxiliary_leg = active_leg_for_asset(&env.portfolio_state(auxiliary_long), 0);
    assert_eq!(successor_leg.a_basis, after.assets[0].a_long);
    assert_eq!(auxiliary_leg.a_basis, after.assets[0].a_long);
    let fresh_long_basis = successor_leg
        .basis_pos_q
        .unsigned_abs()
        .checked_add(auxiliary_leg.basis_pos_q.unsigned_abs())
        .unwrap();
    let missing_effective_oi = fresh_long_basis
        .checked_sub(after.assets[0].oi_eff_long_q)
        .expect("fresh current-A legs exceed the corrupted pooled OI");
    assert_eq!(missing_effective_oi, unrelated_q);
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(after.vault as u64, vault_before);
    Some(missing_effective_oi)
}

#[test]
fn v16_program_post_liquidation_cross_zero_uses_unrelated_oi_on_all_routes() {
    for route in AdlCrossZeroRoute::ALL {
        assert_eq!(run_partial_liquidation_cross_zero_world(route, false), None);
        let missing = run_partial_liquidation_cross_zero_world(route, true)
            .expect("auxiliary OI must reach the vulnerable Flip branch");
        assert!(missing > 0, "{route:?} counterexample must be non-vacuous");
    }
}

#[test]
fn v16_attack_batch_nocpi_exit_only_rejects_cross_zero_flip() {
    for lifecycle_case in ["DrainOnly", "Recovery"] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
        if lifecycle_case == "Recovery" {
            env.configure_permissionless_resolve_with_cu(100, 50);
        }
        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long_account = env.create_portfolio(&long_owner);
        let short_account = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long_account, 1_000_000);
        env.deposit(&short_owner, short_account, 1_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            POS_SCALE as i128,
            100,
            0,
        );

        match lifecycle_case {
            "DrainOnly" => {
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                    0,
                    0,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[0].lifecycle,
                    AssetLifecycleV16::DrainOnly
                );
            }
            "Recovery" => {
                env.svm.warp_to_slot(10);
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                    0,
                    10,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[0].lifecycle,
                    AssetLifecycleV16::Recovery
                );
            }
            _ => unreachable!(),
        }

        let market_before = env.svm.get_account(&env.market).unwrap();
        let long_before = env.svm.get_account(&long_account).unwrap();
        let short_before = env.svm.get_account(&short_account).unwrap();
        env.svm.expire_blockhash();
        let flip = env.send(
            env.batch_trade_no_cpi_ix(
                long_account,
                short_account,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q: -(2 * POS_SCALE as i128),
                    exec_price: 100,
                    fee_bps: 0,
                }],
            ),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long_account, false),
                AccountMeta::new(short_account, false),
            ],
            &[&long_owner, &short_owner],
        );
        assert!(
            flip.is_err(),
            "{lifecycle_case} BatchTradeNoCpi must reject a cross-zero flip"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&long_account).unwrap(), long_before);
        assert_eq!(env.svm.get_account(&short_account).unwrap(), short_before);

        env.svm.expire_blockhash();
        let close_cu = env
            .send(
                env.batch_trade_no_cpi_ix(
                    long_account,
                    short_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(long_owner.pubkey(), true),
                    AccountMeta::new(short_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long_account, false),
                    AccountMeta::new(short_account, false),
                ],
                &[&long_owner, &short_owner],
            )
            .expect("exact BatchTradeNoCpi lifecycle close must remain live");
        assert_cu_within(
            &format!("{lifecycle_case} BatchTradeNoCpi exact close"),
            close_cu,
            TRADE_CU_LIMIT,
        );
        let (_, group_after) = env.market_state();
        assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
        assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(long_account), 0),
            "{lifecycle_case} exact close leaves the long account flat"
        );
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(short_account), 0),
            "{lifecycle_case} exact close leaves the short account flat"
        );
        assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
        assert!(group_after.vault >= group_after.c_tot + group_after.insurance);
    }
}

// security.md sweep — position flip margin (#19/#46 crosses_zero): a trade that flips a position
// long->short must enforce initial_margin_bps on the RESULTING side. An attacker must not be able to
// flip into a larger, under-margined opposite position.
#[test]
fn v16_attack_position_flip_enforces_initial_margin() {
    let mut env = V16CuEnv::new(); // IM = 100%
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 100); // exactly enough for notional 100 at 100% IM
    env.deposit(&lb, pb, 10_000_000); // counterparty well-funded
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0); // la long 1 (notional 100)
    let basis_open = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert_eq!(basis_open, POS_SCALE as i128, "la opened long 1");

    // ATTACK: flip to SHORT 2 (sell 3) -> needs margin 200 > capital 100 -> must reject.
    env.svm.expire_blockhash();
    let r_over = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -(3 * POS_SCALE as i128), 100, 0);
    assert!(
        r_over.is_err(),
        "flip into an under-margined short (2x notional) must reject"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis_open,
        "position unchanged by rejected over-flip"
    );

    // CONTROL: flip to SHORT 1 (sell 2) -> notional 100, margin 100 (at edge) -> allowed.
    env.svm.expire_blockhash();
    let r_ok = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -(2 * POS_SCALE as i128), 100, 0);
    assert!(
        r_ok.is_ok(),
        "flip to an equally-margined short should be allowed: {:?}",
        r_ok
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        -(POS_SCALE as i128),
        "la is now short 1 after flip"
    );
    let (_, g) = env.market_state();
    assert_eq!(g.vault, g.c_tot + g.insurance, "conservation after flip");
    assert_eq!(
        g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q,
        "OI balanced after flip"
    );
}
