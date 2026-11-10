//! Public-route regressions for issues reported against percolator-prog.
//!
//! Each test drives the deployed SBF through LiteSVM with public instructions only and pins the
//! behavior the corresponding issue requires. Retained transactions are built before the state
//! they authorized changes; every build carries a distinct compute-unit price, so a rejected
//! retry is rejected by the program, not by duplicate-signature filtering.

#[allow(dead_code)]
mod support;

use support::v16_svm::{MarketConfig, V16Svm};

fn env(seed: u8, config: MarketConfig) -> V16Svm {
    V16Svm::new([seed; 32], config)
}

fn market_with_barrier(env: &mut V16Svm, short_side: bool) {
    use percolator_prog::constants::{ASSET_ORACLE_WRAPPER_LEN, MARKET_GROUP_LEN, MARKET_GROUP_OFF};
    let engine = MARKET_GROUP_OFF + MARKET_GROUP_LEN + ASSET_ORACLE_WRAPPER_LEN;
    let field = if short_side {
        core::mem::offset_of!(percolator::EngineAssetSlotV16Account, pending_domain_loss_barrier_short)
    } else {
        core::mem::offset_of!(percolator::EngineAssetSlotV16Account, pending_domain_loss_barrier_long)
    };
    let mut account = env.svm.get_account(&env.market).unwrap();
    account.data[engine + field..engine + field + 8].copy_from_slice(&1u64.to_le_bytes());
    env.svm.set_account(env.market, account).unwrap();
}

/// Issue #438: WithdrawInsuranceAsset debits both of an asset's insurance domains, so a pending
/// loss barrier on either domain must block it (hardening; the barrier is injected because no
/// public trace is known to reach it with all global loss counters clear).
#[test]
fn issue438_insurance_withdrawal_respects_both_domain_barriers() {
    // Control: with no barrier the same two-domain withdrawal succeeds.
    let mut control = env(43, MarketConfig::default());
    control.top_up_insurance_domain(0, 100).expect("fund long domain");
    control.top_up_insurance_domain(1, 100).expect("fund short domain");
    control
        .withdraw_insurance_asset_as_admin(0, 150)
        .expect("barrier-free withdrawal succeeds");
    for short_side in [false, true] {
        let mut env = env(43, MarketConfig::default());
        env.top_up_insurance_domain(0, 100).expect("fund long domain");
        env.top_up_insurance_domain(1, 100).expect("fund short domain");
        market_with_barrier(&mut env, short_side);
        assert!(
            env.withdraw_insurance_asset_as_admin(0, 150).is_err(),
            "short_side={short_side}: a pending barrier must block the two-domain debit"
        );
    }
}
