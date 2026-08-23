//! INV-054 - Certificate epoch completeness.
//!
//! Normative obligation: Every health-relevant state change invalidates or conservatively downgrades certificates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public trade/mark/crank/close
//! sequences create a real source-backed released-PnL claim. Public oracle, source-credit,
//! source-lien, lifecycle, reset, and asset-set mutations then make its certificate stale.
//! Favorable conversion must reject with exact rollback until a permissionless public crank
//! refreshes every certificate key. A public bankruptcy-close case separately proves that the
//! touched pending-obligation account is atomically recertified with its exact deficit, unrelated
//! certificates are staled by the composed source-risk writes, risk-bearing reuse rejects, and a
//! flat unrelated owner retains its state-independent principal exit.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

const PUBLIC_RELEASED_PNL: u128 = 50_000;

fn cert_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    let cert = health_cert(&account);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account)
}

fn setup_public_released_pnl_certificate() -> (V16CuEnv, Keypair, Pubkey) {
    const INITIAL_PRICE: u64 = 1_000_000;
    const WINNING_PRICE: u64 = 1_050_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_abs_funding_e9_per_slot: 1_000,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.top_up_backing_bucket(1, 75_000, 10_000);

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, WINNING_PRICE);
    for portfolio in [loser, winner] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        -SIZE_Q,
        WINNING_PRICE,
        0,
    );

    let winner_state = env.portfolio_state(winner);
    assert!(
        !has_active_leg_for_asset(&winner_state, 0),
        "public close must leave the winner flat"
    );
    assert_eq!(
        winner_state.pnl.get(),
        PUBLIC_RELEASED_PNL as i128,
        "public price move and close must create the expected source-backed claim"
    );
    assert!(
        cert_is_current(&env, winner),
        "the public close must issue a fully current certificate"
    );
    (env, winner_owner, winner)
}

fn assert_stale_conversion_rolls_back(env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey) {
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        env.convert_released_pnl_ix(portfolio, PUBLIC_RELEASED_PNL),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    );
    assert!(
        rejected.is_err(),
        "a favorable conversion with a stale certificate must propagate an instruction error"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
}

fn refresh_public_claim_certificate(env: &mut V16CuEnv, portfolio: Pubkey) {
    let now_slot = env.svm.get_sysvar::<Clock>().slot;
    env.crank_if_actionable(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot,
            // A flat stale certificate has no active leg from which the engine can
            // self-select an accrual asset. Any authenticated current observation
            // supplies that bounded refresh context; it does not choose economics.
            observations: crank_observations(0),
        },
    );
    assert!(
        cert_is_current(env, portfolio),
        "permissionless public refresh must restore all certificate keys"
    );
}

fn refresh_and_convert_public_claim(env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey) {
    refresh_public_claim_certificate(env, portfolio);
    let capital_before = env.portfolio_state(portfolio).capital.get();
    let convert_cu = env.convert_released_pnl_with_cu(owner, portfolio, PUBLIC_RELEASED_PNL);
    assert_cu_within(
        "public released-PnL conversion after certificate refresh",
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.portfolio_state(portfolio).capital.get(),
        capital_before + PUBLIC_RELEASED_PNL,
        "refresh admits exactly the publicly realized claim"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault),
        "certificate refresh and conversion preserve SPL custody parity"
    );
}

#[test]
fn v16_attack_source_credit_risk_epoch_invalidates_public_released_pnl_cert() {
    let (mut env, owner, portfolio) = setup_public_released_pnl_certificate();
    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(portfolio));

    env.top_up_backing_bucket(0, 1, 10_000);

    let after = env.market_state().1;
    let stale = health_cert(&env.portfolio_state(portfolio));
    assert_eq!(after.risk_epoch, before.risk_epoch + 1);
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(after.asset_set_epoch, before.asset_set_epoch);
    assert_eq!(
        stale, cert_before,
        "unrelated backing does not rewrite the account"
    );
    assert!(
        stale.cert_risk_epoch < after.risk_epoch,
        "the isolated source-credit mutation must invalidate the old risk certificate"
    );

    assert_stale_conversion_rolls_back(&mut env, &owner, portfolio);
    refresh_and_convert_public_claim(&mut env, &owner, portfolio);
}

