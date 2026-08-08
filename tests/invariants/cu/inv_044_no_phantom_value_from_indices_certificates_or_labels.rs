//! INV-044 - No phantom value from indices, certificates, or labels.
//!
//! Normative obligation: derived labels and crank classifications cannot create
//! token stock, withdrawable value, health, or senior capital by themselves.
//!
//! Evidence in this file (I/C): a third party permissionlessly asks the public
//! crank route to make B-settlement progress on a flat solvent account. Whether
//! the engine returns non-progress or a no-op success, the account's capital,
//! vault, `c_tot`, insurance, and owner withdrawability must remain exact.
//! Additional no-phantom-value coverage lives in INV-025, INV-026, INV-069, and
//! INV-070.

use super::*;

#[test]
fn v16_program_permissionless_settle_b_on_healthy_account_is_safe_noop() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let (_, group_before) = env.market_state();
    let capital_before = env.portfolio_state(portfolio).capital.get();
    assert_eq!(capital_before, 1_000);

    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    let _ = env.send(
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
    );

    let (_, group_after) = env.market_state();
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
    assert_eq!(group_after.vault, group_before.vault);
    assert_eq!(group_after.c_tot, group_before.c_tot);
    assert_eq!(group_after.insurance, group_before.insurance);

    let (dest, _) = env.withdraw_with_cu(&owner, portfolio, 1_000);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "derived crank labels cannot trap an otherwise withdrawable account"
    );
}

// security.md sweep — deposit with parked pnl (#32/#33): depositing while holding junior (parked) pnl
// must credit capital exactly and leave the pnl and its residual backing untouched. No double-count,
// no disturbance of the junior pnl, conservation holds.
#[test]
fn v16_attack_deposit_with_parked_pnl_clean() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    // price up -> long accrues parked pnl; settle.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank_steps_after_market_catchup(
        sh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        lo,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(11);
    for p in [sh, lo] {
        env.crank(
            p,
            ProgInstruction::PermissionlessCrank {
                now_slot: 11,
                observations: crank_observations(0),
            },
        );
    }
    let a0 = env.portfolio_state(lo);
    assert!(a0.pnl.get() > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;

    // deposit MORE while holding the parked pnl.
    env.svm.expire_blockhash();
    env.deposit(&lo_owner, lo, 500_000);
    let a1 = env.portfolio_state(lo);
    let (_, g1) = env.market_state();
    assert_eq!(
        a1.capital.get(),
        a0.capital.get() + 500_000,
        "capital credited exactly by the deposit"
    );
    assert_eq!(
        a1.pnl.get(),
        a0.pnl.get(),
        "parked pnl UNCHANGED by the deposit (no double-count/disturbance)"
    );
    assert_eq!(
        g1.vault,
        g0.vault + 500_000,
        "vault grew by exactly the deposit"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot + 500_000,
        "c_tot grew by exactly the deposit"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault balance"
    );
    // the parked pnl is still backed by (at least) the same residual.
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    assert_eq!(
        resid1, resid0,
        "residual backing of the junior pnl unchanged by the deposit"
    );
    assert!(
        resid1 >= a1.pnl.get().max(0),
        "junior pnl still backed by residual"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}
