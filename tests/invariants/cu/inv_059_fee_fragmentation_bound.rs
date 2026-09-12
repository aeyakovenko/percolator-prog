//! INV-059 - Fee-fragmentation bound.
//!
//! Normative obligation: Splitting an execution or liquidation cannot multiply minimum or episode fees.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions):
//! `v16_attack_min_liquidation_fee_falls_back_to_full_close_progress` and
//! `v16_program_healthy_partial_liquidation_retries_cannot_multiply_fees`. The first proves an
//! inadmissible sub-minimum chunk becomes one full-close fee, while the second charges a real fee
//! on an engine-selected partial close and proves repeated public submissions against the same
//! healthy state cannot charge again or change custody. A second public campaign separates two
//! fee-bearing liquidations with a new authenticated mark and a fresh certified deficit across
//! single/batch CPI/no-CPI setup routes. It proves retries and malformed discovery cannot
//! manufacture episodes while genuine risk deterioration can, all route outcomes are identical,
//! and a fresh owner reduction remains live after the second episode. Execution fragmentation is
//! closed by INV-009's source-complete one-shot composition: a successful single-CPI partial
//! consumes both signed episodes, any residual requires a new signature, and batch CPI is exact
//! fill with aggregate slippage and fee caps.
//! A bounded minimum-fee history matrix compares aggregate owner reductions with two freshly
//! signed one-quantum reductions across changing transports, then one engine-selected full close.
//! Both signs and four minimum/cap profiles retain the same projected economics despite rejected
//! discovery and healthy retries; every world also reaches an exact funded owner withdrawal.
//! This is finite metamorphic coverage, not randomized history closure or caller-sized liquidation.
//! A separate sixteen-world INV-059/061 history interleaves owner deposits and insurance top-ups
//! with a proportional-fee partial liquidation, an authenticated reward tail, and two owner-exit
//! routes. An input-driven ledger separates principal, fees, rewards and SPL custody at each
//! prefix; wrong-owner reward tails and healthy retries frame exactly. Marked PnL remains zero.
//! A shrinkable funded-flat maintenance generator compares split and unsplit episodes under the
//! same authenticated schedule, crossing reward routes, error/retry placement and resolved close.
//! It checks prefix attribution and terminal payouts without claiming randomized liquidation F.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn liquidation_fee_oracle(
    closed_q: u128,
    price: u64,
    fee_bps: u64,
    min_fee: u128,
    fee_cap: u128,
) -> u128 {
    let fee_notional = closed_q
        .checked_mul(price as u128)
        .and_then(|value| value.checked_add(POS_SCALE as u128 - 1))
        .unwrap()
        / POS_SCALE as u128;
    let proportional = fee_notional
        .checked_mul(fee_bps as u128)
        .and_then(|value| value.checked_add(9_999))
        .unwrap()
        / 10_000;
    proportional.max(min_fee).min(fee_cap)
}

#[derive(Clone, Copy, Debug)]
enum RepeatedLiquidationRoute {
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
}

impl RepeatedLiquidationRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RepeatedLiquidationOutcome {
    first_closed_q: u128,
    first_fee: u128,
    second_closed_q: u128,
    second_fee: u128,
    owner_reduction_q: u128,
    remaining_long_oi_q: u128,
    remaining_short_oi_q: u128,
    long_capital: u128,
    short_capital: u128,
    insurance: u128,
    vault: u128,
}

#[allow(clippy::too_many_arguments)]
fn execute_repeated_liquidation_trade(
    env: &mut V16CuEnv,
    route: RepeatedLiquidationRoute,
    long_owner: &Keypair,
    long: Pubkey,
    short_owner: &Keypair,
    short: Pubkey,
    size_q: i128,
    price: u64,
) {
    env.svm.expire_blockhash();
    match route {
        RepeatedLiquidationRoute::TradeNoCpi => {
            env.trade_asset_with_cu(0, long_owner, long, short_owner, short, size_q, price, 0);
        }
        RepeatedLiquidationRoute::TradeCpi => {
            let (matcher_program, context, delegate) =
                auth_matcher_for_lp_via_system_create(env, short_owner, short);
            env.trade_cpi_with_cu_on_asset(
                long_owner,
                long,
                short_owner,
                short,
                matcher_program,
                context,
                delegate,
                0,
                size_q,
                0,
            );
        }
        RepeatedLiquidationRoute::BatchTradeNoCpi => {
            env.send(
                env.batch_trade_no_cpi_ix(
                    long,
                    short,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        exec_price: price,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(long_owner.pubkey(), true),
                    AccountMeta::new(short_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                    AccountMeta::new(short, false),
                ],
                &[long_owner, short_owner],
            )
            .expect("repeated-liquidation BatchTradeNoCpi");
        }
        RepeatedLiquidationRoute::BatchTradeCpi => {
            let (matcher_program, context, delegate) =
                auth_matcher_for_lp_via_system_create(env, short_owner, short);
            env.send(
                env.batch_trade_cpi_ix(
                    long,
                    short,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(long_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                    AccountMeta::new(short, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[long_owner],
            )
            .expect("repeated-liquidation BatchTradeCpi");
        }
    }
}

#[test]
fn v16_program_healthy_partial_liquidation_retries_cannot_multiply_fees() {
    const PRICE: u64 = 100;
    const LIQUIDATION_FEE_BPS: u64 = 100;
    const RETRIES: usize = 16;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        liquidation_fee_bps: LIQUIDATION_FEE_BPS,
        liquidation_fee_cap: 10,
        min_nonzero_mm_req: 10,
        min_nonzero_im_req: 20,
        max_price_move_bps_per_slot: 5_000,
        ..V16CuMarketParams::default()
    });
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 3_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        (10 * POS_SCALE) as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, PRICE * 3);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(4);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );

    let group_before = env.market_state().1;
    let position_before = active_leg_for_asset(&env.portfolio_state(short), 0)
        .basis_pos_q
        .unsigned_abs();
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit > 0,
        "the public setup must be liquidatable before the fee-bearing step"
    );

    env.svm.expire_blockhash();
    let liquidation_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("the current liquidatable account must take one engine-selected step");
    assert_cu_within(
        "INV-059 nonzero-fee partial liquidation",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );

    let group_after = env.market_state().1;
    let short_after = env.portfolio_state(short);
    let position_after = active_leg_for_asset(&short_after, 0)
        .basis_pos_q
        .unsigned_abs();
    let charged_fee = group_after.insurance - group_before.insurance;
    let closed_q = position_before - position_after;
    let expected_fee = liquidation_fee_oracle(
        closed_q,
        group_before.assets[0].effective_price,
        LIQUIDATION_FEE_BPS,
        0,
        10,
    );
    assert!(expected_fee > 0, "the control must derive a real fee");
    assert_eq!(
        charged_fee, expected_fee,
        "the selected close is charged exactly once by the independent fee oracle"
    );
    assert!(
        position_after > 0 && position_after < position_before,
        "the control must be a partial, not terminal, liquidation"
    );
    assert_eq!(
        health_cert(&short_after).certified_liq_deficit,
        0,
        "the engine-selected close must restore maintenance health"
    );
    assert_eq!(group_after.vault as u64, env.token_amount(env.vault));

    let market_fixed_point = env.svm.get_account(&env.market).unwrap();
    let portfolio_fixed_point = env.svm.get_account(&short).unwrap();
    let vault_fixed_point = env.svm.get_account(&env.vault).unwrap();
    for retry in 0..RETRIES {
        env.svm.expire_blockhash();
        let error = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 4,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[],
            )
            .expect_err("a healthy retry must return explicit NonProgress");
        assert!(
            error.contains("Custom(22)") || error.contains("custom program error: 0x16"),
            "healthy retry {retry} failed for the wrong reason: {error}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_fixed_point,
            "retry {retry} must not charge or redistribute another fee"
        );
        assert_eq!(
            env.svm.get_account(&short).unwrap(),
            portfolio_fixed_point,
            "retry {retry} must not fragment the selected close"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_fixed_point,
            "retry {retry} must not move custody"
        );
    }
}

