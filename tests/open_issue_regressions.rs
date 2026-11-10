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

fn maintenance_domains_after(syncs: &[u64]) -> (u128, u128) {
    let mut env = env(
        38,
        MarketConfig {
            maintenance_fee_per_slot: 1,
            ..MarketConfig::default()
        },
    );
    let start = env.current_slot();
    for &offset in syncs {
        env.warp_to_slot(start + offset);
        env.sync_maintenance_fee(0, start + offset)
            .expect("permissionless maintenance sync");
    }
    let (_, group) = env.primary_market_state();
    (group.insurance_domain_budget[0], group.insurance_domain_budget[1])
}

/// Issue #386: the long/short allocation of side-neutral maintenance revenue must not depend on
/// the (permissionless, attacker-chosen) sync cadence.
#[test]
fn issue386_maintenance_fee_split_is_cadence_independent() {
    let once = maintenance_domains_after(&[10]);
    let every_slot = maintenance_domains_after(&(1..=10).collect::<Vec<_>>());
    let mixed = maintenance_domains_after(&[3, 10]);
    assert_eq!(once.0 + once.1, 10, "the whole fee reaches asset-0 insurance");
    assert_eq!(once, every_slot, "ten one-atom syncs must split like one ten-atom sync");
    assert_eq!(once, mixed, "odd partitions must split like one sync");
    assert!(once.0.abs_diff(once.1) <= 1);
}
