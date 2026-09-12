//! INV-060 - Single-sided margin and penalty accounting.
//!
//! Normative obligation: pending obligations, oracle lag, reserves, and penalties
//! must appear exactly once in the relevant health lane: not liquidating accounts
//! that remain above maintenance, not allowing risk increases below initial, and
//! not releasing insurance/backing while exposed target/effective lag is still
//! protecting users.
//!
//! Evidence in this file (I/C): public LiteSVM tests cover the IM/MM gap zone,
//! both live insurance and backing withdrawal gates under target/effective lag,
//! and a four-world exact lane decomposition for maintenance fee plus lag.
//! The accrued-fee admission matrix adds an exact second-asset opening boundary
//! for already-live accounts, before any explicit fee synchronization, across all
//! trade transports and both constrained parties. It does not certify flat first opens.
//!
//! Shared independent evidence (F/I/M): `support::fuzz_model` recomputes every
//! current certificate from raw wrapper state without invoking the engine refresh
//! implementation. It independently derives ADL-effective quantity, ceil notional,
//! IM/MM floors, target lag, source realizability, fee debt, equity, and liquidation
//! deficit after every generated public transition. The directed INV-053 public
//! matrices apply that oracle to valid and exact-expiry impaired liens, both sides
//! of a final-leg pending bankruptcy, and a mixed Recovery/Live portfolio. Those
//! states prove close residuals and impaired claims affect equity once through PnL
//! or source support and do not reappear in requirement lanes. `reserved_pnl` is a
//! terminal payout encumbrance owned by INV-067/068, not a second margin penalty;
//! `cancel_deposit_escrow` has no public writer and is owned by INV-026/087.

use super::*;
use crate::support::fuzz_model::{assert_current_certificate_matches_independent, TradeRoute};