#[test]
fn v16_attack_source_lien_risk_epoch_invalidates_unrelated_released_pnl_cert() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const FIRST_MARK: u64 = 1_050_000;
    const SECOND_ASSET_MARK: u64 = 950_000;
    const SECOND_FIRST_ASSET_MARK: u64 = 1_100_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    for asset_index in 0..2 {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, INITIAL_PRICE);
    }
    env.top_up_backing_bucket(1, 1_000_000, 10_000);

    let claim_owner = Keypair::new();
    let claim_loser_owner = Keypair::new();
    let claimant = env.create_portfolio(&claim_owner);
    let claim_loser = env.create_portfolio(&claim_loser_owner);
    env.deposit(&claim_owner, claimant, 1_000_000);
    env.deposit(&claim_loser_owner, claim_loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &claim_owner,
        claimant,
        &claim_loser_owner,
        claim_loser,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, FIRST_MARK);
    for portfolio in [claim_loser, claimant] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    env.trade_asset_with_cu(
        0,
        &claim_owner,
        claimant,
        &claim_loser_owner,
        claim_loser,
        -(POS_SCALE as i128),
        FIRST_MARK,
        0,
    );
    assert_eq!(
        env.portfolio_state(claimant).pnl.get(),
        PUBLIC_RELEASED_PNL as i128
    );

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross = env.create_portfolio(&cross_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross, 3_130_000);
    env.deposit(&counterparty_owner, counterparty, 10_000_000);
    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross,
        &counterparty_owner,
        counterparty,
        20 * POS_SCALE as i128,
        FIRST_MARK,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross,
        &counterparty_owner,
        counterparty,
        10 * POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_for_asset_as_admin(0, 3, SECOND_FIRST_ASSET_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 3, SECOND_ASSET_MARK);
    for (portfolio, asset_index) in [(counterparty, 0), (cross, 0), (counterparty, 1)] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    assert_eq!(env.portfolio_state(cross).pnl.get(), 500_000);
    refresh_public_claim_certificate(&mut env, claimant);

    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(claimant));
    let increase_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        SECOND_ASSET_MARK,
        0,
    );
    assert_cu_within(
        "public source-credit lien creation",
        increase_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let after = env.market_state().1;
    let cross_after = env.portfolio_state(cross);
    let stale = health_cert(&env.portfolio_state(claimant));
    assert!(
        cross_after
            .source_domains
            .iter()
            .any(|slot| slot.source_lien_effective_reserved.get() != 0),
        "the public risk increase must create a real source-credit lien"
    );
    assert_eq!(after.risk_epoch, before.risk_epoch + 1);
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(after.asset_set_epoch, before.asset_set_epoch);
    assert_eq!(
        stale, cert_before,
        "another account's lien creation must not rewrite the claimant"
    );
    assert!(stale.cert_risk_epoch < after.risk_epoch);

    assert_stale_conversion_rolls_back(&mut env, &claim_owner, claimant);
    refresh_and_convert_public_claim(&mut env, &claim_owner, claimant);
}

#[test]
fn v16_attack_lifecycle_risk_epoch_invalidates_public_released_pnl_cert() {
    let (mut env, owner, portfolio) = setup_public_released_pnl_certificate();
    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(portfolio));
    let lifecycle_cu =
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_DRAIN_ONLY, 0, 0, 0);
    assert_cu_within(
        "public Active-to-DrainOnly certificate invalidation",
        lifecycle_cu,
        CUSTODY_CU_LIMIT,
    );

    let after = env.market_state().1;
    let stale = health_cert(&env.portfolio_state(portfolio));
    assert_eq!(after.assets[0].lifecycle, AssetLifecycleV16::DrainOnly);
    assert_eq!(after.risk_epoch, before.risk_epoch + 1);
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(after.asset_set_epoch, before.asset_set_epoch + 1);
    assert_eq!(
        stale, cert_before,
        "a market lifecycle transition must not rewrite an unrelated portfolio"
    );
    assert!(
        stale.cert_risk_epoch < after.risk_epoch,
        "the lifecycle transition must invalidate the old risk certificate"
    );
    assert!(
        stale.cert_asset_set_epoch < after.asset_set_epoch,
        "the lifecycle transition must invalidate the old asset-set certificate"
    );

    assert_stale_conversion_rolls_back(&mut env, &owner, portfolio);
    refresh_and_convert_public_claim(&mut env, &owner, portfolio);
}