fn run_new_liquidation_fee_episode(route: RepeatedLiquidationRoute) -> RepeatedLiquidationOutcome {
    const PRICE: u64 = 100;
    const LIQUIDATION_FEE_BPS: u64 = 100;
    const FEE_CAP: u128 = 10;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        liquidation_fee_bps: LIQUIDATION_FEE_BPS,
        liquidation_fee_cap: FEE_CAP,
        min_nonzero_mm_req: 10,
        min_nonzero_im_req: 20,
        max_price_move_bps_per_slot: 5_000,
        ..V16CuMarketParams::default()
    });
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 3_000);
    execute_repeated_liquidation_trade(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        (10 * POS_SCALE) as i128,
        PRICE,
    );
    let supply_before = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply;
    let spl_vault_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, PRICE * 3);
    for slot in 2..=4 {
        env.svm.warp_to_slot(slot);
        env.crank(
            short,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }

    let insurance_before_first = env.market_state().1.insurance;
    let first_position_before = active_leg_for_asset(&env.portfolio_state(short), 0)
        .basis_pos_q
        .unsigned_abs();
    env.svm.expire_blockhash();
    let first_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("first certified deficit must select liquidation");
    assert_cu_within(
        "INV-059 first liquidation episode",
        first_cu,
        CRANK_CU_LIMIT,
    );
    let first_group_after = env.market_state().1;
    let first_account_after = env.portfolio_state(short);
    let first_position_after = active_leg_for_asset(&first_account_after, 0)
        .basis_pos_q
        .unsigned_abs();
    let first_closed_q = first_position_before - first_position_after;
    let first_fee = liquidation_fee_oracle(
        first_closed_q,
        first_group_after.assets[0].effective_price,
        LIQUIDATION_FEE_BPS,
        0,
        FEE_CAP,
    );
    assert_eq!(
        first_group_after.insurance - insurance_before_first,
        first_fee
    );
    assert_eq!(health_cert(&first_account_after).certified_liq_deficit, 0);

    let first_fixed_market = env.svm.get_account(&env.market).unwrap();
    let first_fixed_account = env.svm.get_account(&short).unwrap();
    let first_fixed_vault = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[],
    )
    .expect_err("a retry cannot manufacture a second fee episode");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        first_fixed_market
    );
    assert_eq!(env.svm.get_account(&short).unwrap(), first_fixed_account);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), first_fixed_vault);

    // Only a new authenticated market move may make another liquidation actionable.
    env.svm.warp_to_slot(5);
    env.push_auth_mark_with_cu(5, 350);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(0),
        },
    );
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit > 0,
        "the second episode must begin with a new authenticated deficit"
    );

    let second_market_before = env.svm.get_account(&env.market).unwrap();
    let second_account_before = env.svm.get_account(&short).unwrap();
    let second_vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[],
    )
    .expect_err("malformed discovery input must not consume the second fee episode");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        second_market_before
    );
    assert_eq!(env.svm.get_account(&short).unwrap(), second_account_before);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        second_vault_before
    );

    let second_group_before = env.market_state().1;
    let second_position_before = active_leg_for_asset(&env.portfolio_state(short), 0)
        .basis_pos_q
        .unsigned_abs();
    env.svm.expire_blockhash();
    let second_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("the fresh authenticated deficit must select one new liquidation");
    assert_cu_within(
        "INV-059 second liquidation episode",
        second_cu,
        CRANK_CU_LIMIT,
    );
    let second_group_after = env.market_state().1;
    let second_account_after = env.portfolio_state(short);
    let second_position_after = active_leg_for_asset(&second_account_after, 0)
        .basis_pos_q
        .unsigned_abs();
    let second_closed_q = second_position_before - second_position_after;
    let second_fee = liquidation_fee_oracle(
        second_closed_q,
        second_group_before.assets[0].effective_price,
        LIQUIDATION_FEE_BPS,
        0,
        FEE_CAP,
    );
    assert_eq!(
        second_group_after.insurance - second_group_before.insurance,
        second_fee
    );
    assert_eq!(health_cert(&second_account_after).certified_liq_deficit, 0);
    assert_eq!(
        second_group_after.insurance - insurance_before_first,
        first_fee + second_fee,
        "only the two independently certified deficit episodes may charge fees"
    );
    assert_eq!(second_group_after.vault as u64, env.token_amount(env.vault));

    let owner_reduction_q = POS_SCALE as u128;
    assert!(
        second_position_after > owner_reduction_q,
        "{route:?}: repeated liquidation must leave a nontrivial owner-reduction control"
    );
    execute_repeated_liquidation_trade(
        &mut env,
        route,
        &long_owner,
        long,
        &short_owner,
        short,
        -(owner_reduction_q as i128),
        second_group_after.assets[0].effective_price,
    );
    let final_group = env.market_state().1;
    let final_short = env.portfolio_state(short);
    assert!(
        active_leg_for_asset(&final_short, 0)
            .basis_pos_q
            .unsigned_abs()
            < second_position_after,
        "{route:?}: fresh owner risk reduction must strictly reduce retained basis after episode two"
    );
    assert_eq!(
        final_group.assets[0].oi_eff_long_q,
        second_group_after.assets[0].oi_eff_long_q - owner_reduction_q,
        "{route:?}: long OI must follow the owner reduction"
    );
    assert_eq!(
        final_group.assets[0].oi_eff_short_q,
        second_group_after.assets[0].oi_eff_short_q - owner_reduction_q,
        "{route:?}: short OI must follow the owner reduction"
    );
    assert_eq!(env.token_amount(env.vault), spl_vault_before);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        supply_before
    );

    RepeatedLiquidationOutcome {
        first_closed_q,
        first_fee,
        second_closed_q,
        second_fee,
        owner_reduction_q,
        remaining_long_oi_q: final_group.assets[0].oi_eff_long_q,
        remaining_short_oi_q: final_group.assets[0].oi_eff_short_q,
        long_capital: env.portfolio_state(long).capital.get(),
        short_capital: final_short.capital.get(),
        insurance: final_group.insurance,
        vault: final_group.vault,
    }
}

#[test]
fn v16_program_new_liquidation_fee_episode_requires_new_authenticated_deficit() {
    let mut outcomes = Vec::new();
    for route in RepeatedLiquidationRoute::ALL {
        outcomes.push((route, run_new_liquidation_fee_episode(route)));
    }
    let expected = outcomes[0].1;
    for (route, outcome) in outcomes {
        assert_eq!(
            outcome, expected,
            "{route:?}: transport choice changed repeated-liquidation economics"
        );
    }
}

