//! INV-054 - Certificate epoch completeness.
//!
//! Normative obligation: Every health-relevant state change invalidates or conservatively downgrades certificates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public trade/mark/crank/close
//! sequences create a real source-backed released-PnL claim. Public oracle, source-credit,
//! lifecycle, and asset-set mutations then make its certificate stale. Favorable conversion must
//! reject with exact rollback until a permissionless public crank refreshes every certificate key.
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

fn refresh_and_convert_public_claim(env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey) {
    let now_slot = env.svm.get_sysvar::<Clock>().slot;
    env.crank(
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
fn v16_attack_funding_only_epoch_invalidates_public_released_pnl_cert() {
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

    let current_effective_price = env.market_state().1.assets[0].effective_price;
    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(0, 4, current_effective_price);
    let before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(claimant));
    assert_eq!(cert_before.cert_funding_epoch, before.funding_epoch);
    assert_eq!(cert_before.cert_oracle_epoch, before.oracle_epoch);

    // A risk-reducing public trade first books deterministic zero-move funding.
    // Unlike an observation-bearing crank, this route does not synchronize the
    // engine raw target, so a passing assertion below isolates funding_epoch.
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
    assert_eq!(
        after.oracle_epoch, before.oracle_epoch,
        "oracle_epoch must remain fixed so it cannot mask the funding key"
    );
    assert_eq!(
        after.funding_epoch,
        before.funding_epoch + 1,
        "the committed premium interval must advance funding_epoch exactly once"
    );
    assert_ne!(
        after.assets[0].f_long_num, before.assets[0].f_long_num,
        "the isolated epoch bump must correspond to a real funding-ledger change"
    );
    assert_eq!(
        stale, cert_before,
        "another account's funding accrual does not rewrite the claimant"
    );
    assert!(
        stale.cert_funding_epoch < after.funding_epoch,
        "the old claim certificate must be stale solely on the funding key"
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
