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
//! fee-bearing liquidations with a new authenticated mark and a fresh certified deficit, proving
//! that retries cannot manufacture episodes while genuine risk deterioration can.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

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

#[test]
fn v16_program_new_liquidation_fee_episode_requires_new_authenticated_deficit() {
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