#[test]
fn v16_program_minimum_fee_episode_histories_match_aggregate_close() {
    const PRICE: u64 = 100;
    const CAPITAL: u128 = 1_000;
    const OPEN_Q: i128 = (10 * POS_SCALE) as i128;
    let mut worlds = 0;
    let mut rejections = 0;

    for direction in [-1i128, 1] {
        for (fee_bps, minimum, cap) in [(0, 1, 1), (0, 10, 10), (1, 10, 10), (1, 10, 11)] {
            assert!(liquidation_fee_oracle(OPEN_Q as u128, PRICE, fee_bps, 0, cap) < minimum);
            let mut aggregate = None;
            // World zero aggregates the owner reduction. The others rotate all four transports.
            for schedule in 0..=RepeatedLiquidationRoute::ALL.len() {
                let case = format!(
                    "direction={direction}, bps={fee_bps}, min={minimum}, cap={cap}, schedule={schedule}"
                );
                let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                    min_nonzero_mm_req: 100,
                    min_nonzero_im_req: 200,
                    liquidation_fee_bps: fee_bps,
                    liquidation_fee_cap: cap,
                    min_liquidation_abs: minimum,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                });
                env.configure_auth_mark_with_cu(0, PRICE);
                let owner = Keypair::new();
                let peer_owner = Keypair::new();
                let target = env.create_portfolio(&owner);
                let peer = env.create_portfolio(&peer_owner);
                let target_token = env.deposit(&owner, target, CAPITAL);
                let peer_token = env.deposit(&peer_owner, peer, 1_000_000);
                let staging_owner = Keypair::new();
                let staging = env.create_portfolio(&staging_owner);
                let custody = |env: &V16CuEnv| {
                    [env.vault, env.mint, target_token, peer_token]
                        .map(|key| env.svm.get_account(&key).unwrap())
                };
                let initial_custody = custody(&env);
                let initial_group = env.market_state().1;
                let mut expected_fees = 0u128;
                let assert_fees = |env: &V16CuEnv, fees: u128| {
                    let group = env.market_state().1;
                    assert!(fees <= cap, "{case}: cumulative configured episode cap");
                    assert_eq!(group.insurance, initial_group.insurance + fees, "{case}");
                    assert_eq!(group.c_tot, initial_group.c_tot - fees, "{case}");
                    assert_eq!(
                        env.portfolio_state(target).capital.get(),
                        CAPITAL - fees,
                        "{case}"
                    );
                    assert_eq!(env.portfolio_state(peer).capital.get(), 1_000_000, "{case}");
                    assert_eq!(env.portfolio_state(target).pnl.get(), 0, "{case}");
                    assert_eq!(env.portfolio_state(peer).pnl.get(), 0, "{case}");
                    assert_eq!(group.vault, initial_group.vault, "{case}");
                    assert_eq!(
                        group.vault,
                        u128::from(env.token_amount(env.vault)),
                        "{case}"
                    );
                    assert_eq!(custody(env), initial_custody, "{case}");
                };

                execute_repeated_liquidation_trade(
                    &mut env,
                    RepeatedLiquidationRoute::TradeNoCpi,
                    &owner,
                    target,
                    &peer_owner,
                    peer,
                    direction * OPEN_Q,
                    PRICE,
                );
                assert_fees(&env, expected_fees);
                let reductions = if schedule == 0 {
                    vec![2i128]
                } else {
                    vec![1, 1]
                };
                let mut remaining_q = OPEN_Q;
                for (step, quantity) in reductions.into_iter().enumerate() {
                    let route = if schedule == 0 {
                        RepeatedLiquidationRoute::TradeNoCpi
                    } else {
                        RepeatedLiquidationRoute::ALL[(schedule - 1 + step) % 4]
                    };
                    execute_repeated_liquidation_trade(
                        &mut env,
                        route,
                        &owner,
                        target,
                        &peer_owner,
                        peer,
                        -direction * quantity,
                        PRICE,
                    );
                    remaining_q -= quantity;
                    assert_eq!(
                        active_leg_for_asset(&env.portfolio_state(target), 0).basis_pos_q,
                        direction * remaining_q,
                        "{case}: each fresh signed reduction must execute exactly"
                    );
                    let group = env.market_state().1;
                    assert_eq!(group.assets[0].oi_eff_long_q, remaining_q as u128, "{case}");
                    assert_eq!(
                        group.assets[0].oi_eff_short_q, remaining_q as u128,
                        "{case}"
                    );
                    assert_fees(&env, expected_fees);
                }

                // Same-slot authenticated target lag changes risk, not marked PnL or funding.
                let target_price = if direction < 0 { PRICE * 2 } else { 1 };
                env.push_auth_mark_with_cu(0, target_price);
                assert_fees(&env, expected_fees);
                env.crank(
                    staging,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 0,
                        observations: crank_observations(0),
                    },
                );
                assert_fees(&env, expected_fees);
                let group = env.market_state().1;
                assert_eq!(group.assets[0].effective_price, PRICE, "{case}");
                assert_eq!(
                    group.assets[0].raw_oracle_target_price, target_price,
                    "{case}"
                );

                let metas = vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ];
                let send = |env: &mut V16CuEnv, observations| {
                    env.svm.expire_blockhash();
                    env.send(
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 0,
                            observations,
                        },
                        metas.clone(),
                        &[],
                    )
                };
                // Every crank writable except its network fee payer, plus unrelated accounts/custody.
                let frame = |env: &V16CuEnv| {
                    [
                        env.market,
                        target,
                        peer,
                        staging,
                        owner.pubkey(),
                        peer_owner.pubkey(),
                        env.vault,
                        env.mint,
                        target_token,
                        peer_token,
                    ]
                    .map(|key| env.svm.get_account(&key).unwrap())
                };
                for phase in 0..2 {
                    if schedule != 0 {
                        let before = frame(&env);
                        let error = send(&mut env, crank_observations_for_assets(&[0, 0]))
                            .expect_err("duplicate discovery must not consume a fee episode");
                        assert!(
                            error.contains(&format!(
                                "Custom({})",
                                PercolatorError::InvalidInstruction as u32
                            )),
                            "{case}: wrong discovery error: {error}"
                        );
                        assert_eq!(frame(&env), before, "{case}: phase={phase} exact rollback");
                        assert_fees(&env, expected_fees);
                        rejections += 1;
                    }
                    let cu = send(&mut env, crank_observations(0))
                        .unwrap_or_else(|error| panic!("{case}, phase={phase}: {error}"));
                    assert_cu_within("INV-059 minimum-fee episode history", cu, CRANK_CU_LIMIT);
                    let account = env.portfolio_state(target);
                    if phase == 0 {
                        assert_eq!(
                            active_leg_for_asset(&account, 0).basis_pos_q,
                            direction * remaining_q,
                            "{case}: refresh cannot close or charge"
                        );
                        assert!(health_cert(&account).certified_liq_deficit > 0, "{case}");
                    } else {
                        assert!(
                            !has_active_leg_for_asset(&account, 0),
                            "{case}: final residual close"
                        );
                        expected_fees = liquidation_fee_oracle(
                            remaining_q as u128,
                            PRICE,
                            fee_bps,
                            minimum,
                            cap,
                        );
                        assert_eq!(
                            expected_fees, minimum,
                            "{case}: nonzero minimum charged once"
                        );
                        assert!(expected_fees > 0, "{case}");
                        let group = env.market_state().1;
                        assert_eq!(group.assets[0].oi_eff_long_q, 0, "{case}");
                        assert_eq!(group.assets[0].oi_eff_short_q, 0, "{case}");
                    }
                    assert_fees(&env, expected_fees);
                }
                if schedule != 0 {
                    for _ in 0..3 {
                        let before = frame(&env);
                        let error = send(&mut env, vec![])
                            .expect_err("same-episode retries cannot collect another minimum");
                        assert!(
                            error.contains("Custom(22)")
                                || error.contains("custom program error: 0x16"),
                            "{case}: wrong retry error: {error}"
                        );
                        assert_eq!(frame(&env), before, "{case}: retry exact rollback");
                        assert_fees(&env, expected_fees);
                        rejections += 1;
                    }
                }

                let group = env.market_state().1;
                let outcome = (
                    remaining_q,
                    expected_fees,
                    group.insurance,
                    group.c_tot,
                    group.vault,
                    env.portfolio_state(target).capital.get(),
                    env.portfolio_state(peer).capital.get(),
                );
                if let Some(expected) = aggregate {
                    assert_eq!(
                        outcome, expected,
                        "{case}: split/route/retry history vs aggregate"
                    );
                } else {
                    aggregate = Some(outcome);
                }
                let withdraw = CAPITAL - expected_fees;
                let (destination, cu) = env.withdraw_with_cu(&owner, target, withdraw);
                assert_cu_within(
                    "INV-059 minimum-fee history withdrawal",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
                assert_eq!(env.token_amount(destination), withdraw as u64, "{case}");
                assert_eq!(env.portfolio_state(target).capital.get(), 0, "{case}");
                let after = env.market_state().1;
                assert_eq!(
                    after.insurance, group.insurance,
                    "{case}: withdrawal cannot recharge fees"
                );
                assert_eq!(after.vault, group.vault - withdraw, "{case}");
                assert_eq!(
                    after.vault,
                    u128::from(env.token_amount(env.vault)),
                    "{case}"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rejections), (40, 160));
}

