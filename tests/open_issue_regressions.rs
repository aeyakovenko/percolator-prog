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
