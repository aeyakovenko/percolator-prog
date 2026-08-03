//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_sync_maintenance_rejects_cross_market_payer_substitution`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_sync_maintenance_rejects_cross_market_payer_substitution() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let foreign_owner = Keypair::new();
    let foreign_payer = env.create_portfolio(&foreign_owner);
    env.deposit(&foreign_owner, foreign_payer, 100_000_000);

    let params = V16CuMarketParams {
        maintenance_fee_per_slot: 58,
        ..V16CuMarketParams::default()
    };
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let local_owner = Keypair::new();
    let local_payer = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_owner,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &local_owner,
        local_payer,
        100_000_000,
    );

    env.svm.warp_to_slot(10);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let foreign_before = env.svm.get_account(&foreign_payer).unwrap();
    let local_before = env.svm.get_account(&local_payer).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        vec![
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign_payer, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "SyncMaintenanceFee must reject a market-A payer under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&foreign_payer).unwrap(),
        foreign_before,
        "foreign payer is not charged, closed, or re-certified"
    );
    assert_eq!(
        env.svm.get_account(&local_payer).unwrap(),
        local_before,
        "local market-B payer is not touched by the rejected substitution"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let sync_cu = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        vec![
            AccountMeta::new(market_b, false),
            AccountMeta::new(local_payer, false),
        ],
        &[],
    )
    .expect("same-market SyncMaintenanceFee remains live");
    assert_cu_within(
        "SyncMaintenanceFee cross-market payer control",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );
    let local_after = state::read_portfolio(&env.svm.get_account(&local_payer).unwrap().data)
        .expect("market-B local payer");
    assert_eq!(local_after.last_fee_slot.get(), 10);
    assert_eq!(local_after.capital.get(), 100_000_000 - 580);
    let (_, market_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(market_b_after.insurance, 580);
    assert_eq!(market_b_after.vault as u64, env.token_amount(vault_b));
}