#[test]
fn v16_attack_reset_pending_risk_epoch_invalidates_public_released_pnl_cert() {
    const OPEN_Q: u128 = 10 * POS_SCALE;

    let (mut env, claim_owner, claimant) = setup_public_released_pnl_certificate();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    let price = env.market_state().1.assets[0].effective_price;
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        price,
        0,
    );
    refresh_public_claim_certificate(&mut env, claimant);

    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(claimant));
    let reset_cu = env.rebalance_reduce_with_cu(&long_owner, long, 0, OPEN_Q);
    assert_cu_within(
        "public full rebalance entering ResetPending",
        reset_cu,
        CUSTODY_CU_LIMIT,
    );

    let after = env.market_state().1;
    let stale = health_cert(&env.portfolio_state(claimant));
    assert_eq!(after.assets[0].mode_short, SideModeV16::ResetPending);
    assert_eq!(after.assets[0].oi_eff_long_q, 0);
    assert_eq!(after.assets[0].oi_eff_short_q, 0);
    assert_eq!(after.risk_epoch, before.risk_epoch + 1);
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(after.asset_set_epoch, before.asset_set_epoch);
    assert_eq!(
        stale, cert_before,
        "another account's reset transition must not rewrite the claimant"
    );
    assert!(stale.cert_risk_epoch < after.risk_epoch);

    assert_stale_conversion_rolls_back(&mut env, &claim_owner, claimant);

    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: Vec::new(),
        },
    );
    let cleaned = env.market_state().1;
    assert_eq!(cleaned.assets[0].mode_short, SideModeV16::ResetPending);
    assert_eq!(cleaned.assets[0].stored_pos_count_short, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    refresh_public_claim_certificate(&mut env, claimant);

    let before_finalize = env.market_state().1;
    let cert_before_finalize = health_cert(&env.portfolio_state(claimant));
    let finalize_cu = env.finalize_reset_side_with_cu(0, 1);
    assert_cu_within(
        "public ResetPending-to-Normal finalization",
        finalize_cu,
        CUSTODY_CU_LIMIT,
    );
    let finalized = env.market_state().1;
    let finalize_stale = health_cert(&env.portfolio_state(claimant));
    assert_eq!(finalized.assets[0].mode_short, SideModeV16::Normal);
    assert_eq!(finalized.risk_epoch, before_finalize.risk_epoch + 1);
    assert_eq!(finalized.oracle_epoch, before_finalize.oracle_epoch);
    assert_eq!(finalized.funding_epoch, before_finalize.funding_epoch);
    assert_eq!(finalized.asset_set_epoch, before_finalize.asset_set_epoch);
    assert_eq!(finalize_stale, cert_before_finalize);
    assert!(finalize_stale.cert_risk_epoch < finalized.risk_epoch);

    assert_stale_conversion_rolls_back(&mut env, &claim_owner, claimant);
    refresh_and_convert_public_claim(&mut env, &claim_owner, claimant);
}

