//! INV-005 - Authority incarnation binding.
//!
//! Normative obligation: Retained authority is scoped to the configured role and cannot be
//! exercised by an untrusted public caller.
//!
//! Evidence in this file (public SBF I plus exact rollback):
//! `v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers` submits the two
//! authority-only instruction families implicated by privileged deadline and insurance claims.
//! It requires both to reject before changing the market, SPL vault, or attacker destination.
//!
//! Guarantee boundary: this proves the alleged transition is not an unprivileged public attack.
//! It does not protect users from a compromised configured authority; operational deployments
//! must place that role behind their chosen multisignature or governance policy.

use super::*;

#[test]
fn v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let attacker_dest = env.token_account(attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let destination_before = env.svm.get_account(&attacker_dest).unwrap();

    env.svm.expire_blockhash();
    let withdrawal = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        withdrawal.is_err(),
        "an untrusted caller must not exercise the insurance-operator route"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        destination_before
    );

    env.svm.expire_blockhash();
    let policy = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            policy_sequence: u64::MAX,
            stale_slots: 1,
            force_close_delay_slots: 1,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&attacker],
    );
    assert!(
        policy.is_err(),
        "an untrusted caller must not rewrite stale or recovery deadlines"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        destination_before
    );
    assert_eq!(env.token_amount(attacker_dest), 0);
}
