//! INV-050 - Cross-zero decomposition.
//!
//! Normative obligation: A cross-zero operation reduces only real exposure and subjects the new open to normal gates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): lifecycle exact-close
//! controls, initial-margin flip admission, and an all-four-route post-liquidation matrix. The
//! matrix publicly creates partial ADL, then compares the identical cross-zero request with and
//! without unrelated auxiliary OI. Both reject with exact rollback. The auxiliary-OI cases also
//! attempt one atom beyond the account-local effective exposure before closing exactly that
//! exposure through each public trade route. The control cases close through owner
//! `RebalanceReduce`. A separate all-route scalar matrix crosses zero, one-atom reduction,
//! one-atom opposite exposure, exact close, and the first value above the public trade cap. This
//! proves the cap does not turn into an exit DoS. These tests exercise real SBF/LiteSVM account
//! construction and assert economic state, token, rollback, liveness, and compute outcomes.
//!
//! Guarantee boundary: the matrix covers all four trade routes and both OI preflight branches,
//! three distinct publicly reached `a_long` ratios, a mirrored `a_short` ratio, and eleven derived
//! forbidden interior quantities per account-local world. Scalar boundaries and all deployed
//! lifecycle modes are covered separately. A second matrix composes opposite-side active-close
//! barriers on two assets at once, then covers both assets through every route while framing both
//! close ledgers, retaining both account-local obligation classes, and restoring withdrawal.
//! ResetPending stale raw legs and Retired exposure unreachability are covered on every route and
//! both side orientations; INV-046 supplies the existing public Resolved strict/cross-zero and
//! terminal-payout matrix. Full-width ADL arithmetic equivalence remains owned by INV-051/085;
//! there is no remaining wrapper-specific cross-zero partition on the current public surface.

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
    try_adl_cross_zero_route_on_asset(
        env,
        route,
        0,
        taker_owner,
        taker,
        lp_owner,
        lp,
        size_q,
        price,
        matcher,
    )
}