#[test]
fn v16_attack_pending_obligation_recertifies_affected_and_stales_unrelated_cert() {
    const OPEN_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 150;
    const OPEN_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: OPEN_PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let observer_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    let observer = env.create_portfolio(&observer_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&winner_owner, winner, 1_000);
    env.deposit(&loser_owner, loser, 250);
    env.deposit(&observer_owner, observer, 1);
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

    let mut final_slot = 1;
    for (offset, mark) in (105..=WINNING_PRICE).step_by(5).enumerate() {
        final_slot = 2 + u64::try_from(offset).unwrap();
        env.svm.warp_to_slot(final_slot);
        env.push_auth_mark_for_asset_as_admin(0, final_slot, mark);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: final_slot,
                observations: crank_observations(0),
            },
        );
    }
    env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: final_slot,
            observations: crank_observations(0),
        },
    );
    assert!(cert_is_current(&env, observer));

    let before = env.market_state().1;
    let observer_cert_before = health_cert(&env.portfolio_state(observer));
    let close_cu = env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        -OPEN_Q,
        WINNING_PRICE,
        0,
    );
    assert_cu_within(
        "public final reduction creating a pending loss obligation",
        close_cu,
        TRADE_CU_LIMIT,
    );

    let after = env.market_state().1;
    let loser_after = env.portfolio_state(loser);
    let observer_after = env.portfolio_state(observer);
    let pending = close_progress(&loser_after);
    assert!(pending.active && !pending.finalized);
    assert_eq!(pending.residual_remaining, 250);
    assert_eq!(loser_after.capital.get(), 0);
    assert_eq!(loser_after.pnl.get(), -250);
    let loser_cert = health_cert(&loser_after);
    assert!(loser_cert.valid);
    assert_eq!(loser_cert.certified_equity, -250);
    assert_eq!(loser_cert.certified_initial_req, 0);
    assert_eq!(loser_cert.certified_maintenance_req, 0);
    assert_eq!(loser_cert.certified_liq_deficit, 250);
    assert!(cert_is_current(&env, loser));
    assert_eq!(
        after.assets[0].pending_obligation_count_long
            + after.assets[0].pending_obligation_count_short,
        1
    );
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(after.risk_epoch, before.risk_epoch + 2);
    assert_eq!(after.asset_set_epoch, before.asset_set_epoch);
    assert_eq!(
        health_cert(&observer_after),
        observer_cert_before,
        "the close must not rewrite an unrelated account while global risk changes"
    );
    assert!(
        health_cert(&observer_after).cert_risk_epoch < after.risk_epoch,
        "the composed source-risk writes must stale unrelated certificates"
    );
    assert!(!cert_is_current(&env, observer));

    let market_before_reject = env.svm.get_account(&env.market).unwrap();
    let loser_before_reject = env.svm.get_account(&loser).unwrap();
    let observer_before_reject = env.svm.get_account(&observer).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.try_trade_asset_with_cu(
        0,
        &loser_owner,
        loser,
        &observer_owner,
        observer,
        POS_SCALE as i128,
        WINNING_PRICE,
        0,
    );
    assert!(
        rejected.is_err(),
        "the invalidated pending-obligation account must not use its old health state"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_reject
    );
    assert_eq!(env.svm.get_account(&loser).unwrap(), loser_before_reject);
    assert_eq!(
        env.svm.get_account(&observer).unwrap(),
        observer_before_reject
    );

    let destination = env.token_account(observer_owner.pubkey(), 0);
    let group_before_withdraw = env.market_state().1;
    let loser_before_withdraw = env.svm.get_account(&loser).unwrap();
    let vault_before_withdraw = env.token_amount(env.vault);
    env.svm.expire_blockhash();
    let stale_flat_exit_cu = env
        .send(
            env.withdraw_ix(observer, 1),
            vec![
                AccountMeta::new(observer_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(observer, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&observer_owner],
        )
        .expect("a stale but flat account must retain its state-independent principal exit");
    assert_cu_within(
        "stale flat principal exit after pending-obligation creation",
        stale_flat_exit_cu,
        CUSTODY_CU_LIMIT,
    );
    let group_after_withdraw = env.market_state().1;
    assert_eq!(env.portfolio_state(observer).capital.get(), 0);
    assert_eq!(env.token_amount(destination), 1);
    assert_eq!(env.token_amount(env.vault), vault_before_withdraw - 1);
    assert_eq!(group_after_withdraw.c_tot, group_before_withdraw.c_tot - 1);
    assert_eq!(group_after_withdraw.vault, group_before_withdraw.vault - 1);
    assert_eq!(
        group_after_withdraw.assets[0].pending_obligation_count_long
            + group_after_withdraw.assets[0].pending_obligation_count_short,
        1
    );
    assert_eq!(env.svm.get_account(&loser).unwrap(), loser_before_withdraw);
}

#[test]
fn v16_attack_asset_append_invalidates_public_released_pnl_cert() {
    const INIT_FEE: u128 = 1;
    let (mut env, owner, portfolio) = setup_public_released_pnl_certificate();
    env.update_market_init_fee_policy_with_cu(INIT_FEE);
    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(portfolio));
    let creator = Keypair::new();
    let creator_key = creator.pubkey();

    env.svm.warp_to_slot(3);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        3,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        INIT_FEE,
    );

    let after = env.market_state().1;
    let stale = health_cert(&env.portfolio_state(portfolio));
    assert!(
        after.asset_set_epoch > before.asset_set_epoch,
        "physical growth plus activation must advance the asset-set epoch"
    );
    assert!(
        after.risk_epoch > before.risk_epoch,
        "physical growth plus activation must advance the risk epoch"
    );
    assert_eq!(after.oracle_epoch, before.oracle_epoch);
    assert_eq!(after.funding_epoch, before.funding_epoch);
    assert_eq!(
        stale, cert_before,
        "append does not rewrite unrelated portfolios"
    );
    assert!(
        stale.cert_asset_set_epoch < after.asset_set_epoch,
        "the appended asset must invalidate the old asset-set certificate"
    );

    assert_stale_conversion_rolls_back(&mut env, &owner, portfolio);
    refresh_and_convert_public_claim(&mut env, &owner, portfolio);
}