#[test]
fn v16_program_liquidation_mixed_cashflows_preserve_reward_attribution_and_owner_exit() {
    const PRICE: u64 = 100;
    const CAPITAL: u128 = 1_000;
    const PEER_CAPITAL: u128 = 10_000;
    const KEEPER_CAPITAL: u128 = 23;
    const INSURANCE: u128 = 101;
    const OPEN_Q: u128 = 10 * POS_SCALE;
    const FEE_BPS: u64 = 137;
    const FEE_CAP: u128 = 100;
    const REWARD_BPS: u16 = 3_333;

    #[derive(Default)]
    struct Ledger {
        deposited: u128,
        topped_up: u128,
        fee: u128,
        reward: u128,
        owner_withdrawn: u128,
        keeper_withdrawn: u128,
    }

    let mut worlds = 0;
    let mut rejections = 0;
    let mut withdrawals = 0;
    let mut refreshes = 0;
    let mut max_crank_cu = 0;
    let mut max_exit_cu = 0;
    let mut max_custody_cu = 0;
    for direction in [-1i128, 1] {
        for bilateral_exit in [false, true] {
            for cashflows_before in [false, true] {
                for amount in [1u128, 17] {
                    let case = format!(
                        "direction={direction}, bilateral={bilateral_exit}, before={cashflows_before}, amount={amount}"
                    );
                    eprintln!("INV-059/061 mixed cashflows: {case}");
                    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                        min_nonzero_mm_req: 10,
                        min_nonzero_im_req: 20,
                        liquidation_fee_bps: FEE_BPS,
                        liquidation_fee_cap: FEE_CAP,
                        max_price_move_bps_per_slot: 500,
                        ..V16CuMarketParams::default()
                    });
                    env.update_liquidation_fee_policy_with_cu(REWARD_BPS);
                    env.configure_auth_mark_with_cu(0, PRICE);
                    let owner = Keypair::new();
                    let peer_owner = Keypair::new();
                    let keeper_owner = Keypair::new();
                    let target = env.create_portfolio(&owner);
                    let peer = env.create_portfolio(&peer_owner);
                    let keeper = env.create_portfolio(&keeper_owner);
                    env.deposit(&owner, target, CAPITAL + 2 * amount);
                    let peer_token = env.deposit(&peer_owner, peer, PEER_CAPITAL);
                    let keeper_token = env.deposit(&keeper_owner, keeper, KEEPER_CAPITAL);
                    env.top_up_insurance(INSURANCE + 3 * amount);
                    // Pre-fund the history through public withdrawals, then reuse these SPL accounts.
                    let owner_token = env.withdraw(&owner, target, 2 * amount);
                    let insurance_token = env.withdraw_insurance_with_cu(3 * amount).0;
                    env.trade_asset_with_cu(
                        0,
                        &owner,
                        target,
                        &peer_owner,
                        peer,
                        direction * OPEN_Q as i128,
                        PRICE,
                        0,
                    );

                    let mut ledger = Ledger::default();
                    let mint = env.svm.get_account(&env.mint).unwrap();
                    let total_tokens =
                        INSURANCE + CAPITAL + PEER_CAPITAL + KEEPER_CAPITAL + 5 * amount;
                    let assert_ledger = |env: &V16CuEnv, ledger: &Ledger| {
                        let group = env.market_state().1;
                        let capitals = [
                            CAPITAL + ledger.deposited - ledger.fee - ledger.owner_withdrawn,
                            PEER_CAPITAL,
                            KEEPER_CAPITAL + ledger.reward - ledger.keeper_withdrawn,
                        ];
                        for (portfolio, capital) in [target, peer, keeper].into_iter().zip(capitals)
                        {
                            let account = env.portfolio_state(portfolio);
                            assert_eq!(account.capital.get(), capital, "{case}: principal ledger");
                            assert_eq!(account.pnl.get(), 0, "{case}: no marked PnL attribution");
                        }
                        assert_eq!(group.c_tot, capitals.into_iter().sum::<u128>(), "{case}");
                        assert_eq!(
                            group.insurance,
                            INSURANCE + ledger.topped_up + ledger.fee - ledger.reward,
                            "{case}: external top-ups are not liquidation proceeds"
                        );
                        let vault = INSURANCE
                            + CAPITAL
                            + PEER_CAPITAL
                            + KEEPER_CAPITAL
                            + ledger.deposited
                            + ledger.topped_up
                            - ledger.owner_withdrawn
                            - ledger.keeper_withdrawn;
                        assert_eq!(group.vault, vault, "{case}: input-driven custody ledger");
                        assert_eq!(group.c_tot + group.insurance, vault, "{case}");
                        let balances = [
                            (env.vault, vault),
                            (
                                owner_token,
                                2 * amount - ledger.deposited + ledger.owner_withdrawn,
                            ),
                            (insurance_token, 3 * amount - ledger.topped_up),
                            (keeper_token, ledger.keeper_withdrawn),
                            (peer_token, 0),
                        ];
                        for (token, expected) in balances {
                            assert_eq!(u128::from(env.token_amount(token)), expected, "{case}");
                        }
                        assert_eq!(
                            balances
                                .into_iter()
                                .map(|(_, balance)| balance)
                                .sum::<u128>(),
                            total_tokens,
                            "{case}: all endowed SPL tokens remain attributed"
                        );
                        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint, "{case}");
                    };
                    // All crank writables except the network-fee payer, plus unrelated custody/owners.
                    let frame = |env: &V16CuEnv| {
                        [
                            env.market,
                            target,
                            peer,
                            keeper,
                            owner.pubkey(),
                            peer_owner.pubkey(),
                            keeper_owner.pubkey(),
                            env.admin.pubkey(),
                            env.vault,
                            env.mint,
                            owner_token,
                            peer_token,
                            keeper_token,
                            insurance_token,
                        ]
                        .map(|key| env.svm.get_account(&key).unwrap())
                    };
                    let custody = |env: &V16CuEnv| {
                        [
                            env.vault,
                            env.mint,
                            owner_token,
                            peer_token,
                            keeper_token,
                            insurance_token,
                        ]
                        .map(|key| env.svm.get_account(&key).unwrap())
                    };
                    let crank = |env: &mut V16CuEnv, signer: &Keypair, observations| {
                        env.svm.expire_blockhash();
                        env.send(
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 0,
                                observations,
                            },
                            vec![
                                AccountMeta::new(signer.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(target, false),
                                AccountMeta::new(keeper, false),
                            ],
                            &[signer],
                        )
                    };
                    assert_ledger(&env, &ledger);
                    env.push_auth_mark_with_cu(0, if direction < 0 { PRICE * 2 } else { 1 });
                    assert_ledger(&env, &ledger);
                    let cu = crank(&mut env, &keeper_owner, crank_observations(0))
                        .expect("authenticated deficit refresh");
                    assert_cu_within("INV-059/061 mixed-flow refresh", cu, CRANK_CU_LIMIT);
                    max_crank_cu = max_crank_cu.max(cu);
                    refreshes += 1;
                    assert_eq!(
                        env.market_state().1.assets[0].effective_price,
                        PRICE,
                        "{case}"
                    );
                    assert_eq!(
                        active_leg_for_asset(&env.portfolio_state(target), 0).basis_pos_q,
                        direction * OPEN_Q as i128,
                        "{case}: refresh cannot liquidate"
                    );
                    assert!(health_cert(&env.portfolio_state(target)).certified_liq_deficit > 0);
                    assert_ledger(&env, &ledger);

                    let before = frame(&env);
                    let error = crank(&mut env, &owner, crank_observations(0))
                        .expect_err("a different owner's valid reward portfolio must reject");
                    assert!(
                        error
                            .contains(&format!("Custom({})", PercolatorError::Unauthorized as u32)),
                        "{case}: wrong reward-tail error: {error}"
                    );
                    assert_eq!(frame(&env), before, "{case}: reward-tail exact rollback");
                    assert_ledger(&env, &ledger);
                    rejections += 1;

                    for before_liquidation in [true, false] {
                        if cashflows_before == before_liquidation {
                            env.svm.expire_blockhash();
                            let cu = env
                                .send(
                                    env.deposit_ix(target, amount),
                                    vec![
                                        AccountMeta::new(owner.pubkey(), true),
                                        AccountMeta::new(env.market, false),
                                        AccountMeta::new(target, false),
                                        AccountMeta::new(owner_token, false),
                                        AccountMeta::new(env.vault, false),
                                        AccountMeta::new_readonly(spl_token::ID, false),
                                    ],
                                    &[&owner],
                                )
                                .expect("public owner re-deposit around liquidation");
                            assert_cu_within(
                                "INV-059/061 mixed-flow deposit",
                                cu,
                                CUSTODY_CU_LIMIT,
                            );
                            max_custody_cu = max_custody_cu.max(cu);
                            ledger.deposited += amount;
                            assert_ledger(&env, &ledger);
                            let cu = env.top_up_insurance_from_admin_token_with_cu(
                                insurance_token,
                                3 * amount,
                            );
                            assert_cu_within("INV-059/061 mixed-flow top-up", cu, CUSTODY_CU_LIMIT);
                            max_custody_cu = max_custody_cu.max(cu);
                            ledger.topped_up += 3 * amount;
                            assert_ledger(&env, &ledger);
                        }
                        if !before_liquidation {
                            continue;
                        }
                        // A deposit may invalidate the certificate; allow at most one re-refresh.
                        for step in 0..2 {
                            let custody_before = custody(&env);
                            let peer_before = env.svm.get_account(&peer).unwrap();
                            let cu = crank(&mut env, &keeper_owner, crank_observations(0))
                                .unwrap_or_else(|error| {
                                    panic!("{case}: crank step={step}: {error}")
                                });
                            assert_cu_within(
                                "INV-059/061 mixed-flow liquidation",
                                cu,
                                CRANK_CU_LIMIT,
                            );
                            max_crank_cu = max_crank_cu.max(cu);
                            assert_eq!(custody(&env), custody_before, "{case}: crank SPL frame");
                            assert_eq!(env.svm.get_account(&peer).unwrap(), peer_before, "{case}");
                            let account = env.portfolio_state(target);
                            let remaining_q =
                                active_leg_for_asset(&account, 0).basis_pos_q.unsigned_abs();
                            if remaining_q == OPEN_Q {
                                assert_eq!(step, 0, "{case}: bounded recertification");
                                assert!(health_cert(&account).certified_liq_deficit > 0, "{case}");
                                refreshes += 1;
                                assert_ledger(&env, &ledger);
                                continue;
                            }
                            assert!(
                                remaining_q > 0 && remaining_q < OPEN_Q,
                                "{case}: partial close"
                            );
                            let closed_q = OPEN_Q - remaining_q;
                            ledger.fee =
                                liquidation_fee_oracle(closed_q, PRICE, FEE_BPS, 0, FEE_CAP);
                            ledger.reward = ledger.fee * u128::from(REWARD_BPS) / 10_000;
                            assert!(ledger.reward > 0 && ledger.reward < ledger.fee, "{case}");
                            assert!(
                                ledger.fee < FEE_CAP,
                                "{case}: proportional, not minimum/cap dominated"
                            );
                            assert_ne!(ledger.fee * u128::from(REWARD_BPS) % 10_000, 0, "{case}");
                            assert_eq!(health_cert(&account).certified_liq_deficit, 0, "{case}");
                            let group = env.market_state().1;
                            assert_eq!(group.assets[0].oi_eff_long_q, remaining_q, "{case}");
                            assert_eq!(group.assets[0].oi_eff_short_q, remaining_q, "{case}");
                            assert_ledger(&env, &ledger);
                            eprintln!(
                                "  closed_q={closed_q}, fee={}, reward={}, liquidation_cu={cu}",
                                ledger.fee, ledger.reward
                            );
                            break;
                        }
                        assert!(ledger.fee > 0, "{case}: the bounded history must liquidate");
                        let before = frame(&env);
                        let error = crank(&mut env, &keeper_owner, vec![])
                            .expect_err("a healthy retry cannot pay the reward twice");
                        assert!(
                            error.contains(&format!(
                                "Custom({})",
                                PercolatorError::EngineNonProgress as u32
                            )),
                            "{case}: wrong healthy-retry error: {error}"
                        );
                        assert_eq!(frame(&env), before, "{case}: rewarded retry exact rollback");
                        assert_ledger(&env, &ledger);
                        rejections += 1;
                    }

                    let remaining_q = active_leg_for_asset(&env.portfolio_state(target), 0)
                        .basis_pos_q
                        .unsigned_abs();
                    let custody_before = custody(&env);
                    env.svm.expire_blockhash();
                    let cu = if bilateral_exit {
                        env.trade_asset_with_cu(
                            0,
                            &owner,
                            target,
                            &peer_owner,
                            peer,
                            -direction * remaining_q as i128,
                            PRICE,
                            0,
                        )
                    } else {
                        env.rebalance_reduce_with_cu(&owner, target, 0, remaining_q)
                    };
                    assert_cu_within(
                        "INV-059/061 post-progress owner reduction",
                        cu,
                        TRADE_CU_LIMIT,
                    );
                    max_exit_cu = max_exit_cu.max(cu);
                    assert!(
                        !has_active_leg_for_asset(&env.portfolio_state(target), 0),
                        "{case}"
                    );
                    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0, "{case}");
                    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 0, "{case}");
                    assert_eq!(
                        custody(&env),
                        custody_before,
                        "{case}: owner reduction SPL frame"
                    );
                    assert_ledger(&env, &ledger);

                    for (signer, portfolio, destination, payout) in [
                        (
                            &keeper_owner,
                            keeper,
                            keeper_token,
                            KEEPER_CAPITAL + ledger.reward,
                        ),
                        (&owner, target, owner_token, amount),
                        (&owner, target, owner_token, CAPITAL - ledger.fee),
                    ] {
                        env.svm.expire_blockhash();
                        let cu = env
                            .send(
                                env.withdraw_ix(portfolio, payout),
                                vec![
                                    AccountMeta::new(signer.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolio, false),
                                    AccountMeta::new(destination, false),
                                    AccountMeta::new(env.vault, false),
                                    AccountMeta::new_readonly(env.vault_authority, false),
                                    AccountMeta::new_readonly(spl_token::ID, false),
                                ],
                                &[signer],
                            )
                            .unwrap_or_else(|error| panic!("{case}: funded owner exit: {error}"));
                        assert_cu_within(
                            "INV-059/061 funded mixed-flow exit",
                            cu,
                            CUSTODY_CU_LIMIT,
                        );
                        max_custody_cu = max_custody_cu.max(cu);
                        if portfolio == keeper {
                            ledger.keeper_withdrawn += payout;
                        } else {
                            ledger.owner_withdrawn += payout;
                        }
                        assert_ledger(&env, &ledger);
                        withdrawals += 1;
                    }
                    let custody_before = custody(&env);
                    for (signer, portfolio) in [(&owner, target), (&keeper_owner, keeper)] {
                        let rent = env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
                        let count = env.market_state().1.materialized_portfolio_count;
                        let cu = env.close_portfolio_with_cu(signer, portfolio);
                        assert_cu_within("INV-059/061 empty owner close", cu, CUSTODY_CU_LIMIT);
                        max_custody_cu = max_custody_cu.max(cu);
                        assert_eq!(
                            env.market_state().1.materialized_portfolio_count,
                            count - 1,
                            "{case}"
                        );
                        assert_eq!(
                            env.svm.get_account(&env.market).unwrap().lamports,
                            market_lamports + rent,
                            "{case}"
                        );
                        if let Some(closed) = env.svm.get_account(&portfolio) {
                            assert_eq!(closed.lamports, 0, "{case}");
                            assert!(
                                closed.data.is_empty() || !state::is_initialized(&closed.data),
                                "{case}"
                            );
                        }
                    }
                    assert_eq!(custody(&env), custody_before, "{case}: close SPL frame");
                    let group = env.market_state().1;
                    assert_eq!(group.c_tot, PEER_CAPITAL, "{case}");
                    assert_eq!(
                        group.insurance,
                        INSURANCE + 3 * amount + ledger.fee - ledger.reward,
                        "{case}"
                    );
                    assert_eq!(group.vault, group.c_tot + group.insurance, "{case}");
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, rejections, withdrawals), (16, 32, 48));
    assert!((16..=24).contains(&refreshes));
    eprintln!("INV-059/061 mixed cashflows: worlds={worlds}, liquidations={worlds}, refreshes={refreshes}, rejections={rejections}, owner_reductions={worlds}, withdrawals={withdrawals}, closes={}, max_crank_cu={max_crank_cu}, max_exit_cu={max_exit_cu}, max_custody_cu={max_custody_cu}", 2 * worlds);
}