#[allow(clippy::too_many_arguments)]
fn try_adl_cross_zero_route_on_asset(
    env: &mut V16CuEnv,
    route: AdlCrossZeroRoute,
    asset_index: u16,
    taker_owner: &Keypair,
    taker: Pubkey,
    lp_owner: &Keypair,
    lp: Pubkey,
    size_q: i128,
    price: u64,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
) -> Result<u64, String> {
    match route {
        AdlCrossZeroRoute::TradeNoCpi => env.try_trade_asset_with_cu(
            asset_index,
            taker_owner,
            taker,
            lp_owner,
            lp,
            size_q,
            price,
            0,
        ),
        AdlCrossZeroRoute::BatchTradeNoCpi => env.send(
            env.batch_trade_no_cpi_ix(
                taker,
                lp,
                vec![BatchTradeLeg {
                    asset_index,
                    market_id: env.asset_market_id(asset_index),
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
                asset_index,
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
                        asset_index,
                        market_id: env.asset_market_id(asset_index),
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

#[allow(clippy::too_many_arguments)]
fn assert_adl_route_rejects_exactly(
    env: &mut V16CuEnv,
    route: AdlCrossZeroRoute,
    taker_owner: &Keypair,
    taker: Pubkey,
    lp_owner: &Keypair,
    lp: Pubkey,
    size_q: i128,
    price: u64,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    require_account_local_gate: bool,
    context: &str,
) {
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let matcher_before =
        matcher.map(|(_, matcher_context, _)| env.svm.get_account(&matcher_context).unwrap());

    env.svm.expire_blockhash();
    let error = try_adl_cross_zero_route(
        env,
        route,
        taker_owner,
        taker,
        lp_owner,
        lp,
        size_q,
        price,
        matcher,
    )
    .expect_err(context);

    if require_account_local_gate {
        assert!(
            error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
            "{context} must reach the account-local ADL gate: {error}"
        );
    }
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    if let (Some((_, matcher_context, _)), Some(before)) = (matcher, matcher_before) {
        assert_eq!(env.svm.get_account(&matcher_context).unwrap(), before);
    }
}

fn run_partial_liquidation_cross_zero_world(
    route: AdlCrossZeroRoute,
    add_unrelated_oi: bool,
    loser_deposit: u128,
    winner_side: SideV16,
    adverse_price: u64,
    maintenance_fee_per_slot: u128,
) -> u128 {
    const OPEN_PRICE: u64 = 100;
    const OPEN_Q: i128 = 2 * POS_SCALE as i128;
    const NEW_SIDE_Q: i128 = POS_SCALE as i128 / 4;

    let open_q = match winner_side {
        SideV16::Long => OPEN_Q,
        SideV16::Short => -OPEN_Q,
    };
    let signed_reduction = |quantity_q: u128| match winner_side {
        SideV16::Long => -(quantity_q as i128),
        SideV16::Short => quantity_q as i128,
    };

    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1,
        10_000,
        10_000,
        10_000,
        maintenance_fee_per_slot,
    );
    env.configure_auth_mark_with_cu(0, OPEN_PRICE);

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let auxiliary_winner_owner = Keypair::new();
    let auxiliary_loser_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    let auxiliary_winner = env.create_portfolio(&auxiliary_winner_owner);
    let auxiliary_loser = env.create_portfolio(&auxiliary_loser_owner);
    let successor = env.create_portfolio(&successor_owner);
    for (owner, portfolio, deposit) in [
        (&winner_owner, winner, 1_000),
        (&loser_owner, loser, loser_deposit),
        (&auxiliary_winner_owner, auxiliary_winner, 1_000_000),
        (&auxiliary_loser_owner, auxiliary_loser, 1_000_000),
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
        open_q,
        OPEN_PRICE,
        0,
    );
    if add_unrelated_oi {
        env.trade_asset_with_cu(
            0,
            &auxiliary_winner_owner,
            auxiliary_winner,
            &auxiliary_loser_owner,
            auxiliary_loser,
            open_q,
            OPEN_PRICE,
            0,
        );
    }

    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, adverse_price);
    for portfolio in [loser, winner] {
        let _ = env.send_crank_if_actionable(
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
    if add_unrelated_oi && maintenance_fee_per_slot > 0 {
        env.sync_maintenance_fee_with_cu(auxiliary_winner, None, 6);
        env.sync_maintenance_fee_with_cu(auxiliary_loser, None, 6);
    }

    let adl = env.market_state().1;
    let winner_leg = active_leg_for_asset(&env.portfolio_state(winner), 0);
    assert_eq!(winner_leg.side, winner_side);
    let raw_q = winner_leg.basis_pos_q.unsigned_abs();
    let (side_a, global_effective_q, opposite_effective_q) = match winner_side {
        SideV16::Long => (
            adl.assets[0].a_long,
            adl.assets[0].oi_eff_long_q,
            adl.assets[0].oi_eff_short_q,
        ),
        SideV16::Short => (
            adl.assets[0].a_short,
            adl.assets[0].oi_eff_short_q,
            adl.assets[0].oi_eff_long_q,
        ),
    };
    let winner_effective_num = raw_q
        .checked_mul(side_a)
        .expect("bounded ADL effective quantity");
    let winner_effective_q = winner_effective_num / winner_leg.a_basis;
    let winner_effective_ceiling_q =
        winner_effective_q + u128::from(winner_effective_num % winner_leg.a_basis != 0);
    assert_eq!(raw_q, OPEN_Q.unsigned_abs());
    assert!(winner_effective_q > 0 && winner_effective_q < raw_q);
    assert!(winner_effective_ceiling_q < raw_q);
    assert!(side_a < ADL_ONE);
    assert_eq!(opposite_effective_q, global_effective_q);
    if add_unrelated_oi {
        assert!(
            global_effective_q >= raw_q,
            "preexisting auxiliary OI must admit the raw reduction preflight"
        );
    } else {
        assert!(
            global_effective_q < raw_q,
            "control must reject before the raw reduction exceeds pooled OI"
        );
    }

    if add_unrelated_oi {
        let reduction_matcher = route.uses_cpi().then(|| {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes =
                std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            let (context, delegate, _) = env.init_auth_matcher_context(
                matcher_program,
                &auxiliary_loser_owner,
                auxiliary_loser,
            );
            (matcher_program, context, delegate)
        });
        let effective_gap_q = raw_q - winner_effective_ceiling_q;
        let mut forbidden_reductions = vec![
            winner_effective_ceiling_q + 1,
            winner_effective_ceiling_q + effective_gap_q / 4,
            winner_effective_ceiling_q + effective_gap_q / 2,
            winner_effective_ceiling_q + effective_gap_q * 3 / 4,
            raw_q - 1,
            raw_q,
        ];
        forbidden_reductions.sort_unstable();
        forbidden_reductions.dedup();
        for reduction_q in forbidden_reductions {
            assert!(reduction_q > winner_effective_ceiling_q && reduction_q <= raw_q);
            assert_adl_route_rejects_exactly(
                &mut env,
                route,
                &winner_owner,
                winner,
                &auxiliary_loser_owner,
                auxiliary_loser,
                signed_reduction(reduction_q),
                adverse_price,
                reduction_matcher,
                true,
                &format!(
                    "{route:?} post-ADL reduction {reduction_q}/{raw_q} must not consume stale raw basis"
                ),
            );
        }
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
    let mut cross_zero_suffixes = if add_unrelated_oi {
        vec![
            1,
            winner_effective_q / 4,
            winner_effective_q / 2,
            winner_effective_q * 3 / 4,
            winner_effective_q,
        ]
    } else {
        vec![NEW_SIDE_Q as u128]
    };
    cross_zero_suffixes.retain(|suffix_q| *suffix_q > 0);
    cross_zero_suffixes.sort_unstable();
    cross_zero_suffixes.dedup();
    for suffix_q in cross_zero_suffixes {
        if add_unrelated_oi {
            assert!(
                raw_q <= global_effective_q,
                "generated raw reduction component must pass the pooled-OI preflight"
            );
            assert!(suffix_q <= winner_effective_q);
        }
        assert_adl_route_rejects_exactly(
            &mut env,
            route,
            &winner_owner,
            winner,
            &successor_owner,
            successor,
            signed_reduction(raw_q + suffix_q),
            adverse_price,
            matcher,
            add_unrelated_oi,
            &format!(
                "{route:?} post-ADL cross-zero suffix {suffix_q}/{winner_effective_q} must not reissue fresh basis"
            ),
        );
    }

    let exit_q = winner_effective_ceiling_q;
    let market_before_exit = env.market_state().1;
    let vault_before_exit = env.token_amount(env.vault);
    let exit_cu = if add_unrelated_oi {
        let exit_matcher = route.uses_cpi().then(|| {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes =
                std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            let (context, delegate, _) = env.init_auth_matcher_context(
                matcher_program,
                &auxiliary_loser_owner,
                auxiliary_loser,
            );
            (matcher_program, context, delegate)
        });
        env.svm.expire_blockhash();
        let cu = try_adl_cross_zero_route(
            &mut env,
            route,
            &winner_owner,
            winner,
            &auxiliary_loser_owner,
            auxiliary_loser,
            signed_reduction(exit_q),
            adverse_price,
            exit_matcher,
        )
        .expect("the exact ADL-effective exposure must remain trade-closeable");
        assert_cu_within(
            &format!("{route:?} exact post-ADL trade exit"),
            cu,
            TRADE_CU_LIMIT,
        );
        cu
    } else {
        let cu = env.rebalance_reduce_with_cu(&winner_owner, winner, 0, exit_q);
        assert_cu_within(
            &format!("{route:?} exact post-ADL owner exit"),
            cu,
            CUSTODY_CU_LIMIT,
        );
        cu
    };
    assert!(exit_cu > 0);
    let market_after_exit = env.market_state().1;
    assert!(!has_active_leg_for_asset(&env.portfolio_state(winner), 0));
    assert_eq!(
        market_after_exit.assets[0].oi_eff_long_q,
        market_before_exit.assets[0].oi_eff_long_q - exit_q
    );
    assert_eq!(
        market_after_exit.assets[0].oi_eff_short_q,
        market_before_exit.assets[0].oi_eff_short_q - exit_q
    );
    assert_eq!(env.token_amount(env.vault), vault_before_exit);
    side_a
}

#[test]
fn v16_program_post_liquidation_cross_zero_rejects_basis_reissue_on_all_routes() {
    for route in AdlCrossZeroRoute::ALL {
        run_partial_liquidation_cross_zero_world(route, false, 900, SideV16::Long, 500, 0);
    }
}

#[test]
fn v16_program_post_adl_generated_interior_quantities_span_ratios_and_directions() {
    let mut reached_ratios = Vec::new();
    for loser_deposit in [850, 875, 900] {
        let mut route_ratios = Vec::new();
        for route in AdlCrossZeroRoute::ALL {
            route_ratios.push(run_partial_liquidation_cross_zero_world(
                route,
                true,
                loser_deposit,
                SideV16::Long,
                500,
                0,
            ));
        }
        assert!(
            route_ratios.windows(2).all(|pair| pair[0] == pair[1]),
            "route choice must not alter the publicly reached ADL ratio: {route_ratios:?}"
        );
        reached_ratios.push(route_ratios[0]);
    }
    reached_ratios.sort_unstable();
    reached_ratios.dedup();
    assert_eq!(
        reached_ratios.len(),
        3,
        "the generated public worlds must reach three distinct nonunit ADL ratios"
    );

    let mut route_ratios = Vec::new();
    for route in AdlCrossZeroRoute::ALL {
        route_ratios.push(run_partial_liquidation_cross_zero_world(
            route,
            true,
            200,
            SideV16::Short,
            20,
            1,
        ));
    }
    assert!(
        route_ratios.windows(2).all(|pair| pair[0] == pair[1]),
        "route choice must not alter the mirrored public ADL ratio: {route_ratios:?}"
    );
}

#[allow(clippy::too_many_arguments)]
fn create_public_active_close_on_asset(
    env: &mut V16CuEnv,
    asset_index: u16,
    winner_owner: &Keypair,
    winner: Pubkey,
    loser_owner: &Keypair,
    loser: Pubkey,
    keeper: Pubkey,
    winner_side: SideV16,
) -> CloseProgressLedgerV16 {
    const MOVE_STEPS: usize = 20;

    let catchup_slot = env.svm.get_sysvar::<Clock>().slot;
    let catchup_cu = env.crank_steps_after_market_catchup(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: catchup_slot,
            observations: crank_observations(asset_index),
        },
        1,
    );
    if catchup_cu != 0 {
        assert_cu_within(
            &format!("asset {asset_index} pre-close market catch-up"),
            catchup_cu,
            CRANK_CU_LIMIT,
        );
    }
    assert_eq!(
        env.market_state().1.assets[asset_index as usize].slot_last,
        catchup_slot,
        "the close fixture starts from a current asset"
    );

    for _ in 0..MOVE_STEPS {
        let current_slot = env.svm.get_sysvar::<Clock>().slot;
        let next_slot = current_slot.checked_add(1).expect("fixture slot overflow");
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let profile = state::read_asset_oracle_profile(&market_data, asset_index as usize).unwrap();
        let next_mark = match winner_side {
            SideV16::Long => {
                profile
                    .mark_ewma_e6
                    .checked_mul(10_500)
                    .expect("fixture mark overflow")
                    / 10_000
            }
            SideV16::Short => (profile.mark_ewma_e6.saturating_mul(9_500) / 10_000).max(1),
        };
        env.svm.warp_to_slot(next_slot);
        env.push_auth_mark_for_asset_as_admin(asset_index, next_slot, next_mark);
        let crank_cu = env
            .send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: next_slot,
                    observations: crank_observations(asset_index),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                ],
                &[],
            )
            .expect("each authenticated mark step must produce bounded market progress");
        assert_cu_within(
            &format!("asset {asset_index} adverse mark crank"),
            crank_cu,
            CRANK_CU_LIMIT,
        );
    }

    let winner_leg = active_leg_for_asset(&env.portfolio_state(winner), asset_index as usize);
    assert_eq!(winner_leg.side, winner_side);
    let close_q = winner_leg
        .basis_pos_q
        .checked_neg()
        .expect("fixture reduction negation overflow");
    let exec_price = env.market_state().1.assets[asset_index as usize].effective_price;
    let reduction_cu = env.trade_asset_with_cu(
        asset_index,
        winner_owner,
        winner,
        loser_owner,
        loser,
        close_q,
        exec_price,
        0,
    );
    assert_cu_within(
        &format!("asset {asset_index} bankruptcy close creation"),
        reduction_cu,
        TRADE_CU_LIMIT,
    );

    let close = close_progress(&env.portfolio_state(loser));
    let retained = active_leg_for_asset(&env.portfolio_state(winner), asset_index as usize);
    assert!(close.active && !close.canceled && !close.finalized);
    assert_eq!(close.asset_index, u32::from(asset_index));
    assert_eq!(close.domain_side, winner_side);
    assert!(close.residual_remaining > 0);
    assert_eq!(close.residual_remaining, close.gross_loss_at_close_start);
    assert_eq!(retained.basis_pos_q, 0);
    assert!(retained.loss_weight > 0);
    close
}

fn cure_public_close_with_bounded_deposit(env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey) {
    const MAX_CURE_SOURCE: u64 = 100_000_000_000;

    let source = env.token_account(owner.pubkey(), MAX_CURE_SOURCE);
    let mut amount = 1_000_000u128;
    loop {
        let portfolio_id = env.portfolio_id(portfolio);
        let position_epoch = env.portfolio_position_epoch(portfolio);
        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::CureAndCancelClose {
                portfolio_id,
                position_epoch,
                optional_deposit: amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        );
        match result {
            Ok(cu) => {
                assert_cu_within("simultaneous barrier cure", cu, CUSTODY_CU_LIMIT);
                assert!(close_progress(&env.portfolio_state(portfolio)).canceled);
                return;
            }
            Err(error)
                if error.contains("Custom(14)") || error.contains("custom program error: 0xe") =>
            {
                amount = amount
                    .checked_mul(2)
                    .filter(|next| *next <= u128::from(MAX_CURE_SOURCE))
                    .unwrap_or_else(|| {
                        panic!("no bounded public cure amount; last error: {error}")
                    });
            }
            Err(error) => panic!("public simultaneous-barrier cure failed: {error}"),
        }
    }
}

fn release_zero_basis_obligation(env: &mut V16CuEnv, portfolio: Pubkey, asset_index: u16) {
    const RELEASE_BOUND: usize = 8;

    let next_slot = env.market_state().1.assets[asset_index as usize]
        .slot_last
        .checked_add(1)
        .expect("release slot overflow");
    if env.svm.get_sysvar::<Clock>().slot < next_slot {
        env.svm.warp_to_slot(next_slot);
    }
    for _ in 0..RELEASE_BOUND {
        let portfolio_state = env.portfolio_state(portfolio);
        let owns_obligation = portfolio_state
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .any(|leg| {
                leg.active
                    && leg.asset_index == u32::from(asset_index)
                    && leg.basis_pos_q == 0
                    && leg.loss_weight != 0
            });
        if !owns_obligation {
            break;
        }
        let current_slot = env.svm.get_sysvar::<Clock>().slot;
        let cu = env
            .send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: current_slot,
                    observations: crank_observations(asset_index),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            )
            .expect("the real obligation owner must expose bounded progress");
        assert_cu_within(
            "simultaneous barrier obligation release",
            cu,
            CRANK_CU_LIMIT,
        );
    }
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(portfolio),
        asset_index as usize
    ));
}

fn run_simultaneous_cross_asset_barrier_world(route: AdlCrossZeroRoute) {
    const INITIAL_PRICE: u64 = 1_000_000;
    const CLOSE_Q: i128 = 3 * POS_SCALE as i128 / 4;
    const AUXILIARY_Q: i128 = POS_SCALE as i128 / 4;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: INITIAL_PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        max_bankrupt_close_lifetime_slots: 1_000,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let close0_winner_owner = Keypair::new();
    let close0_loser_owner = Keypair::new();
    let close1_winner_owner = Keypair::new();
    let close1_loser_owner = Keypair::new();
    let auxiliary_long_owner = Keypair::new();
    let auxiliary_short_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let close0_winner = env.create_portfolio(&close0_winner_owner);
    let close0_loser = env.create_portfolio(&close0_loser_owner);
    let close1_winner = env.create_portfolio(&close1_winner_owner);
    let close1_loser = env.create_portfolio(&close1_loser_owner);
    let auxiliary_long = env.create_portfolio(&auxiliary_long_owner);
    let auxiliary_short = env.create_portfolio(&auxiliary_short_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    for (owner, portfolio, amount) in [
        (&close0_winner_owner, close0_winner, 1_000_000),
        (&close0_loser_owner, close0_loser, 161_600),
        (&close1_winner_owner, close1_winner, 1_000_000),
        (&close1_loser_owner, close1_loser, 161_600),
        (&auxiliary_long_owner, auxiliary_long, 10_000_000),
        (&auxiliary_short_owner, auxiliary_short, 10_000_000),
        (&keeper_owner, keeper, 1),
    ] {
        env.deposit(owner, portfolio, amount);
    }

    env.trade_asset_with_cu(
        0,
        &close0_winner_owner,
        close0_winner,
        &close0_loser_owner,
        close0_loser,
        CLOSE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &close1_winner_owner,
        close1_winner,
        &close1_loser_owner,
        close1_loser,
        -CLOSE_Q,
        INITIAL_PRICE,
        0,
    );
    for asset_index in [0, 1] {
        env.trade_asset_with_cu(
            asset_index,
            &auxiliary_long_owner,
            auxiliary_long,
            &auxiliary_short_owner,
            auxiliary_short,
            AUXILIARY_Q,
            INITIAL_PRICE,
            0,
        );
    }

    let close0 = create_public_active_close_on_asset(
        &mut env,
        0,
        &close0_winner_owner,
        close0_winner,
        &close0_loser_owner,
        close0_loser,
        keeper,
        SideV16::Long,
    );
    let close1 = create_public_active_close_on_asset(
        &mut env,
        1,
        &close1_winner_owner,
        close1_winner,
        &close1_loser_owner,
        close1_loser,
        keeper,
        SideV16::Short,
    );
    assert_eq!(close_progress(&env.portfolio_state(close0_loser)), close0);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
    for asset_index in [0usize, 1] {
        let asset = env.market_state().1.assets[asset_index];
        assert_eq!(asset.lifecycle, AssetLifecycleV16::Active);
        assert!(
            asset.pending_obligation_count_long > 0 || asset.pending_obligation_count_short > 0
        );
    }

    let current_slot = env.svm.get_sysvar::<Clock>().slot;
    let catchup_cu = env.crank_steps_after_market_catchup(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: current_slot,
            observations: crank_observations(0),
        },
        1,
    );
    assert_cu_within(
        "cross-asset barrier market catch-up",
        catchup_cu,
        CRANK_CU_LIMIT,
    );
    assert_eq!(env.market_state().1.assets[0].slot_last, current_slot);
    assert_eq!(close_progress(&env.portfolio_state(close0_loser)), close0);
    assert_eq!(close_progress(&env.portfolio_state(close1_loser)), close1);

    let matcher = route.uses_cpi().then(|| {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (context, delegate, _) =
            env.init_auth_matcher_context(matcher_program, &auxiliary_short_owner, auxiliary_short);
        (matcher_program, context, delegate)
    });

    for asset_index in [0u16, 1] {
        let price = env.market_state().1.assets[asset_index as usize].effective_price;
        let market_before = env.svm.get_account(&env.market).unwrap();
        let auxiliary_long_before = env.svm.get_account(&auxiliary_long).unwrap();
        let auxiliary_short_before = env.svm.get_account(&auxiliary_short).unwrap();
        let matcher_before = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let error = try_adl_cross_zero_route_on_asset(
            &mut env,
            route,
            asset_index,
            &auxiliary_long_owner,
            auxiliary_long,
            &auxiliary_short_owner,
            auxiliary_short,
            -(AUXILIARY_Q + 1),
            price,
            matcher,
        )
        .expect_err("a simultaneous domain barrier must reject cross-zero basis reissue");
        assert!(
            error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
            "{route:?} asset {asset_index} must reach EngineLockActive: {error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(
            env.svm.get_account(&auxiliary_long).unwrap(),
            auxiliary_long_before
        );
        assert_eq!(
            env.svm.get_account(&auxiliary_short).unwrap(),
            auxiliary_short_before
        );
        if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before) {
            assert_eq!(env.svm.get_account(&context).unwrap(), before);
        }
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        assert_eq!(close_progress(&env.portfolio_state(close0_loser)), close0);
        assert_eq!(close_progress(&env.portfolio_state(close1_loser)), close1);

        env.svm.expire_blockhash();
        let exit_cu = try_adl_cross_zero_route_on_asset(
            &mut env,
            route,
            asset_index,
            &auxiliary_long_owner,
            auxiliary_long,
            &auxiliary_short_owner,
            auxiliary_short,
            -AUXILIARY_Q,
            price,
            matcher,
        )
        .expect("the exact same-side pair exit must remain live under simultaneous barriers");
        assert_cu_within(
            &format!("{route:?} simultaneous barrier asset {asset_index} exit"),
            exit_cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        let (barrier_account, detached_account, barrier_side) = if asset_index == 0 {
            (auxiliary_long, auxiliary_short, SideV16::Long)
        } else {
            (auxiliary_short, auxiliary_long, SideV16::Short)
        };
        let retained =
            active_leg_for_asset(&env.portfolio_state(barrier_account), asset_index as usize);
        assert_eq!(retained.side, barrier_side);
        assert_eq!(retained.basis_pos_q, 0);
        assert!(retained.loss_weight > 0);
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(detached_account),
            asset_index as usize
        ));
        let group = env.market_state().1;
        assert_eq!(group.assets[asset_index as usize].oi_eff_long_q, 0);
        assert_eq!(group.assets[asset_index as usize].oi_eff_short_q, 0);
        assert_eq!(close_progress(&env.portfolio_state(close0_loser)), close0);
        assert_eq!(close_progress(&env.portfolio_state(close1_loser)), close1);
    }

    cure_public_close_with_bounded_deposit(&mut env, &close0_loser_owner, close0_loser);
    cure_public_close_with_bounded_deposit(&mut env, &close1_loser_owner, close1_loser);
    release_zero_basis_obligation(&mut env, close0_winner, 0);
    release_zero_basis_obligation(&mut env, auxiliary_long, 0);
    release_zero_basis_obligation(&mut env, close1_winner, 1);
    release_zero_basis_obligation(&mut env, auxiliary_short, 1);
    let group = env.market_state().1;
    for asset in &group.assets[..2] {
        assert_eq!(asset.pending_obligation_count_long, 0);
        assert_eq!(asset.pending_obligation_count_short, 0);
    }
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
    assert!(group.vault >= group.c_tot + group.insurance);
    for (owner, portfolio) in [
        (&auxiliary_long_owner, auxiliary_long),
        (&auxiliary_short_owner, auxiliary_short),
    ] {
        env.svm.expire_blockhash();
        let (destination, withdraw_cu) = env.withdraw_with_cu(owner, portfolio, 1);
        assert_cu_within(
            "post-barrier owner withdrawal",
            withdraw_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(env.token_amount(destination), 1);
    }
}