#[test]
fn v16_program_accrued_maintenance_precedes_new_asset_risk_admission() {
    const PRICE: u64 = 100;
    const START_SLOT: u64 = 1;
    const ADMISSION_SLOT: u64 = 4;
    const FEE_PER_SLOT: u128 = 37;
    const ACCRUED_FEE: u128 = FEE_PER_SLOT * (ADMISSION_SLOT - START_SLOT) as u128;
    const THIN_CAPITAL: u128 = 200 + ACCRUED_FEE;
    const PEER_CAPITAL: u128 = 1_000;
    const SIZE_Q: i128 = POS_SCALE as i128;
    const OVER_LIMIT_Q: i128 = SIZE_Q + (POS_SCALE / PRICE as u128) as i128;

    assert_eq!(ACCRUED_FEE, 111);
    assert_eq!(OVER_LIMIT_Q as u128 * PRICE as u128 / POS_SCALE, 101);
    assert!(
        THIN_CAPITAL >= 201,
        "skipping accrued fees would admit the larger request"
    );
    assert_eq!(THIN_CAPITAL - ACCRUED_FEE, 200);

    let (mut worlds, mut peak_trade_cu) = (0, 0);
    for thin_party in 0..2 {
        let mut full_refresh_certs = None;
        for route in [
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ] {
            for explicitly_refreshed in [true, false] {
                let label = format!("{route:?}/thin={thin_party}/refreshed={explicitly_refreshed}");
                let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
                    2,
                    5_000,
                    10_000,
                    500,
                    FEE_PER_SLOT,
                );
                env.svm.warp_to_slot(START_SLOT);
                for asset in 0..2 {
                    env.configure_auth_mark_for_asset_as_admin(asset, START_SLOT, PRICE);
                }
                let owners = [Keypair::new(), Keypair::new()];
                let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
                let keeper_owner = Keypair::new();
                let keeper = env.create_portfolio(&keeper_owner);
                let deposits = std::array::from_fn::<_, 2, _>(|party| {
                    if party == thin_party {
                        THIN_CAPITAL
                    } else {
                        PEER_CAPITAL
                    }
                });
                let sources = std::array::from_fn::<_, 2, _>(|party| {
                    env.deposit(&owners[party], portfolios[party], deposits[party])
                });
                env.trade_asset_with_cu(
                    0,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    SIZE_Q,
                    PRICE,
                    0,
                );
                let matcher = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi).then(|| {
                    auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1])
                });
                let before_age = portfolios.map(|key| env.svm.get_account(&key));
                let opening_group = env.market_state().1;

                // Only the empty keeper observes time. Neither participant is fee-synced or refreshed.
                for slot in START_SLOT + 1..=ADMISSION_SLOT {
                    env.svm.warp_to_slot(slot);
                    env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&[0, 1]),
                        },
                    );
                }
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    before_age,
                    "{label}"
                );
                let aged_group = env.market_state().1;
                assert_eq!(aged_group.current_slot, ADMISSION_SLOT);
                assert_eq!(aged_group.insurance, 0);
                assert_eq!(aged_group.c_tot, THIN_CAPITAL + PEER_CAPITAL);
                for asset in 0..2 {
                    let aged = &aged_group.assets[asset];
                    let opening = &opening_group.assets[asset];
                    assert_eq!(aged.slot_last, ADMISSION_SLOT);
                    assert_eq!(aged.effective_price, PRICE);
                    assert_eq!(aged.raw_oracle_target_price, PRICE);
                    assert_eq!(
                        (aged.k_long, aged.k_short),
                        (opening.k_long, opening.k_short)
                    );
                    assert_eq!(
                        (aged.f_long_num, aged.f_short_num),
                        (opening.f_long_num, opening.f_short_num)
                    );
                }
                for party in 0..2 {
                    let account = env.portfolio_state(portfolios[party]);
                    assert_eq!(account.last_fee_slot.get(), START_SLOT);
                    assert_eq!(account.fee_credits.get(), 0);
                    assert_eq!(account.capital.get(), deposits[party]);
                    assert_eq!(account.pnl.get(), 0);
                    assert_eq!(
                        percolator::active_bitmap_count_ones(active_bitmap(&account)),
                        1
                    );
                    assert!(!has_active_leg_for_asset(&account, 1));
                    assert_eq!(health_cert(&account).certified_initial_req, PRICE as u128);
                }

                let trade = |env: &mut V16CuEnv, size_q| {
                    let ix = match route {
                        TradeRoute::NoCpi => {
                            env.trade_no_cpi_ix(portfolios[0], portfolios[1], 1, size_q, PRICE, 0)
                        }
                        TradeRoute::Cpi => {
                            env.trade_cpi_ix(portfolios[0], portfolios[1], 1, size_q, 0, PRICE)
                        }
                        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeLeg {
                                asset_index: 1,
                                market_id: env.asset_market_id(1),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: 0,
                            }],
                        ),
                        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeCpiLeg {
                                asset_index: 1,
                                market_id: env.asset_market_id(1),
                                size_q,
                                fee_bps: 0,
                                limit_price: PRICE,
                            }],
                            0,
                            0,
                        ),
                    };
                    env.svm.expire_blockhash();
                    if let Some((program, context, delegate)) = matcher {
                        env.send(
                            ix,
                            vec![
                                AccountMeta::new(owners[0].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[0], false),
                                AccountMeta::new(portfolios[1], false),
                                AccountMeta::new_readonly(program, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            &[&owners[0]],
                        )
                    } else {
                        env.send(
                            ix,
                            vec![
                                AccountMeta::new(owners[0].pubkey(), true),
                                AccountMeta::new(owners[1].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[0], false),
                                AccountMeta::new(portfolios[1], false),
                            ],
                            &[&owners[0], &owners[1]],
                        )
                    }
                };
                let check_accounting = |env: &V16CuEnv, leg_count: u32| {
                    let group = env.market_state().1;
                    assert_eq!(
                        group.c_tot,
                        THIN_CAPITAL + PEER_CAPITAL - 2 * ACCRUED_FEE,
                        "{label}"
                    );
                    assert_eq!(group.insurance, 2 * ACCRUED_FEE, "{label}");
                    assert_eq!(group.vault, THIN_CAPITAL + PEER_CAPITAL, "{label}");
                    assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
                    assert_eq!(
                        group.vault,
                        u128::from(env.token_amount(env.vault)),
                        "{label}"
                    );
                    // Split each account's odd fee before aggregating the two domain credits.
                    let budgets = [
                        2 * (ACCRUED_FEE / 2),
                        2 * (ACCRUED_FEE - ACCRUED_FEE / 2),
                        0,
                        0,
                    ];
                    for (domain, expected) in budgets.into_iter().enumerate() {
                        assert_eq!(
                            group.insurance_domain_budget[domain], expected,
                            "{label}: maintenance belongs to asset zero"
                        );
                    }
                    std::array::from_fn::<_, 2, _>(|party| {
                        let account = env.portfolio_state(portfolios[party]);
                        let cert = health_cert(&account);
                        assert_eq!(
                            account.capital.get(),
                            deposits[party] - ACCRUED_FEE,
                            "{label}"
                        );
                        assert_eq!(account.last_fee_slot.get(), ADMISSION_SLOT, "{label}");
                        assert_eq!(account.fee_credits.get(), 0, "{label}");
                        assert_eq!(account.pnl.get(), 0, "{label}");
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(&account)),
                            leg_count,
                            "{label}"
                        );
                        assert_eq!(
                            cert.certified_equity,
                            (deposits[party] - ACCRUED_FEE) as i128,
                            "{label}"
                        );
                        assert_eq!(
                            cert.certified_initial_req,
                            u128::from(leg_count) * PRICE as u128,
                            "{label}: fees cannot also inflate IM"
                        );
                        assert_eq!(
                            cert.certified_maintenance_req,
                            u128::from(leg_count) * PRICE as u128 / 2,
                            "{label}: fees cannot also inflate MM"
                        );
                        assert_eq!(
                            cert.certified_worst_case_loss,
                            u128::from(leg_count) * PRICE as u128,
                            "{label}"
                        );
                        assert_eq!(cert.certified_liq_deficit, 0, "{label}");
                        assert!(assert_current_certificate_matches_independent(
                            &label, &group, &account
                        )
                        .expect("every current certificate lane must match the independent model"));
                        cert
                    })
                };

                let mut tracked = vec![
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    keeper,
                    env.vault,
                    env.mint,
                    sources[0],
                    sources[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    env.program_id,
                    spl_token::ID,
                ];
                if let Some((program, context, delegate)) = matcher {
                    tracked.extend([program, context, delegate]);
                }
                let snapshot = |env: &V16CuEnv| {
                    tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let original_frame = snapshot(&env);
                if explicitly_refreshed {
                    for portfolio in portfolios {
                        env.sync_maintenance_fee_with_cu(portfolio, None, ADMISSION_SLOT);
                        env.crank(
                            portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: ADMISSION_SLOT,
                                observations: crank_observations_for_assets(&[0, 1]),
                            },
                        );
                    }
                    check_accounting(&env, 1);
                }

                let before_rejection = snapshot(&env);
                let error = trade(&mut env, OVER_LIMIT_Q).expect_err(
                    "the one-atom excess must reject after accounting all accrued fees",
                );
                assert!(
                    error.contains(&format!(
                        "Custom({})",
                        PercolatorError::EngineInvalidConfig as u32
                    )),
                    "{label}: wrong admission error: {error}"
                );
                assert_eq!(
                    snapshot(&env),
                    before_rejection,
                    "{label}: exact economic account/lamport rollback, including matcher state"
                );

                let trade_cu = trade(&mut env, SIZE_Q)
                    .expect("the exact post-fee IM boundary must remain admissible");
                assert_cu_within(&label, trade_cu, MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
                peak_trade_cu = peak_trade_cu.max(trade_cu);
                let certs = check_accounting(&env, 2);
                assert_eq!(
                    certs[thin_party].certified_equity
                        - certs[thin_party].certified_initial_req as i128,
                    0,
                    "{label}"
                );
                if let Some(expected) = full_refresh_certs {
                    assert_eq!(
                        certs, expected,
                        "{label}: direct admission cannot improve on public full refresh"
                    );
                } else {
                    assert!(explicitly_refreshed);
                    full_refresh_certs = Some(certs);
                }
                for asset in 0..2 {
                    let group = env.market_state().1;
                    assert_eq!(group.assets[asset].oi_eff_long_q, POS_SCALE);
                    assert_eq!(group.assets[asset].oi_eff_short_q, POS_SCALE);
                    for party in 0..2 {
                        assert_eq!(
                            active_leg_for_asset(&env.portfolio_state(portfolios[party]), asset)
                                .basis_pos_q,
                            if party == 0 { SIZE_Q } else { -SIZE_Q },
                            "{label}"
                        );
                    }
                }
                for (key, before) in tracked.iter().zip(&original_frame) {
                    if *key != env.market
                        && !portfolios.contains(key)
                        && !matcher.is_some_and(|(_, context, _)| *key == context)
                    {
                        assert_eq!(
                            env.svm.get_account(key),
                            *before,
                            "{label}: unrelated/custody frame {key}"
                        );
                    }
                }

                // Replaying explicit synchronization in the same slot must not charge either party again.
                let after_admission = snapshot(&env);
                for portfolio in portfolios {
                    env.svm.expire_blockhash();
                    env.sync_maintenance_fee_with_cu(portfolio, None, ADMISSION_SLOT);
                    assert_eq!(
                        snapshot(&env),
                        after_admission,
                        "{label}: already-accounted fees must be an exact no-op"
                    );
                }
                println!("{label}: exact rejection, boundary admission, same-slot fee replay; trade={trade_cu} CU");
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-060 accrued maintenance: {worlds} worlds, 16 exact rejections, 16 boundary admissions, 32 exact fee no-ops; peak_trade_cu={peak_trade_cu}");
}

#[derive(Debug)]
struct Inv060PublicLaneWorld {
    cert: percolator::HealthCertV16,
    capital: u128,
    raw_target_price: u64,
    effective_price: u64,
}

fn inv060_public_lane_world(
    maintenance_fee_per_slot: u128,
    max_price_move_bps_per_slot: u64,
    raw_target_price: u64,
) -> Inv060PublicLaneWorld {
    const INITIAL_PRICE: u64 = 100_000;

    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1,
        5_000,
        10_000,
        max_price_move_bps_per_slot,
        maintenance_fee_per_slot,
    );
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, INITIAL_PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        10 * POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, raw_target_price);
    env.crank_steps_after_market_catchup(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        if maintenance_fee_per_slot == 0 { 1 } else { 2 },
    );

    let portfolio = env.portfolio_state(long);
    let group = env.market_state().1;
    Inv060PublicLaneWorld {
        cert: health_cert(&portfolio),
        capital: portfolio.capital.get(),
        raw_target_price: group.assets[0].raw_oracle_target_price,
        effective_price: group.assets[0].effective_price,
    }
}

