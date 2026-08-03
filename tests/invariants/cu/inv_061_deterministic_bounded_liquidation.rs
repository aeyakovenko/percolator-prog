//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: Liquidation is deterministic, risk reducing, OI coherent, and bounded at maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_liquidation_reward_share_without_tail_still_progresses`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_liquidation_reward_share_without_tail_still_progresses() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper = Keypair::new();
    env.ensure_signer_account(keeper.pubkey());
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0,
        "setup must make the target liquidatable before the no-tail reward-share crank"
    );

    let (_, before) = env.market_state();
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(keeper.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[&keeper],
    );
    assert!(
        accepted.is_ok(),
        "reward-enabled liquidation must remain live when the keeper omits the optional reward tail: {accepted:?}"
    );

    let (_, after) = env.market_state();
    assert!(
        after.insurance > before.insurance,
        "without a reward tail, the liquidation fee is retained by insurance"
    );
    assert_eq!(
        after.vault, before.vault,
        "liquidation reward sharing is an internal fee split, not a vault mint"
    );
    assert_eq!(
        after.vault as u64,
        env.token_amount(env.vault),
        "vault accounting remains tied to SPL custody"
    );
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation"
    );
}
