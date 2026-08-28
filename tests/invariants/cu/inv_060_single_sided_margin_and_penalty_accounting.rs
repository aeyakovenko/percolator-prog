//! INV-060 - Single-sided margin and penalty accounting.
//!
//! Normative obligation: pending obligations, oracle lag, reserves, and penalties
//! must appear exactly once in the relevant health lane: not liquidating accounts
//! that remain above maintenance, not allowing risk increases below initial, and
//! not releasing insurance/backing while exposed target/effective lag is still
//! protecting users.
//!
//! Evidence in this file (I/C): public LiteSVM tests cover the IM/MM gap zone
//! and both live insurance and backing withdrawal gates under target/effective
//! lag. Full decomposition of every certificate lane remains a proof/model
//! follow-up, but these regressions hit the high-risk public routes.

use super::*;

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