#[test]
fn v16_program_liquidation_fee_surface_is_single_route_and_engine_selected() {
    const PRODUCTION_SOURCE: &str = include_str!("../../../src/v16_program.rs");
    const CALLER_INPUT_ROSTER: &str = include_str!("../inv_023_caller_input_roster.tsv");

    let production = PRODUCTION_SOURCE
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    assert_eq!(
        production.matches("AutoCrankPlanV16::Liquidate").count(),
        3,
        "a new liquidation dispatch or post-processing branch requires fee-episode review"
    );
    assert_eq!(
        production.matches("LiquidationRequestV16").count(),
        0,
        "the wrapper must not construct a caller-sized liquidation request"
    );
    for forbidden_variant in ["Liquidate {", "LiquidatePosition", "LiquidateAccount"] {
        assert!(
            !production.contains(&format!("Self::{forbidden_variant}")),
            "a direct liquidation instruction reopens the fragmentation surface"
        );
    }
    assert!(CALLER_INPUT_ROSTER.contains("PermissionlessCrank\tobservations\tDISCOVERY_HINT\t"));
    assert!(CALLER_INPUT_ROSTER
        .contains("CrankObservationHint\tasset_index,oracle_accounts\tDISCOVERY_HINT\t"));
    assert!(
        !CALLER_INPUT_ROSTER.contains("PermissionlessCrank.close"),
        "caller-selected close quantity would make liquidation partitioning public"
    );
    let one_shot = include_str!("inv_009_partial_fill_and_retry_accounting.rs");
    assert!(
        one_shot.contains("fn v16_program_one_shot_trade_consent_composition_is_source_complete(")
    );
    let aggregate = include_str!("inv_011_signed_aggregate_economic_bounds.rs");
    assert!(aggregate.contains(
        "fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically("
    ));
    crate::assert_certified_engine_pin("INV-059 engine-selected liquidation evidence");
}

