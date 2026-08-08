//! INV-023 - caller-input confinement for derived safety state.
//!
//! `CrankObservationHint` is discovery input: it tells the wrapper which oracle
//! accounts the caller supplied. It must not become an authority to partially
//! mutate market time, oracle checkpoints, funding, or account state when a later
//! caller-controlled hint proves malformed. These tests intentionally place a valid
//! observation before a bad one so the only correct outcome is full instruction
//! failure and exact SVM rollback.

use super::*;

fn assert_late_bad_crank_hint_rolls_back(label: &str, bad_tail: CrankObservationHint) {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.svm.warp_to_slot(5);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let slot_before = env.market_state().1.assets[0].slot_last;

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                bad_tail,
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "{label}: malformed late hint must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label}: failed crank rolls back market bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "{label}: failed crank rolls back portfolio bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{label}: failed crank cannot move custody",
    );

    env.svm.expire_blockhash();
    let valid_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("same public route succeeds once the hostile hint is removed");
    assert_cu_within(label, valid_cu, CRANK_CU_LIMIT);
    assert!(
        env.market_state().1.assets[0].slot_last > slot_before,
        "{label}: non-vacuous control proves the first hint would have advanced state",
    );
}

#[test]
fn v16_program_duplicate_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 duplicate crank hint",
        CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        },
    );
}

#[test]
fn v16_program_out_of_range_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 out-of-range crank hint",
        CrankObservationHint {
            asset_index: 1,
            oracle_accounts: 0,
        },
    );
}