#[test]
fn v16_program_fee_and_target_lag_compose_exactly_once_in_health_lanes() {
    const EFFECTIVE_PRICE: u64 = 99_760;
    const LAGGED_TARGET_PRICE: u64 = 90_000;
    const MAINTENANCE_FEE_PER_SLOT: u128 = 37;

    // A 24 bps move toward 90_000 and a 24 bps move exactly to 99_760 produce the
    // same effective price. This isolates raw-target lag from marked PnL.
    let base = inv060_public_lane_world(0, 24, EFFECTIVE_PRICE);
    let fee = inv060_public_lane_world(MAINTENANCE_FEE_PER_SLOT, 24, EFFECTIVE_PRICE);
    let lag = inv060_public_lane_world(0, 24, LAGGED_TARGET_PRICE);
    let combined = inv060_public_lane_world(MAINTENANCE_FEE_PER_SLOT, 24, LAGGED_TARGET_PRICE);

    for world in [&base, &fee, &lag, &combined] {
        assert_eq!(world.effective_price, EFFECTIVE_PRICE);
        assert!(world.cert.valid);
    }
    assert_eq!(base.raw_target_price, EFFECTIVE_PRICE);
    assert_eq!(fee.raw_target_price, EFFECTIVE_PRICE);
    assert_eq!(lag.raw_target_price, LAGGED_TARGET_PRICE);
    assert_eq!(combined.raw_target_price, LAGGED_TARGET_PRICE);

    let fee_charge = base
        .capital
        .checked_sub(fee.capital)
        .expect("maintenance fee only debits capital");
    assert_eq!(fee_charge, MAINTENANCE_FEE_PER_SLOT);
    assert_eq!(
        lag.capital - combined.capital,
        fee_charge,
        "raw-target lag cannot change the maintenance charge"
    );

    assert_eq!(
        fee.cert.certified_initial_req,
        base.cert.certified_initial_req
    );
    assert_eq!(
        fee.cert.certified_maintenance_req,
        base.cert.certified_maintenance_req
    );
    assert_eq!(
        fee.cert.certified_worst_case_loss,
        base.cert.certified_worst_case_loss
    );
    assert_eq!(
        fee.cert.certified_equity,
        base.cert.certified_equity - fee_charge as i128,
        "maintenance charge belongs only in equity"
    );

    assert_eq!(
        lag.cert.certified_equity, base.cert.certified_equity,
        "equal effective prices isolate target lag from marked PnL"
    );
    let initial_lag = lag.cert.certified_initial_req - base.cert.certified_initial_req;
    let maintenance_lag = lag.cert.certified_maintenance_req - base.cert.certified_maintenance_req;
    let worst_case_lag = lag.cert.certified_worst_case_loss - base.cert.certified_worst_case_loss;
    assert!(initial_lag > 0, "the lag world exercises a real penalty");
    assert_eq!(maintenance_lag, initial_lag);
    assert_eq!(worst_case_lag, initial_lag);

    assert_eq!(
        combined.cert.certified_initial_req,
        lag.cert.certified_initial_req
    );
    assert_eq!(
        combined.cert.certified_maintenance_req,
        lag.cert.certified_maintenance_req
    );
    assert_eq!(
        combined.cert.certified_worst_case_loss,
        lag.cert.certified_worst_case_loss
    );
    assert_eq!(
        combined.cert.certified_equity,
        lag.cert.certified_equity - fee_charge as i128
    );

    let base_initial_headroom =
        base.cert.certified_equity - base.cert.certified_initial_req as i128;
    let combined_initial_headroom =
        combined.cert.certified_equity - combined.cert.certified_initial_req as i128;
    assert_eq!(
        base_initial_headroom - combined_initial_headroom,
        (fee_charge + initial_lag) as i128,
        "fee plus lag tightens initial headroom exactly once each"
    );
    let base_maintenance_headroom =
        base.cert.certified_equity - base.cert.certified_maintenance_req as i128;
    let combined_maintenance_headroom =
        combined.cert.certified_equity - combined.cert.certified_maintenance_req as i128;
    assert_eq!(
        base_maintenance_headroom - combined_maintenance_headroom,
        (fee_charge + maintenance_lag) as i128,
        "fee plus lag tightens maintenance headroom exactly once each"
    );
}

