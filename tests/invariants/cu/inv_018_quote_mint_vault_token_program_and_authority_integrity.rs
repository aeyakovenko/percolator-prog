//! INV-018 - Quote mint, vault, token-program, and authority integrity.
//!
//! Normative obligation: External token movement stays bound to canonical custody and token identities.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_base_unit_mints_reject_post_resolve_with_user_value`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_base_unit_mints_reject_post_resolve_with_user_value() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let replacement_primary = env.create_mint();
    let replacement_secondary = env.create_mint();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: replacement_primary.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(replacement_primary, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    );
    let err =
        rejected.expect_err("base-unit rails must not rotate while resolved user value remains");
    assert!(
        err.contains("Custom(21)"),
        "post-resolve base-unit rotation with user value should fail as EngineLockActive, got {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected post-resolve base-unit rotation leaves terminal market state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected post-resolve base-unit rotation leaves payout vault custody untouched"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected post-resolve base-unit rotation leaves the user claim untouched"
    );
    assert_eq!(
        env.market_state().0.collateral_mint,
        env.mint.to_bytes(),
        "primary payout rail remains pinned to the funded mint"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        [0u8; 32],
        "no secondary rail is installed by the rejected post-resolve rotation"
    );

    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "resolved user payout remains live on the original funded rail"
    );
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(env.market_state().1.vault, 0);
}