#[test]
fn v16_attack_min_liquidation_fee_falls_back_to_full_close_progress() {
    const PRICE: u64 = 100;
    const MIN_FEE: u128 = 10;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        min_nonzero_mm_req: 100,
        min_nonzero_im_req: 200,
        liquidation_fee_bps: 0,
        liquidation_fee_cap: MIN_FEE,
        min_liquidation_abs: MIN_FEE,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 10 * PRICE as u128);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        (10 * POS_SCALE) as i128,
        PRICE,
        0,
    );

    // Same-slot target-only lag makes the short liquidatable without first changing its marked PnL.
    // A separate public crank commits the authenticated target while dt=0 keeps effective_price at
    // PRICE, matching the production out-of-order keeper flow.
    let staging_owner = Keypair::new();
    let staging = env.create_portfolio(&staging_owner);
    env.push_auth_mark_with_cu(0, PRICE * 2);
    env.crank(
        staging,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let (_, before_group) = env.market_state();
    let before_short = env.portfolio_state(short);
    assert_eq!(before_group.assets[0].effective_price, PRICE);
    assert_eq!(before_group.assets[0].raw_oracle_target_price, PRICE * 2);
    assert_eq!(before_short.pnl.get(), 0);
    assert!(
        health_cert(&before_short).cert_oracle_epoch < before_group.oracle_epoch,
        "target-only lag makes the victim certificate stale"
    );
    let insurance_before = before_group.insurance;

    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("the first auto-crank refreshes the target-lagged account");
    assert_cu_within(
        "minimum-fee liquidation pre-refresh",
        refresh_cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        has_active_leg_for_asset(&env.portfolio_state(short), 0),
        "the first selected step is a refresh, not the liquidation under test"
    );

    env.svm.expire_blockhash();
    let liquidation_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("minimum-fee liquidation must fall back to a full-close progress step");
    assert_cu_within(
        "minimum-fee full-close liquidation fallback",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );

    let short_after = env.portfolio_state(short);
    let (_, after_group) = env.market_state();
    assert!(
        !has_active_leg_for_asset(&short_after, 0),
        "the inadmissible partial chunk falls back to closing the selected leg"
    );
    assert_eq!(
        short_after.capital.get(),
        10 * PRICE as u128 - MIN_FEE,
        "the configured full-close minimum fee is charged exactly once"
    );
    assert_eq!(
        after_group.insurance - insurance_before,
        MIN_FEE,
        "the collected minimum fee remains conserved in insurance"
    );
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
}

// LOF: the batch's single end-state initial-margin check protects the COUNTERPARTY too. A funded
// taker cannot use a batch to force an undercapitalized LP into a position it cannot margin — the
// security.md sweep - BatchTradeCpi zero-fill atomicity (#22/#39): batch strategies require every
// leg to fill. A zero-capacity matcher returning exec_size=0 must reject the whole batch, not create
// security.md sweep - stale-resolve BatchTradeCpi rollback (#30/#35/#48): the batch CPI path invokes
// the matcher before it reaches the shared batch engine pre-pass that freezes stale-matured markets.
// Once the oracle is past permissionless_resolve_stale_slots, a batched matcher fill must reject and
// CU/DoS hardening: stale-resolve-matured BatchTradeCpi must fail before the external matcher CPI.
// The older rollback test proves protocol/matcher writes are reverted after a stale rejection. This
// uses a hostile over-fill matcher as a sentinel: while fresh, that exact route reaches matcher-return
// validation and fails as InvalidAccountData; once stale, the wrapper must return OracleStale first.
#[test]
fn v16_attack_batch_tradecpi_stale_rejects_before_hostile_matcher_cpi() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &la,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, la, ctx, delegate, 1);

    let sz = (5 * POS_SCALE) as i128;
    let legs = vec![
        BatchTradeCpiLeg {
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        },
        BatchTradeCpiLeg {
            asset_index: 1,
            market_id: first_generation_market_id((1) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        },
    ];
    let accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };
    let set_hostile_mode = |env: &mut V16CuEnv, mode: u8| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = mode;
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    };

    env.svm.warp_to_slot(4);
    set_hostile_mode(&mut env, 0);
    env.svm.expire_blockhash();
    let fresh_err = env
        .send(
            env.batch_trade_cpi_ix(ta, la, legs.clone()),
            accounts(&env),
            &[&taker],
        )
        .expect_err("fresh hostile over-fill must reach matcher-return validation");
    assert!(
        fresh_err.contains("InvalidAccountData"),
        "fresh hostile over-fill should fail from matcher-return validation, got {fresh_err}"
    );
    assert!(
        !fresh_err.contains("Custom(27)") && !fresh_err.contains("0x1b"),
        "fresh hostile over-fill must not be mistaken for stale gating: {fresh_err}"
    );

    env.svm.warp_to_slot(40);
    set_hostile_mode(&mut env, 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale_err = env
        .send(
            env.batch_trade_cpi_ix(ta, la, legs),
            accounts(&env),
            &[&taker],
        )
        .expect_err("stale BatchTradeCpi must reject before matcher CPI");
    assert!(
        stale_err.contains("Custom(27)") || stale_err.contains("0x1b"),
        "stale BatchTradeCpi must fail as OracleStale before hostile matcher validation, got {stale_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "pre-CPI stale rejection leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ta).unwrap(),
        taker_before,
        "pre-CPI stale rejection leaves taker bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&la).unwrap(),
        lp_before,
        "pre-CPI stale rejection leaves LP bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "pre-CPI stale rejection never gives the hostile matcher a writable context"
    );
}

