//! INV-033 - Insurance-backed lien single classification.
//!
//! Normative obligation: an insurance-backed source-credit lien, if exposed by a
//! public route, must be classified exactly once as insurance backing and never
//! also as counterparty backing or generic support.
//!
//! Current wrapper boundary: public domain-insurance top-up funds insurance
//! budgets, but it does not expose the engine's insurance-credit reservation
//! primitive. This test therefore locks down the deployed public behavior:
//! ordinary risk-increase may create a counterparty-backed source lien when a
//! fresh backing bucket exists, but the same route must not silently consume
//! unreserved domain insurance or populate the insurance-backed lien fields.

use super::*;

#[derive(Debug)]
struct SourceLienClassification {
    trade_succeeded: bool,
    source_claim_counterparty_liened_num: u128,
    source_claim_insurance_liened_num: u128,
    source_lien_counterparty_backing_num: u128,
    source_lien_insurance_backing_num: u128,
    market_valid_liened_backing_num: u128,
    market_insurance_credit_reserved_num: u128,
    market_valid_liened_insurance_num: u128,
    domain_insurance_budget: u128,
}

fn run_public_source_lien_classification(
    use_counterparty_backing: bool,
) -> SourceLienClassification {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const SOURCE_DOMAIN: usize = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);

    if use_counterparty_backing {
        env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 150, 10);
    } else {
        let admin = env.admin.insecure_clone();
        env.top_up_insurance_domain_with_authority(&admin, SOURCE_DOMAIN as u16, 150);
    }

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let before_vault = env.svm.get_account(&env.vault).unwrap();
    let trade_result = env.try_trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );

    if use_counterparty_backing {
        assert!(
            trade_result.is_ok(),
            "fresh counterparty backing should admit the source-credit risk increase: {trade_result:?}",
        );
    } else {
        assert!(
            trade_result.is_err(),
            "unreserved domain insurance must not be silently consumed as source-credit backing",
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before_market,
            "rejected insurance-only source-credit attempt rewrote market state",
        );
        assert_eq!(
            env.svm.get_account(&cross_account).unwrap(),
            before_cross,
            "rejected insurance-only source-credit attempt rewrote the trader portfolio",
        );
        assert_eq!(
            env.svm.get_account(&counterparty_account).unwrap(),
            before_counterparty,
            "rejected insurance-only source-credit attempt rewrote the counterparty portfolio",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            before_vault,
            "rejected insurance-only source-credit attempt moved vault tokens",
        );
    }

    let cross_after = env.portfolio_state(cross_account);
    let source = state::portfolio_source_domain(&cross_after, SOURCE_DOMAIN);
    let (_, group_after) = env.market_state();
    SourceLienClassification {
        trade_succeeded: trade_result.is_ok(),
        source_claim_counterparty_liened_num: source.source_claim_counterparty_liened_num.get(),
        source_claim_insurance_liened_num: source.source_claim_insurance_liened_num.get(),
        source_lien_counterparty_backing_num: source.source_lien_counterparty_backing_num.get(),
        source_lien_insurance_backing_num: source.source_lien_insurance_backing_num.get(),
        market_valid_liened_backing_num: group_after.source_credit[SOURCE_DOMAIN]
            .valid_liened_backing_num,
        market_insurance_credit_reserved_num: group_after.source_credit[SOURCE_DOMAIN]
            .insurance_credit_reserved_num,
        market_valid_liened_insurance_num: group_after.source_credit[SOURCE_DOMAIN]
            .valid_liened_insurance_num,
        domain_insurance_budget: group_after.insurance_domain_budget[SOURCE_DOMAIN],
    }
}

#[test]
fn v16_program_public_source_lien_classification_never_double_counts_insurance() {
    let counterparty = run_public_source_lien_classification(true);
    assert!(counterparty.trade_succeeded);
    assert!(
        counterparty.source_claim_counterparty_liened_num > 0,
        "control route must create a real counterparty-backed claim lien",
    );
    assert!(
        counterparty.source_lien_counterparty_backing_num > 0,
        "control route must reserve real counterparty backing",
    );
    assert_eq!(
        counterparty.source_claim_insurance_liened_num, 0,
        "counterparty-backed route must not also classify the same claim as insurance-backed",
    );
    assert_eq!(
        counterparty.source_lien_insurance_backing_num, 0,
        "counterparty-backed route must not reserve insurance backing",
    );
    assert!(counterparty.market_valid_liened_backing_num > 0);
    assert_eq!(counterparty.market_insurance_credit_reserved_num, 0);
    assert_eq!(counterparty.market_valid_liened_insurance_num, 0);

    let insurance_only = run_public_source_lien_classification(false);
    assert!(!insurance_only.trade_succeeded);
    assert!(
        insurance_only.domain_insurance_budget > 0,
        "negative route must be nonvacuous: domain insurance was actually funded",
    );
    assert_eq!(
        insurance_only.source_claim_counterparty_liened_num, 0,
        "rejected insurance-only route must not create counterparty claim liens",
    );
    assert_eq!(
        insurance_only.source_claim_insurance_liened_num, 0,
        "unreserved domain insurance must not become an account-local insurance claim lien",
    );
    assert_eq!(
        insurance_only.source_lien_counterparty_backing_num, 0,
        "rejected insurance-only route must not reserve counterparty backing",
    );
    assert_eq!(
        insurance_only.source_lien_insurance_backing_num, 0,
        "unreserved domain insurance must not become account-local insurance backing",
    );
    assert_eq!(insurance_only.market_valid_liened_backing_num, 0);
    assert_eq!(
        insurance_only.market_insurance_credit_reserved_num, 0,
        "public domain-insurance top-up must not expose the engine-only insurance-credit reservation",
    );
    assert_eq!(
        insurance_only.market_valid_liened_insurance_num, 0,
        "rejected route must not consume reserved insurance",
    );

    // This is an intentional API absence, not an untested public transition. The wrapper may
    // serialize the engine-owned reservation fields, but it cannot create or mutate them. A new
    // callsite makes the insurance-lien lifecycle publicly reachable and must reopen INV-033.
    let wrapper = include_str!("../../../src/v16_program.rs");
    for engine_method in [
        "reserve_insurance_credit_not_atomic(",
        "create_source_credit_lien_from_insurance_not_atomic(",
        "release_source_credit_lien_from_insurance_not_atomic(",
        "consume_source_credit_lien_from_insurance_not_atomic(",
        "impair_source_credit_lien_from_insurance_not_atomic(",
    ] {
        assert_eq!(
            wrapper.matches(engine_method).count(),
            0,
            "wrapper exposed engine-only insurance-lien method {engine_method}",
        );
    }

    crate::assert_certified_engine_pin("INV-033 engine-contract evidence");
}
