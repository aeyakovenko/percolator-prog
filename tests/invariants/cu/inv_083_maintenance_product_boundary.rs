//! INV-083/077/073: a publicly accrued fee wider than u128 must clip to available
//! capital, consume its elapsed interval, and leave the credited reward withdrawable.

use super::*;
use num_bigint::BigUint;
use num_traits::ToPrimitive;

#[test]
fn v16_program_maintenance_u128_product_boundary_preserves_bounded_owner_exit() {
    // The first overflowing product is exactly 2^128, so truncation would charge zero.
    const RATE: u128 = 1u128 << percolator::MAX_PROTOCOL_FEE_ABS.ilog2();
    const PRINCIPAL: u128 = percolator::MAX_VAULT_TVL - 1;
    const SHARE_BPS: u16 = 3_333;
    let last_fitting_slot = u64::try_from(u128::MAX / RATE).unwrap();
    assert!(RATE <= percolator::MAX_PROTOCOL_FEE_ABS);
    assert_eq!(
        BigUint::from(RATE) * BigUint::from(last_fitting_slot + 1),
        BigUint::from(1u8) << 128usize
    );
    let mut peak_cu = 0;

    for slot in [last_fitting_slot, last_fitting_slot + 1, u64::MAX] {
        let nominal = BigUint::from(RATE) * BigUint::from(slot);
        assert_eq!(
            nominal > BigUint::from(u128::MAX),
            slot > last_fitting_slot,
            "the corpus must straddle the deployed U256-to-u128 saturation boundary"
        );
        let charged = nominal.min(BigUint::from(PRINCIPAL)).to_u128().unwrap();
        let reward = (BigUint::from(charged) * BigUint::from(SHARE_BPS) / BigUint::from(10_000u16))
            .to_u128()
            .unwrap();
        let retained = charged - reward;
        assert_eq!(charged, PRINCIPAL);
        assert!(reward > 0 && retained > 0);

        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            maintenance_fee_per_slot: RATE,
            ..V16CuMarketParams::default()
        });
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        let source = env.deposit(&owner, portfolio, PRINCIPAL);
        env.update_maintenance_fee_policy_with_cu(SHARE_BPS);
        assert_eq!(env.portfolio_state(portfolio).last_fee_slot.get(), 0);
        assert_eq!(env.portfolio_state(portfolio).capital.get(), PRINCIPAL);
        let custody_keys = [source, env.vault, env.mint, owner.pubkey()];
        let custody_before = custody_keys.map(|key| env.svm.get_account(&key));

        set_test_clock(&mut env, slot, 100);
        let cu = env.sync_maintenance_fee_with_cu(portfolio, Some(portfolio), slot);
        assert_cu_within("wide maintenance product", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        let account = env.portfolio_state(portfolio);
        let group = env.market_state().1;
        assert_eq!(account.last_fee_slot.get(), slot);
        assert_eq!(account.capital.get(), reward);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(group.c_tot, reward);
        assert_eq!(group.insurance, retained);
        assert_eq!(group.vault, PRINCIPAL);
        assert_eq!(group.c_tot + group.insurance, group.vault);
        assert_eq!(group.materialized_portfolio_count, 1);
        assert_domain_budget_remaining_total_consistent(&group, "wide maintenance product");
        assert_eq!(
            custody_keys.map(|key| env.svm.get_account(&key)),
            custody_before,
            "fee reclassification cannot move SPL tokens or owner lamports"
        );

        let replay_keys = [env.market, portfolio, source, env.vault, env.mint];
        let after_sync = replay_keys.map(|key| env.svm.get_account(&key));
        env.svm.expire_blockhash();
        let cu = env.sync_maintenance_fee_with_cu(portfolio, Some(portfolio), slot);
        assert_cu_within("wide maintenance same-slot retry", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        assert_eq!(
            replay_keys.map(|key| env.svm.get_account(&key)),
            after_sync,
            "even an overflowing nominal fee must consume the entire elapsed interval"
        );

        let (destination, cu) = env.withdraw_with_cu(&owner, portfolio, reward);
        assert_cu_within("owner exit after wide maintenance", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        assert_eq!(u128::from(env.token_amount(destination)), reward);
        assert_eq!(u128::from(env.token_amount(env.vault)), retained);
        assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
        let cu = env.close_portfolio_with_cu(&owner, portfolio);
        assert_cu_within("close after wide maintenance", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        assert!(env
            .svm
            .get_account(&portfolio)
            .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
        let terminal = env.market_state().1;
        assert_eq!(terminal.materialized_portfolio_count, 0);
        assert_eq!(terminal.c_tot, 0);
        assert_eq!(terminal.insurance, retained);
        assert_eq!(terminal.vault, retained);
    }

    println!(
        "INV-083 maintenance product: last fitting slot={last_fitting_slot}, worlds=3, peak={peak_cu} CU"
    );
}