#[test]
fn v16_program_simultaneous_cross_asset_barriers_preserve_all_route_exits() {
    for route in AdlCrossZeroRoute::ALL {
        run_simultaneous_cross_asset_barrier_world(route);
    }
}

fn run_active_cross_zero_quantity_boundary_world(route: AdlCrossZeroRoute) {
    const PRICE: u64 = 100;
    const OPEN_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 1_000);
    env.configure_auth_mark_with_cu(0, PRICE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 20_000_000_000);
    env.deposit(&short_owner, short, 20_000_000_000);
    env.trade_asset_with_cu(0, &long_owner, long, &short_owner, short, OPEN_Q, PRICE, 0);
    let matcher = route.uses_cpi().then(|| {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (context, delegate, _) =
            env.init_auth_matcher_context(matcher_program, &short_owner, short);
        (matcher_program, context, delegate)
    });

    let market_before_zero = env.svm.get_account(&env.market).unwrap();
    let long_before_zero = env.svm.get_account(&long).unwrap();
    let short_before_zero = env.svm.get_account(&short).unwrap();
    let matcher_before_zero = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before_zero = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    assert!(
        try_adl_cross_zero_route(
            &mut env,
            route,
            &long_owner,
            long,
            &short_owner,
            short,
            0,
            PRICE,
            matcher,
        )
        .is_err(),
        "{route:?} zero-size trade must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_zero
    );
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_zero);
    assert_eq!(env.svm.get_account(&short).unwrap(), short_before_zero);
    if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before_zero) {
        assert_eq!(env.svm.get_account(&context).unwrap(), before);
    }
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_zero);

    env.svm.expire_blockhash();
    let one_atom_reduce_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        -1,
        PRICE,
        matcher,
    )
    .expect("one-atom same-side reduction must land");
    assert_cu_within(
        &format!("{route:?} one-atom reduction"),
        one_atom_reduce_cu,
        TRADE_CU_LIMIT,
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long), 0).basis_pos_q,
        OPEN_Q - 1
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short), 0).basis_pos_q,
        -(OPEN_Q - 1)
    );

    env.svm.expire_blockhash();
    let one_atom_flip_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        -OPEN_Q,
        PRICE,
        matcher,
    )
    .expect("one-atom opposite exposure must pass the ordinary Active gate");
    assert_cu_within(
        &format!("{route:?} one-atom cross-zero"),
        one_atom_flip_cu,
        TRADE_CU_LIMIT,
    );
    let long_after_flip = active_leg_for_asset(&env.portfolio_state(long), 0);
    let short_after_flip = active_leg_for_asset(&env.portfolio_state(short), 0);
    assert_eq!(long_after_flip.side, SideV16::Short);
    assert_eq!(short_after_flip.side, SideV16::Long);
    assert_eq!(long_after_flip.basis_pos_q.unsigned_abs(), 1);
    assert_eq!(short_after_flip.basis_pos_q.unsigned_abs(), 1);

    env.svm.expire_blockhash();
    let exact_close_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        1,
        PRICE,
        matcher,
    )
    .expect("one-atom opposite exposure must remain exactly closeable");
    assert_cu_within(
        &format!("{route:?} one-atom exact close"),
        exact_close_cu,
        TRADE_CU_LIMIT,
    );
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[0].oi_eff_short_q, 0);

    let market_before_cap = env.svm.get_account(&env.market).unwrap();
    let long_before_cap = env.svm.get_account(&long).unwrap();
    let short_before_cap = env.svm.get_account(&short).unwrap();
    let matcher_before_cap = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before_cap = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let above_cap =
        i128::try_from(percolator::MAX_TRADE_SIZE_Q + 1).expect("MAX_TRADE_SIZE_Q + 1 fits i128");
    assert!(
        try_adl_cross_zero_route(
            &mut env,
            route,
            &long_owner,
            long,
            &short_owner,
            short,
            above_cap,
            PRICE,
            matcher,
        )
        .is_err(),
        "{route:?} first quantity above the public cap must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before_cap);
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_cap);
    assert_eq!(env.svm.get_account(&short).unwrap(), short_before_cap);
    if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before_cap) {
        assert_eq!(env.svm.get_account(&context).unwrap(), before);
    }
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_cap);

    let max_q = i128::try_from(percolator::MAX_TRADE_SIZE_Q)
        .expect("MAX_TRADE_SIZE_Q fits the signed request domain");
    env.svm.expire_blockhash();
    let max_open_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        max_q,
        PRICE,
        matcher,
    )
    .expect("the exact public quantity cap must remain executable");
    assert_cu_within(
        &format!("{route:?} exact maximum quantity open"),
        max_open_cu,
        TRADE_CU_LIMIT,
    );
    let (_, max_group) = env.market_state();
    assert_eq!(
        max_group.assets[0].oi_eff_long_q,
        percolator::MAX_TRADE_SIZE_Q
    );
    assert_eq!(
        max_group.assets[0].oi_eff_short_q,
        percolator::MAX_TRADE_SIZE_Q
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long), 0).basis_pos_q,
        max_q
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short), 0).basis_pos_q,
        -max_q
    );

    env.svm.expire_blockhash();
    let max_close_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        -max_q,
        PRICE,
        matcher,
    )
    .expect("the exact maximum position must remain closeable");
    assert_cu_within(
        &format!("{route:?} exact maximum quantity close"),
        max_close_cu,
        TRADE_CU_LIMIT,
    );
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    let (_, terminal_group) = env.market_state();
    assert_eq!(terminal_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(terminal_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(terminal_group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_cross_zero_scalar_boundaries_hold_on_all_routes() {
    for route in AdlCrossZeroRoute::ALL {
        run_active_cross_zero_quantity_boundary_world(route);
    }
}

#[test]
fn v16_program_exit_only_lifecycles_reject_cross_zero_on_all_routes() {
    for route in AdlCrossZeroRoute::ALL {
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
            let matcher = route.uses_cpi().then(|| {
                let matcher_program = Pubkey::new_unique();
                let matcher_bytes =
                    std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
                env.svm.add_program(matcher_program, &matcher_bytes);
                let (context, delegate, _) =
                    env.init_auth_matcher_context(matcher_program, &short_owner, short_account);
                (matcher_program, context, delegate)
            });

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
            let matcher_before =
                matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            env.svm.expire_blockhash();
            let flip = try_adl_cross_zero_route(
                &mut env,
                route,
                &long_owner,
                long_account,
                &short_owner,
                short_account,
                -(2 * POS_SCALE as i128),
                100,
                matcher,
            );
            assert!(
                flip.is_err(),
                "{route:?} {lifecycle_case} must reject a cross-zero flip"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&long_account).unwrap(), long_before);
            assert_eq!(env.svm.get_account(&short_account).unwrap(), short_before);
            if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before) {
                assert_eq!(env.svm.get_account(&context).unwrap(), before);
            }
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

            env.svm.expire_blockhash();
            let close_cu = try_adl_cross_zero_route(
                &mut env,
                route,
                &long_owner,
                long_account,
                &short_owner,
                short_account,
                -(POS_SCALE as i128),
                100,
                matcher,
            )
            .expect("exact exit-only lifecycle close must remain live");
            assert_cu_within(
                &format!("{route:?} {lifecycle_case} exact close"),
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
}

fn run_reset_pending_stale_leg_world(route: AdlCrossZeroRoute, reducer_side: SideV16) {
    const PRICE: u64 = 100;
    const OPEN_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new();
    let reducer_owner = Keypair::new();
    let stale_owner = Keypair::new();
    let fresh_owner = Keypair::new();
    let reducer = env.create_portfolio(&reducer_owner);
    let stale = env.create_portfolio(&stale_owner);
    let fresh = env.create_portfolio(&fresh_owner);
    for (owner, portfolio) in [
        (&reducer_owner, reducer),
        (&stale_owner, stale),
        (&fresh_owner, fresh),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }

    let signed_open_q = match reducer_side {
        SideV16::Long => OPEN_Q,
        SideV16::Short => -OPEN_Q,
    };
    env.trade_asset_with_cu(
        0,
        &reducer_owner,
        reducer,
        &stale_owner,
        stale,
        signed_open_q,
        PRICE,
        0,
    );
    let reduce_cu = env.rebalance_reduce_with_cu(&reducer_owner, reducer, 0, OPEN_Q as u128);
    assert_cu_within(
        &format!("{route:?} {reducer_side:?} ResetPending creation"),
        reduce_cu,
        CUSTODY_CU_LIMIT,
    );

    let stale_leg = active_leg_for_asset(&env.portfolio_state(stale), 0);
    let expected_stale_side = match reducer_side {
        SideV16::Long => SideV16::Short,
        SideV16::Short => SideV16::Long,
    };
    assert_eq!(stale_leg.side, expected_stale_side);
    assert_eq!(stale_leg.basis_pos_q.unsigned_abs(), OPEN_Q as u128);
    let reset_side = stale_leg.side;
    let reset = env.market_state().1;
    let reset_mode = match reset_side {
        SideV16::Long => reset.assets[0].mode_long,
        SideV16::Short => reset.assets[0].mode_short,
    };
    assert_eq!(reset_mode, SideModeV16::ResetPending);
    assert_eq!(reset.assets[0].oi_eff_long_q, 0);
    assert_eq!(reset.assets[0].oi_eff_short_q, 0);
    let cross_zero_q = if stale_leg.basis_pos_q > 0 {
        -(OPEN_Q + 1)
    } else {
        OPEN_Q + 1
    };

    let matcher = route.uses_cpi().then(|| {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (context, delegate, _) =
            env.init_auth_matcher_context(matcher_program, &fresh_owner, fresh);
        (matcher_program, context, delegate)
    });
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_before = env.svm.get_account(&stale).unwrap();
    let fresh_before = env.svm.get_account(&fresh).unwrap();
    let matcher_before = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let error = try_adl_cross_zero_route(
        &mut env,
        route,
        &stale_owner,
        stale,
        &fresh_owner,
        fresh,
        cross_zero_q,
        PRICE,
        matcher,
    )
    .expect_err("a prior-epoch ResetPending leg must not be reissued across zero");
    assert!(
        error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
        "{route:?} {reset_side:?} stale-leg cross-zero must reach EngineLockActive: {error}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&stale).unwrap(), stale_before);
    assert_eq!(env.svm.get_account(&fresh).unwrap(), fresh_before);
    if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before) {
        assert_eq!(env.svm.get_account(&context).unwrap(), before);
    }
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let crank_cu = env
        .send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: env.svm.get_sysvar::<Clock>().slot,
                observations: Vec::new(),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(stale, false),
            ],
            &[],
        )
        .expect("the stale ResetPending leg must expose permissionless cleanup");
    assert_cu_within("ResetPending stale-leg cleanup", crank_cu, CRANK_CU_LIMIT);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(stale), 0));
    let reset_side_wire = match reset_side {
        SideV16::Long => 0,
        SideV16::Short => 1,
    };
    let finalize_cu = env.finalize_reset_side_with_cu(0, reset_side_wire);
    assert_cu_within(
        "ResetPending stale-leg finalization",
        finalize_cu,
        CUSTODY_CU_LIMIT,
    );
    let finalized = env.market_state().1;
    let finalized_mode = match reset_side {
        SideV16::Long => finalized.assets[0].mode_long,
        SideV16::Short => finalized.assets[0].mode_short,
    };
    assert_eq!(finalized_mode, SideModeV16::Normal);

    env.svm.expire_blockhash();
    let retry_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &stale_owner,
        stale,
        &fresh_owner,
        fresh,
        cross_zero_q,
        PRICE,
        matcher,
    )
    .expect("the identical quantity must become ordinary fresh risk after reset finalization");
    assert_cu_within("post-reset fresh retry", retry_cu, TRADE_CU_LIMIT);
    env.svm.expire_blockhash();
    let close_cu = try_adl_cross_zero_route(
        &mut env,
        route,
        &stale_owner,
        stale,
        &fresh_owner,
        fresh,
        -cross_zero_q,
        PRICE,
        matcher,
    )
    .expect("post-reset fresh risk must retain a complete matched exit");
    assert_cu_within("post-reset fresh close", close_cu, TRADE_CU_LIMIT);
    let terminal = env.market_state().1;
    assert_eq!(terminal.assets[0].oi_eff_long_q, 0);
    assert_eq!(terminal.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(stale), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(fresh), 0));
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_reset_pending_stale_leg_cannot_reissue_basis_on_any_route() {
    for route in AdlCrossZeroRoute::ALL {
        for reducer_side in [SideV16::Long, SideV16::Short] {
            run_reset_pending_stale_leg_world(route, reducer_side);
        }
    }
}