#[test]
fn v16_program_margin_gap_zone_no_liquidation_no_risk_increase() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.configure_auth_mark_with_cu(0, 100);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty_owner = Keypair::new();
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 100);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let basis_before = env.portfolio_state(portfolio).legs[0].basis_pos_q.get();
    assert_ne!(basis_before, 0);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 110);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        1,
    );
    let state = env.portfolio_state(portfolio);
    let cert = health_cert(&state);
    let equity = cert.certified_equity;
    let maintenance = cert.certified_maintenance_req as i128;
    let initial = cert.certified_initial_req as i128;
    assert!(equity > maintenance);
    assert!(equity < initial);
    assert_eq!(cert.certified_liq_deficit, 0);
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert_eq!(
        env.portfolio_state(portfolio).legs[0].basis_pos_q.get(),
        basis_before,
        "in-gap account is not liquidated"
    );

    env.svm.expire_blockhash();
    let risk_increase = env.try_trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        110,
        0,
    );
    assert!(
        risk_increase.is_err(),
        "risk increase below initial margin must reject"
    );
    assert_eq!(
        env.portfolio_state(portfolio).legs[0].basis_pos_q.get(),
        basis_before
    );
    let group = env.market_state().1;
    assert!(group.vault >= group.c_tot + group.insurance);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_live_insurance_withdraw_rejects_exposed_target_effective_lag() {
    const INITIAL_MARK: u64 = 100_000_000;
    const TARGET_MARK: u64 = 90_000_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 24);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, INITIAL_MARK);
    env.enable_live_insurance_withdrawal();
    env.top_up_insurance(1_000_000);
    env.top_up_insurance_domain_with_authority(&env.admin.insecure_clone(), 0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000_000);
    env.deposit(&short_owner, short, 1_000_000_000);
    env.svm.expire_blockhash();
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        INITIAL_MARK,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, TARGET_MARK);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    let group_before = env.market_state().1;
    assert_ne!(
        group_before.assets[0].raw_oracle_target_price,
        group_before.assets[0].effective_price
    );
    assert!(group_before.assets[0].oi_eff_long_q > 0 || group_before.assets[0].oi_eff_short_q > 0);

    let admin = env.admin.insecure_clone();
    let rejected = env.try_withdraw_insurance_asset_with_authority(&admin, 0, 100);
    assert!(
        rejected.is_err(),
        "live insurance withdrawal must reject while exposed lag exists"
    );
    let group_after = env.market_state().1;
    assert_eq!(group_after.insurance, group_before.insurance);
    assert_eq!(
        group_after.insurance_domain_budget[0],
        group_before.insurance_domain_budget[0]
    );
}