#[test]
fn v16_attack_target_and_funding_epochs_invalidate_public_released_pnl_cert() {
    const PREMIUM_MARK: u64 = 2_000_000;
    const FUNDING_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    let (mut env, claim_owner, claimant) = setup_public_released_pnl_certificate();

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    let open_price = env.market_state().1.assets[0].effective_price;
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        FUNDING_SIZE_Q,
        open_price,
        0,
    );

    // Stage and activate a premium funding mark. This first interval moves the
    // effective price and therefore advances oracle_epoch; refresh the claimant
    // afterward so the next interval starts with every certificate key current.
    env.svm.warp_to_slot(3);
    env.push_auth_mark_for_asset_as_admin(0, 3, PREMIUM_MARK);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    env.crank(
        claimant,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert!(cert_is_current(&env, claimant));

    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(claimant));
    assert_eq!(cert_before.cert_funding_epoch, before.funding_epoch);
    assert_eq!(cert_before.cert_oracle_epoch, before.oracle_epoch);

    let current_effective_price = before.assets[0].effective_price;
    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(0, 4, current_effective_price);

    // Target publication invalidates the oracle certificate immediately. The next slot's
    // risk-reducing trade commits the premium interval and independently advances funding_epoch.
    // The engine's kernel_cert_is_current contract proves each key is individually necessary;
    // this public route demonstrates the reachable composition without manufacturing state.
    env.svm.warp_to_slot(5);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        current_effective_price,
        0,
    );

    let after = env.market_state().1;
    let stale = health_cert(&env.portfolio_state(claimant));
    assert_eq!(
        after.assets[0].effective_price, current_effective_price,
        "the funding interval must have zero effective-price movement"
    );
    assert_eq!(after.oracle_epoch, before.oracle_epoch + 1);
    assert_eq!(
        after.funding_epoch,
        before.funding_epoch + 1,
        "the committed premium interval must advance funding_epoch exactly once"
    );
    assert_ne!(
        after.assets[0].f_long_num, before.assets[0].f_long_num,
        "the funding epoch bump must correspond to a real funding-ledger change"
    );
    assert_eq!(
        stale, cert_before,
        "another account's funding accrual does not rewrite the claimant"
    );
    assert!(
        stale.cert_funding_epoch < after.funding_epoch,
        "the old claim certificate must be stale on the funding key"
    );
    assert!(
        stale.cert_oracle_epoch < after.oracle_epoch,
        "the old claim certificate must also be stale on the published-target key"
    );

    assert_stale_conversion_rolls_back(&mut env, &claim_owner, claimant);
    refresh_and_convert_public_claim(&mut env, &claim_owner, claimant);
}