fn run_retired_exposure_unreachability_world(route: AdlCrossZeroRoute, open_side: SideV16) {
    const ASSET_INDEX: u16 = 1;
    const PRICE: u64 = 100;
    const OPEN_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(ASSET_INDEX, 1, PRICE);
    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 1_000_000);
    env.deposit(&owner_b, account_b, 1_000_000);
    let matcher = route.uses_cpi().then(|| {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (context, delegate, _) =
            env.init_auth_matcher_context(matcher_program, &owner_b, account_b);
        (matcher_program, context, delegate)
    });
    let signed_open_q = match open_side {
        SideV16::Long => OPEN_Q,
        SideV16::Short => -OPEN_Q,
    };
    env.svm.expire_blockhash();
    let open_cu = try_adl_cross_zero_route_on_asset(
        &mut env,
        route,
        ASSET_INDEX,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        signed_open_q,
        PRICE,
        matcher,
    )
    .expect("the public route must create live exposure before the retirement attempt");
    assert_cu_within("pre-retirement public open", open_cu, TRADE_CU_LIMIT);
    let opened = env.market_state().1;
    assert_eq!(
        opened.assets[ASSET_INDEX as usize].oi_eff_long_q,
        OPEN_Q as u128
    );
    assert_eq!(
        opened.assets[ASSET_INDEX as usize].oi_eff_short_q,
        OPEN_Q as u128
    );

    let lifecycle_cu = env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        ASSET_INDEX,
        0,
        0,
    );
    assert_cu_within(
        "live asset enters DrainOnly",
        lifecycle_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().1.assets[ASSET_INDEX as usize].lifecycle,
        AssetLifecycleV16::DrainOnly
    );

    let admin = env.admin.insecure_clone();
    let now_slot = env.svm.get_sysvar::<Clock>().slot;
    let market_id = env.asset_market_id(ASSET_INDEX);
    let retire_ix = ProgInstruction::UpdateAssetLifecycle {
        action: percolator_prog::processor::ASSET_ACTION_RETIRE,
        asset_index: ASSET_INDEX,
        market_id,
        authority_epoch: 0,
        now_slot,
        initial_price: 0,
        max_init_fee: u128::MAX,
        insurance_authority: admin.pubkey().to_bytes(),
        insurance_operator: admin.pubkey().to_bytes(),
        backing_bucket_authority: admin.pubkey().to_bytes(),
        oracle_authority: admin.pubkey().to_bytes(),
    };
    let market_before = env.svm.get_account(&env.market).unwrap();
    let account_a_before = env.svm.get_account(&account_a).unwrap();
    let account_b_before = env.svm.get_account(&account_b).unwrap();
    let matcher_before = matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let retire_error = env
        .send(
            retire_ix,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        )
        .expect_err("an asset with live exposure must not become Retired");
    assert!(
        retire_error.contains("Custom(21)") || retire_error.contains("custom program error: 0x15"),
        "{route:?} {open_side:?} nonempty retirement must reach EngineLockActive: {retire_error}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&account_a).unwrap(), account_a_before);
    assert_eq!(env.svm.get_account(&account_b).unwrap(), account_b_before);
    if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before) {
        assert_eq!(env.svm.get_account(&context).unwrap(), before);
    }
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let close_cu = try_adl_cross_zero_route_on_asset(
        &mut env,
        route,
        ASSET_INDEX,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        -signed_open_q,
        PRICE,
        matcher,
    )
    .expect("DrainOnly must preserve the same route's exact matched exit");
    assert_cu_within("pre-retirement exact exit", close_cu, TRADE_CU_LIMIT);
    let flat = env.market_state().1;
    let flat_asset = flat.assets[ASSET_INDEX as usize];
    assert_eq!(flat_asset.oi_eff_long_q, 0);
    assert_eq!(flat_asset.oi_eff_short_q, 0);
    assert_eq!(flat_asset.stored_pos_count_long, 0);
    assert_eq!(flat_asset.stored_pos_count_short, 0);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_a),
        ASSET_INDEX as usize
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_b),
        ASSET_INDEX as usize
    ));

    let retire_cu = env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        ASSET_INDEX,
        now_slot,
        0,
    );
    assert_cu_within("empty asset retirement", retire_cu, CUSTODY_CU_LIMIT);
    let retired = env.market_state().1;
    let retired_asset = retired.assets[ASSET_INDEX as usize];
    assert_eq!(retired_asset.lifecycle, AssetLifecycleV16::Retired);
    assert_eq!(retired_asset.oi_eff_long_q, 0);
    assert_eq!(retired_asset.oi_eff_short_q, 0);
    assert_eq!(retired_asset.stored_pos_count_long, 0);
    assert_eq!(retired_asset.stored_pos_count_short, 0);

    let market_before_reissue = env.svm.get_account(&env.market).unwrap();
    let account_a_before_reissue = env.svm.get_account(&account_a).unwrap();
    let account_b_before_reissue = env.svm.get_account(&account_b).unwrap();
    let matcher_before_reissue =
        matcher.map(|(_, context, _)| env.svm.get_account(&context).unwrap());
    let vault_before_reissue = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let reissue_error = try_adl_cross_zero_route_on_asset(
        &mut env,
        route,
        ASSET_INDEX,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        signed_open_q,
        PRICE,
        matcher,
    )
    .expect_err("a Retired asset must reject any fresh basis reissue");
    assert!(
        reissue_error.contains("Custom(21)")
            || reissue_error.contains("custom program error: 0x15"),
        "{route:?} {open_side:?} Retired reissue must reach EngineLockActive: {reissue_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_reissue
    );
    assert_eq!(
        env.svm.get_account(&account_a).unwrap(),
        account_a_before_reissue
    );
    assert_eq!(
        env.svm.get_account(&account_b).unwrap(),
        account_b_before_reissue
    );
    if let (Some((_, context, _)), Some(before)) = (matcher, matcher_before_reissue) {
        assert_eq!(env.svm.get_account(&context).unwrap(), before);
    }
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_reissue
    );
    for (owner, portfolio) in [(&owner_a, account_a), (&owner_b, account_b)] {
        env.svm.expire_blockhash();
        let (destination, withdraw_cu) = env.withdraw_with_cu(owner, portfolio, 1);
        assert_cu_within(
            "flat owner withdrawal after retirement",
            withdraw_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(env.token_amount(destination), 1);
    }
}

#[test]
fn v16_program_retired_exposure_is_publicly_unreachable_on_every_trade_route() {
    for route in AdlCrossZeroRoute::ALL {
        for open_side in [SideV16::Long, SideV16::Short] {
            run_retired_exposure_unreachability_world(route, open_side);
        }
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