#[test]
fn v16_program_live_backing_withdraw_rejects_exposed_target_effective_lag() {
    const INITIAL_MARK: u64 = 100_000_000;
    const TARGET_MARK: u64 = 90_000_000;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 24);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, INITIAL_MARK);
    env.top_up_backing_bucket(0, 500_000, 1_000_000);
    let backing_before = env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num;
    assert!(backing_before > 0);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000_000);
    env.deposit(&short_owner, short, 1_000_000_000);
    env.svm.expire_blockhash();
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        INITIAL_MARK,
        0,
    );

    let admin = env.admin.insecure_clone();
    let dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    env.withdraw_backing_bucket_to_admin_token_with_cu(dest, 0, 100);
    assert!(
        env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num < backing_before,
        "healthy backing withdrawal proves the route is nonvacuous"
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, TARGET_MARK);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    let group_before = env.market_state().1;
    assert_ne!(
        group_before.assets[0].raw_oracle_target_price,
        group_before.assets[0].effective_price
    );
    let backing_before_attack = group_before.source_backing_buckets[0].fresh_unliened_backing_num;

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: group_before.assets[0].market_id,
            authority_epoch: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "live backing withdrawal must reject while exposed lag exists"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num,
        backing_before_attack
    );
}

// security.md sweep — dust-position margin floor (#9/#22): the per-leg initial margin requirement is
// floored at min_nonzero_im_req for any nonzero position. Attacker goal: open a tiny position whose
// proportional IM (bps * tiny notional) rounds below the floor, getting near-free leverage / a position
// that evades meaningful margin. Protection: certified_initial_req >= min_nonzero_im_req for a live leg.
#[test]
fn v16_attack_dust_position_margin_floored() {
    // production_risk_params: initial_margin_bps=500 (5%), min_nonzero_im_req=600.
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let xo = Keypair::new();
    let x = env.create_portfolio(&xo);
    let yo = Keypair::new();
    let y = env.create_portfolio(&yo);
    env.deposit(&xo, x, 1_000_000);
    env.deposit(&yo, y, 1_000_000);

    // a DUST position: size POS_SCALE/1000 -> notional ~1000 -> proportional IM (5%) ~50, BELOW the 600 floor.
    let dust = (POS_SCALE / 1_000) as i128;
    assert!(dust > 0, "dust size is nonzero");
    env.trade_asset_with_cu(0, &xo, x, &yo, y, dust, 1_000_000, 0);

    let xs = env.portfolio_state(x);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&xs)),
        1,
        "dust position opened"
    );
    let req = health_cert(&xs).certified_initial_req;
    // FLOOR: the requirement is the min_nonzero_im_req floor (600), NOT the tiny proportional ~50.
    assert!(
        req >= 600,
        "dust-position initial margin floored at min_nonzero_im_req (600), got {}",
        req
    );
    // sanity: the floor is well above the naive proportional IM for this dust notional (~50).
    assert!(
        req > 50,
        "floor strictly exceeds the proportional dust IM (no near-free leverage)"
    );

    let g = env.market_state().1;
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}

