//! INV-027 - Protected principal seniority.
//!
//! Normative obligation: Junior value and fees cannot outrank protected principal or pending senior obligations.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_bpf_public_crystallized_loss_budget_credits_only_fresh_lp_principal`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_bpf_public_crystallized_loss_budget_credits_only_fresh_lp_principal() {
    const OPEN_PRICE: u64 = 1_000;
    const ADVERSE_PRICE: u64 = 900;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);

    let trader_owner = Keypair::new();
    let trader = env.create_portfolio(&trader_owner);
    let first_lp_owner = Keypair::new();
    let first_lp = env.create_portfolio(&first_lp_owner);
    let fresh_lp_owner = Keypair::new();
    let fresh_lp = env.create_portfolio(&fresh_lp_owner);
    env.deposit(&trader_owner, trader, 10_000);
    env.deposit(&first_lp_owner, first_lp, 10_000);
    env.deposit(&fresh_lp_owner, fresh_lp, 10_000);

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &first_lp_owner,
        first_lp,
        POS_SCALE as i128,
        OPEN_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, ADVERSE_PRICE);
    for portfolio in [trader, first_lp] {
        env.svm.expire_blockhash();
        env.send(
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
        )
        .expect("authenticated adverse mark refresh");
    }

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &first_lp_owner,
        first_lp,
        -(POS_SCALE as i128),
        ADVERSE_PRICE,
        0,
    );
    let after_loss = env.portfolio_state(trader);
    let crystallized = after_loss.residual_crystallized_loss_atoms_total.get();
    assert!(
        crystallized > 0,
        "closing the adverse position must crystallize real principal loss"
    );
    assert_eq!(
        after_loss.residual_spent_principal_atoms_total.get(),
        0,
        "closing risk creates but does not spend the residual reward budget"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&after_loss)),
        "the losing episode is fully closed before the reward-bearing trade"
    );

    let fresh_lp_before = env.portfolio_state(fresh_lp);
    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &fresh_lp_owner,
        fresh_lp,
        POS_SCALE as i128,
        ADVERSE_PRICE,
        0,
    );

    let trader_after = env.portfolio_state(trader);
    let fresh_lp_after = env.portfolio_state(fresh_lp);
    let spent = trader_after.residual_spent_principal_atoms_total.get();
    let received = fresh_lp_after
        .residual_received_atoms_total
        .get()
        .checked_sub(fresh_lp_before.residual_received_atoms_total.get())
        .expect("monotonic recipient counter");
    assert!(
        spent > 0,
        "fresh risk consumes a nonzero real-principal budget"
    );
    assert_eq!(
        received, spent,
        "the independent LP receives exactly the trader's consumed budget"
    );
    assert!(
        spent <= crystallized,
        "public trading cannot spend more than the actual crystallized loss"
    );
    assert_eq!(
        trader_after.residual_crystallized_loss_atoms_total.get(),
        crystallized,
        "spending a reward budget never manufactures another crystallized loss"
    );
}