#[test]
fn v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh() {
    let (mut env, owner, portfolio) = setup_public_released_pnl_certificate();

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long = env.create_portfolio(&crank_long_owner);
    let crank_short = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long, 10_000_000);
    env.deposit(&crank_short_owner, crank_short, 10_000_000);
    let price = env.market_state().1.assets[0].effective_price;
    env.trade_with_cu(
        &crank_long_owner,
        crank_long,
        &crank_short_owner,
        crank_short,
        POS_SCALE as i128,
        price,
        0,
    );
    assert_eq!(
        env.portfolio_state(portfolio).pnl.get(),
        PUBLIC_RELEASED_PNL as i128,
        "setup must realize PnL through public trades"
    );
    assert!(cert_is_current(&env, portfolio));

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, price + 1);
    env.crank(
        crank_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let (_, stale_group) = env.market_state();
    assert!(
        health_cert(&env.portfolio_state(portfolio)).cert_oracle_epoch < stale_group.oracle_epoch,
        "auth mark update must make the existing cert stale"
    );

    assert_stale_conversion_rolls_back(&mut env, &owner, portfolio);
    refresh_and_convert_public_claim(&mut env, &owner, portfolio);
}

#[test]
fn v16_attack_target_only_lag_invalidates_unrelated_single_trade_cert() {
    const PRICE: u64 = 100;
    const TARGET: u64 = 90;
    const ASSET1_SIZE_Q: i128 = (10 * POS_SCALE) as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        ASSET1_SIZE_Q,
        PRICE,
        0,
    );

    let (_, before_target) = env.market_state();
    let long_before = env.portfolio_state(long_account);
    let short_before = env.portfolio_state(short_account);
    assert_eq!(
        health_cert(&long_before).cert_oracle_epoch,
        before_target.oracle_epoch
    );
    assert_eq!(
        health_cert(&short_before).cert_oracle_epoch,
        before_target.oracle_epoch
    );

    let cranker_owner = Keypair::new();
    let cranker_account = env.create_portfolio(&cranker_owner);
    env.push_auth_mark_for_asset_as_admin(1, 0, TARGET);
    env.crank(
        cranker_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(1),
        },
    );

    let (_, lagged_group) = env.market_state();
    let stale_long = env.portfolio_state(long_account);
    let stale_short = env.portfolio_state(short_account);
    assert_eq!(lagged_group.assets[1].raw_oracle_target_price, TARGET);
    assert_eq!(
        lagged_group.assets[1].effective_price, PRICE,
        "same-slot crank must create target-only lag without an effective-price move"
    );
    assert_eq!(lagged_group.oracle_epoch, before_target.oracle_epoch + 1);
    assert!(
        health_cert(&stale_long).cert_oracle_epoch < lagged_group.oracle_epoch
            && health_cert(&stale_short).cert_oracle_epoch < lagged_group.oracle_epoch,
        "target-only lag must invalidate every prior portfolio certificate in O(1)"
    );

    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    assert_cu_within(
        "unrelated single trade after target-only lag",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, after_trade_group) = env.market_state();
    let long_after = env.portfolio_state(long_account);
    let short_after = env.portfolio_state(short_account);
    let long_cert = health_cert(&long_after);
    let short_cert = health_cert(&short_after);
    assert_eq!(long_cert.cert_oracle_epoch, after_trade_group.oracle_epoch);
    assert_eq!(short_cert.cert_oracle_epoch, after_trade_group.oracle_epoch);
    assert_eq!(
        long_cert.certified_maintenance_req,
        short_cert.certified_maintenance_req + 100,
        "the unrelated trade must retain asset 1's adverse long lag penalty"
    );
    assert_eq!(long_cert.certified_maintenance_req, 1_200);
    assert_eq!(short_cert.certified_maintenance_req, 1_100);
}