// CU/DoS hardening: active-stale portfolios at or above the wrapper currentness threshold must
// reject before matcher CPI too. Stale-resolve-matured preflight is covered above; this covers the
// account-local EngineStale gate that prevents a doomed 8+ active-leg trade from invoking a hostile
// matcher first.
#[test]
fn v16_attack_tradecpi_active_stale_rejects_before_hostile_matcher_cpi() {
    {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(8, 1_000, 1_000, 500);
        let hostile = Pubkey::new_unique();
        env.svm.add_program(
            hostile,
            &std::fs::read(hostile_matcher_program_path()).unwrap(),
        );
        let taker = Keypair::new();
        let lp = Keypair::new();
        let ta = env.create_portfolio(&taker);
        let la = env.create_portfolio(&lp);
        env.deposit(&taker, ta, 100_000);
        env.deposit(&lp, la, 100_000);
        env.seed_current_n_leg_position_for_benchmark(ta, la, 8);

        let ctx = Pubkey::new_unique();
        let delegate = matcher_delegate_key(
            &env.program_id,
            &env.market,
            &la,
            &lp.pubkey(),
            &hostile,
            &ctx,
        );
        env.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.set_matcher_config(hostile, &lp, la, ctx, delegate, 1);

        let set_hostile_mode = |env: &mut V16CuEnv| {
            let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
            data[0] = 0; // hostile over-fill mode: if CPI occurs, validation fails.
            env.svm
                .set_account(
                    ctx,
                    Account {
                        lamports: 1_000_000_000,
                        data,
                        owner: hostile,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
        };
        let accounts = |env: &V16CuEnv| {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ]
        };

        set_hostile_mode(&mut env);
        env.svm.expire_blockhash();
        let fresh_err = env
            .send(
                env.trade_cpi_ix(ta, la, 0, -(POS_SCALE as i128), 0, 0),
                accounts(&env),
                &[&taker],
            )
            .expect_err("fresh active-current sentinel control should reach matcher validation");
        assert!(
            fresh_err.contains("InvalidAccountData"),
            "fresh active-current TradeCpi should fail from hostile matcher validation, got {fresh_err}"
        );

        env.svm.warp_to_slot(16);
        env.mutate_market(|_, group| {
            for asset_index in 0..8usize {
                group
                    .accrue_asset_to_not_atomic(asset_index, 16, 95, 0, true)
                    .unwrap();
                group.assets[asset_index].raw_oracle_target_price = 95;
            }
        });
        set_hostile_mode(&mut env);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&ta).unwrap();
        let lp_before = env.svm.get_account(&la).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let stale_err = env
            .send(
                env.trade_cpi_ix(ta, la, 0, -(POS_SCALE as i128), 0, 0),
                accounts(&env),
                &[&taker],
            )
            .expect_err("8-leg active-stale TradeCpi must reject before matcher CPI");
        assert!(
            stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
            "active-stale TradeCpi must fail as EngineStale before hostile matcher validation, got {stale_err}"
        );
        assert!(
            !stale_err.contains("InvalidAccountData"),
            "active-stale TradeCpi must not reach hostile matcher validation: {stale_err}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
        assert_eq!(
            env.svm.get_account(&ctx).unwrap(),
            ctx_before,
            "active-stale TradeCpi rejection never gives the hostile matcher a writable context"
        );
    }

    {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(8, 1_000, 1_000, 500);
        let hostile = Pubkey::new_unique();
        env.svm.add_program(
            hostile,
            &std::fs::read(hostile_matcher_program_path()).unwrap(),
        );
        let taker = Keypair::new();
        let lp = Keypair::new();
        let ta = env.create_portfolio(&taker);
        let la = env.create_portfolio(&lp);
        env.deposit(&taker, ta, 100_000);
        env.deposit(&lp, la, 100_000);
        env.seed_current_n_leg_position_for_benchmark(ta, la, 8);

        let ctx = Pubkey::new_unique();
        let delegate = matcher_delegate_key(
            &env.program_id,
            &env.market,
            &la,
            &lp.pubkey(),
            &hostile,
            &ctx,
        );
        env.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.set_matcher_config(hostile, &lp, la, ctx, delegate, 1);

        let set_hostile_mode = |env: &mut V16CuEnv| {
            let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
            data[0] = 0; // hostile over-fill mode: if CPI occurs, validation fails.
            env.svm
                .set_account(
                    ctx,
                    Account {
                        lamports: 1_000_000_000,
                        data,
                        owner: hostile,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
        };
        let legs = vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: -(POS_SCALE as i128),
                fee_bps: 0,
                limit_price: 0,
            },
            BatchTradeCpiLeg {
                asset_index: 1,
                market_id: first_generation_market_id((1) as u16),
                size_q: -(POS_SCALE as i128),
                fee_bps: 0,
                limit_price: 0,
            },
        ];
        let accounts = |env: &V16CuEnv| {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ]
        };

        set_hostile_mode(&mut env);
        env.svm.expire_blockhash();
        let fresh_err = env
            .send(
                env.batch_trade_cpi_ix(ta, la, legs.clone()),
                accounts(&env),
                &[&taker],
            )
            .expect_err("fresh active-current BatchTradeCpi should reach matcher validation");
        assert!(
            fresh_err.contains("InvalidAccountData"),
            "fresh active-current BatchTradeCpi should fail from hostile matcher validation, got {fresh_err}"
        );

        env.svm.warp_to_slot(16);
        env.mutate_market(|_, group| {
            for asset_index in 0..8usize {
                group
                    .accrue_asset_to_not_atomic(asset_index, 16, 95, 0, true)
                    .unwrap();
                group.assets[asset_index].raw_oracle_target_price = 95;
            }
        });
        set_hostile_mode(&mut env);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&ta).unwrap();
        let lp_before = env.svm.get_account(&la).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let stale_err = env
            .send(
                env.batch_trade_cpi_ix(ta, la, legs),
                accounts(&env),
                &[&taker],
            )
            .expect_err("8-leg active-stale BatchTradeCpi must reject before matcher CPI");
        assert!(
            stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
            "active-stale BatchTradeCpi must fail as EngineStale before hostile matcher validation, got {stale_err}"
        );
        assert!(
            !stale_err.contains("InvalidAccountData"),
            "active-stale BatchTradeCpi must not reach hostile matcher validation: {stale_err}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
        assert_eq!(
            env.svm.get_account(&ctx).unwrap(),
            ctx_before,
            "active-stale BatchTradeCpi rejection never gives the hostile matcher a writable context"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum MaintenanceRewardRoute {
    Insurance,
    SelfReward,
    Keeper(usize),
}

#[derive(Clone, Debug)]
struct MaintenanceFeeFragment {
    slots: u64,
    route: MaintenanceRewardRoute,
    retries: u8,
    abort_after_fee: Option<bool>,
}

fn maintenance_reward_route() -> impl Strategy<Value = MaintenanceRewardRoute> {
    prop_oneof![
        Just(MaintenanceRewardRoute::Insurance),
        Just(MaintenanceRewardRoute::SelfReward),
        Just(MaintenanceRewardRoute::Keeper(0)),
        Just(MaintenanceRewardRoute::Keeper(1)),
    ]
}

fn maintenance_fee_fragment() -> impl Strategy<Value = MaintenanceFeeFragment> {
    (
        1u64..=31,
        maintenance_reward_route(),
        0u8..=2,
        proptest::option::of(any::<bool>()),
    )
        .prop_map(
            |(slots, route, retries, abort_after_fee)| MaintenanceFeeFragment {
                slots,
                route,
                retries,
                abort_after_fee,
            },
        )
}

#[derive(Default)]
struct MaintenanceFeeOracle {
    gross: u128,
    retained: u128,
    self_reward: u128,
    keeper_rewards: [u128; 2],
    rewarded_gross: u128,
    rewarded_fragments: u128,
}

impl MaintenanceFeeOracle {
    // Inputs come only from the generated authenticated schedule and public policy.
    // Do not infer charges from account deltas or call engine fee helpers here.
    fn charge(&mut self, slots: u64, rate: u128, share: u16, route: MaintenanceRewardRoute) {
        let gross = u128::from(slots) * rate;
        let reward = match route {
            MaintenanceRewardRoute::Insurance => 0,
            _ => gross * u128::from(share) / 10_000,
        };
        self.gross += gross;
        self.retained += gross - reward;
        match route {
            MaintenanceRewardRoute::Insurance => (),
            MaintenanceRewardRoute::SelfReward => self.self_reward += reward,
            MaintenanceRewardRoute::Keeper(index) => self.keeper_rewards[index] += reward,
        }
        if !matches!(route, MaintenanceRewardRoute::Insurance) {
            self.rewarded_gross += gross;
            self.rewarded_fragments += 1;
        }
    }

    fn rewards(&self) -> u128 {
        self.self_reward + self.keeper_rewards.iter().sum::<u128>()
    }
}

struct MaintenanceFeeEpisode {
    env: V16CuEnv,
    owner: Keypair,
    split: Pubkey,
    whole: Pubkey,
    keepers: [Pubkey; 2],
    split_dest: Pubkey,
    whole_dest: Pubkey,
    bad_dest: Pubkey,
    deposit: u128,
    stationary_accounts: [(Pubkey, Account); 7],
}

impl MaintenanceFeeEpisode {
    fn new(rate: u128, share: u16, deposit: u128) -> Self {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            maintenance_fee_per_slot: rate,
            ..V16CuMarketParams::default()
        });
        env.update_maintenance_fee_policy_with_cu(share);
        let owner = Keypair::new();
        let split = env.create_portfolio(&owner);
        let whole = env.create_portfolio(&owner);
        let keeper_owners = [Keypair::new(), Keypair::new()];
        let keepers = keeper_owners
            .each_ref()
            .map(|owner| env.create_portfolio(owner));
        let sources = [
            env.deposit(&owner, split, deposit),
            env.deposit(&owner, whole, deposit),
        ];
        let split_dest = env.token_account(owner.pubkey(), 0);
        let whole_dest = env.token_account(owner.pubkey(), 0);
        let bad_dest = env.token_account(Pubkey::new_unique(), 0);
        let stationary_accounts = [
            env.mint,
            owner.pubkey(),
            env.admin.pubkey(),
            keeper_owners[0].pubkey(),
            keeper_owners[1].pubkey(),
            sources[0],
            sources[1],
        ]
        .map(|key| (key, env.svm.get_account(&key).unwrap()));
        assert_eq!(env.portfolio_state(split).last_fee_slot.get(), 0);
        assert_eq!(env.portfolio_state(whole).last_fee_slot.get(), 0);
        Self {
            env,
            owner,
            split,
            whole,
            keepers,
            split_dest,
            whole_dest,
            bad_dest,
            deposit,
            stationary_accounts,
        }
    }

    fn snapshot(&self) -> Vec<Option<Account>> {
        [
            self.env.market,
            self.split,
            self.whole,
            self.keepers[0],
            self.keepers[1],
            self.env.vault,
            self.split_dest,
            self.whole_dest,
            self.bad_dest,
        ]
        .into_iter()
        .chain(self.stationary_accounts.iter().map(|(key, _)| *key))
        .map(|key| self.env.svm.get_account(&key))
        .collect()
    }

    fn sync_instruction(&self, route: MaintenanceRewardRoute, slot_hint: u64) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.split, false),
        ];
        match route {
            MaintenanceRewardRoute::Insurance => (),
            MaintenanceRewardRoute::SelfReward => {
                accounts.push(AccountMeta::new(self.split, false))
            }
            MaintenanceRewardRoute::Keeper(index) => {
                accounts.push(AccountMeta::new(self.keepers[index], false));
            }
        }
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: ProgInstruction::SyncMaintenanceFee {
                now_slot: slot_hint,
            }
            .encode(),
        }
    }

    fn sync(
        &mut self,
        route: MaintenanceRewardRoute,
        slot_hint: u64,
        abort_after_fee: Option<bool>,
    ) {
        let mut instructions = vec![heap_ix(), cu_ix()];
        let fee = self.sync_instruction(route, slot_hint);
        let sequences = self.env.control_sequences(0);
        // A known-invalid public policy update makes either ordering abort. When
        // last, it checks transaction rollback after an otherwise payable fee.
        let invalid = Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.env.admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
            ],
            data: ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps: 10_001,
                policy_sequence: next_control_sequence(sequences.maintenance_fee),
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        };
        match abort_after_fee {
            Some(false) => instructions.extend([invalid, fee]),
            Some(true) => instructions.extend([fee, invalid]),
            None => instructions.push(fee),
        }
        let before = self.snapshot();
        self.env.svm.expire_blockhash();
        let mut signers = vec![&self.env.payer];
        if abort_after_fee.is_some() {
            signers.push(&self.env.admin);
        }
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        let result = self.env.svm.send_transaction(tx);
        if let Some(after) = abort_after_fee {
            let error = result.expect_err("injected invalid policy must abort the transaction");
            assert_eq!(
                error.err,
                solana_sdk::transaction::TransactionError::InstructionError(
                    if after { 3 } else { 2 },
                    solana_sdk::instruction::InstructionError::Custom(
                        PercolatorError::InvalidInstruction as u32,
                    ),
                ),
                "failure must occur at the injected instruction, not at the fee instruction"
            );
            assert_eq!(self.snapshot(), before, "aborted fragment must be atomic");
        } else {
            let metadata = result.expect("valid public fee fragment must succeed");
            assert_cu_within(
                "INV-059 maintenance history sync",
                metadata.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
        }
    }

    fn close(&mut self, portfolio: Pubkey, dest: Pubkey, rate_hint: u128) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        self.env
            .send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: rate_hint,
                },
                vec![
                    AccountMeta::new_readonly(self.owner.pubkey(), false),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(dest, false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(self.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            )
            .map(|cu| {
                assert_cu_within("INV-059 maintenance history close", cu, CUSTODY_CU_LIMIT);
                cu
            })
    }

    fn assert_prefix(&self, expected: &MaintenanceFeeOracle, anchor: u64) {
        let split = self.env.portfolio_state(self.split);
        let whole = self.env.portfolio_state(self.whole);
        assert_eq!(
            split.capital.get(),
            self.deposit - expected.gross + expected.self_reward,
            "split payer is charged only the elapsed fee, net of its own reward"
        );
        assert_eq!(split.last_fee_slot.get(), anchor);
        assert_eq!(whole.capital.get(), self.deposit);
        assert_eq!(
            whole.last_fee_slot.get(),
            0,
            "control remains unsynchronized"
        );
        self.assert_accounting(expected, 0, false);
    }

    fn assert_accounting(&self, expected: &MaintenanceFeeOracle, whole_fee: u128, closed: bool) {
        for (key, before) in &self.stationary_accounts {
            assert_eq!(self.env.svm.get_account(key).as_ref(), Some(before));
        }
        let group = self.env.market_state().1;
        let rewards: u128 = self
            .keepers
            .iter()
            .zip(expected.keeper_rewards)
            .map(|(&key, reward)| {
                assert_eq!(self.env.portfolio_state(key).capital.get(), reward);
                reward
            })
            .sum();
        assert_eq!(group.insurance, expected.retained + whole_fee);
        assert_eq!(
            group.insurance_domain_budget_remaining_total, group.insurance,
            "every retained fee must remain in domain budgets across route changes"
        );
        assert_domain_budget_remaining_total_consistent(&group, "INV-059 fee history");
        assert_eq!(
            group.c_tot,
            if closed {
                rewards
            } else {
                2 * self.deposit - expected.gross + expected.rewards()
            }
        );
        assert_eq!(group.vault, group.c_tot + group.insurance);
        assert_eq!(
            group.vault,
            u128::from(self.env.token_amount(self.env.vault))
        );
        assert_eq!(
            group.vault
                + u128::from(self.env.token_amount(self.split_dest))
                + u128::from(self.env.token_amount(self.whole_dest)),
            2 * self.deposit,
            "fees and payouts conserve actual custody"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/inv_059_maintenance_episode_fragmentation.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_public_maintenance_episode_fragmentation(
        rate in 1u128..=257,
        share in prop_oneof![
            Just(0u16), Just(1u16), Just(3_333u16), Just(5_000u16),
            Just(9_999u16), Just(10_000u16), 0u16..=10_000,
        ],
        first_slots in 1u64..=31,
        second_slots in 1u64..=31,
        rest in proptest::collection::vec(maintenance_fee_fragment(), 0..7),
        tail_slots in 1u64..=31,
        terminal_delay in 1u64..=31,
        terminal_sync in proptest::option::of(maintenance_reward_route()),
        slot_hint in prop_oneof![Just(0u64), Just(u64::MAX), any::<u64>()],
        rate_hint in prop_oneof![Just(0u128), Just(u128::MAX), any::<u128>()],
    ) {
        // Shrinking retains two positive fragments, a route change, an abort after
        // a payable fee, and a positive unpaid tail crossing into CloseResolved.
        let mut fragments = vec![
            MaintenanceFeeFragment {
                slots: first_slots,
                route: MaintenanceRewardRoute::SelfReward,
                abort_after_fee: Some(true),
                retries: 1,
            },
            MaintenanceFeeFragment {
                slots: second_slots,
                route: MaintenanceRewardRoute::Keeper(0),
                abort_after_fee: Some(false),
                retries: 0,
            },
        ];
        fragments.extend(rest);
        let resolved_slot = fragments.iter().map(|part| part.slots).sum::<u64>() + tail_slots;
        let whole_fee = rate * u128::from(resolved_slot);
        let mut episode = MaintenanceFeeEpisode::new(rate, share, whole_fee + 10_000);
        let mut expected = MaintenanceFeeOracle::default();
        let mut slot = 0;
        episode.assert_prefix(&expected, slot);

        for part in &fragments {
            slot += part.slots;
            episode.env.svm.warp_to_slot(slot);
            if let Some(after) = part.abort_after_fee {
                episode.sync(part.route, slot_hint, Some(after));
                episode.assert_prefix(&expected, slot - part.slots);
            }
            episode.sync(part.route, slot_hint, None);
            expected.charge(part.slots, rate, share, part.route);
            episode.assert_prefix(&expected, slot);
            for retry in 0..part.retries {
                // Distinct blockhashes and a different reward tail ensure these
                // reach the wrapper, rather than just hitting AlreadyProcessed.
                let before = episode.snapshot();
                let retry_route = if retry == 0 {
                    MaintenanceRewardRoute::Keeper(1)
                } else {
                    MaintenanceRewardRoute::Insurance
                };
                episode.sync(retry_route, slot_hint, None);
                assert_eq!(episode.snapshot(), before, "same-slot route change must not rebill");
            }
        }

        episode.env.svm.warp_to_slot(resolved_slot);
        episode.env.resolve();
        assert_eq!(episode.env.market_state().1.resolved_slot, resolved_slot);
        episode.env.svm.warp_to_slot(resolved_slot + terminal_delay);
        let before = episode.snapshot();
        let error = episode.close(episode.split, episode.bad_dest, rate_hint)
            .expect_err("wrong-owner destination must reject after terminal fee accounting");
        assert!(error.contains(&format!(
            "Custom({})",
            PercolatorError::InvalidTokenAccount as u32,
        )), "close must reach destination validation: {error}");
        assert_eq!(episode.snapshot(), before, "failed close cannot consume the unpaid fee tail");

        if let Some(route) = terminal_sync {
            episode.sync(route, slot_hint, None);
            expected.charge(tail_slots, rate, share, route);
            episode.assert_prefix(&expected, resolved_slot);
        } else {
            expected.charge(tail_slots, rate, share, MaintenanceRewardRoute::Insurance);
        }

        episode.close(episode.split, episode.split_dest, rate_hint)
            .expect("repaired split close must pay out");
        episode.close(episode.whole, episode.whole_dest, rate_hint)
            .expect("unsplit control closes under the same authenticated schedule");
        episode.assert_accounting(&expected, whole_fee, true);
        assert_eq!(expected.gross, whole_fee, "gross fee fragmentation has zero slack");
        let split_payout = u128::from(episode.env.token_amount(episode.split_dest));
        let whole_payout = u128::from(episode.env.token_amount(episode.whole_dest));
        assert_eq!(whole_payout, episode.deposit - whole_fee);
        assert_eq!(split_payout - expected.self_reward, whole_payout);
        assert_eq!(expected.retained + expected.rewards(), whole_fee);

        // Reward rounding is the only partition allowance, not a fresh fee on
        // retry or CloseResolved. Normalize for fragments with no reward route.
        let unsplit_reward = expected.rewarded_gross * u128::from(share) / 10_000;
        assert!(expected.rewards() <= unsplit_reward);
        assert!(unsplit_reward - expected.rewards() < expected.rewarded_fragments);
        let before = episode.snapshot();
        episode.env.svm.warp_to_slot(resolved_slot + terminal_delay + 31);
        episode.sync(MaintenanceRewardRoute::Keeper(1), u64::MAX, None);
        let error = episode.close(episode.split, episode.split_dest, u128::MAX)
            .expect_err("completed close must select no action, not another payout");
        assert!(is_engine_non_progress_error(&error), "terminal retry: {error}");
        assert_eq!(episode.snapshot(), before, "terminal route retries neither rebill nor repay");
    }
}