// security.md sweep — dust-position MAINTENANCE floor (#9/#19): the per-leg maintenance requirement is
// floored at min_nonzero_mm_req for any nonzero leg (the liquidation-threshold counterpart to the IM
// floor in #161). Attacker goal: a dust position whose proportional maintenance margin (bps*tiny
// notional) rounds near 0 becomes effectively un-liquidatable (it never breaches a ~0 maintenance req).
// Protection: certified_maintenance_req >= min_nonzero_mm_req for a live leg.
#[test]
fn v16_attack_dust_position_maintenance_floored() {
    // production_risk_params: maintenance_margin_bps=500 (5%), min_nonzero_mm_req=599.
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let xo = Keypair::new();
    let x = env.create_portfolio(&xo);
    let yo = Keypair::new();
    let y = env.create_portfolio(&yo);
    env.deposit(&xo, x, 1_000_000);
    env.deposit(&yo, y, 1_000_000);
    let dust = (POS_SCALE / 1_000) as i128; // notional ~1000 -> proportional MM (5%) ~25
    env.trade_asset_with_cu(0, &xo, x, &yo, y, dust, 1_000_000, 0);

    let xs = env.portfolio_state(x);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&xs)),
        1,
        "dust position opened"
    );
    let mreq = health_cert(&xs).certified_maintenance_req;
    // FLOOR: maintenance req is the min_nonzero_mm_req floor (599), not the proportional ~25.
    assert!(
        mreq >= 599,
        "dust maintenance req floored at min_nonzero_mm_req (599), got {}",
        mreq
    );
    assert!(
        mreq > 25,
        "floor strictly exceeds the proportional dust MM (no liquidation-immune dust)"
    );
    // and the maintenance floor is below the initial floor (a real gap remains for the dust leg).
    assert!(
        mreq <= health_cert(&xs).certified_initial_req,
        "maint floor <= initial floor"
    );

    let g = env.market_state().1;
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}
