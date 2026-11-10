//! Probe: admin-driven asset append must be able to grow the market account past 10 KiB.
//!
//! Each append grows the market account by exactly one asset slot (1813 bytes), far below the
//! runtime's `MAX_PERMITTED_DATA_INCREASE` (10_240 bytes) per transaction. The only way an append
//! can fail with `InvalidRealloc` is if the program's own pre-check misreads the account's
//! original data length.
//!
//! Run against a specific SBF artifact with `PERCOLATOR_FUZZ_SBF=<path>`.

#[allow(dead_code)]
mod support;

use percolator_prog::state;
use support::v16_svm::{MarketConfig, V16Svm, ASSET_COUNT, INITIAL_PRICE};

const MAX_PERMITTED_DATA_INCREASE: usize = 10_240;
// Clear the engine's asset-activation cooldown between appends.
const SLOTS_BETWEEN_APPENDS: u64 = 1_000_000;

#[test]
fn appending_assets_grows_market_account_past_10_kib() {
    let mut svm = V16Svm::new([11u8; 32], MarketConfig::default());
    println!("program artifact hash: {}", svm.loaded_program_hash);

    let start = svm.svm.get_account(&svm.market).expect("market").data.len();
    println!("market account starts at {start} bytes (capacity {ASSET_COUNT})");

    let mut failures = Vec::new();
    for _ in 0..6 {
        let next_slot = svm.current_slot() + SLOTS_BETWEEN_APPENDS;
        svm.warp_to_slot(next_slot);
        let configured = svm.primary_market_state().1.config.max_market_slots as usize;
        let asset_index = configured as u16;
        let before = svm.svm.get_account(&svm.market).expect("market").data.len();
        let new_len = state::market_account_len_for_capacity(configured + 1).expect("len");
        let growth = new_len.saturating_sub(before);
        assert!(
            growth <= MAX_PERMITTED_DATA_INCREASE,
            "single-step growth is runtime-legal"
        );

        let tx = svm.build_retained_activate_asset(asset_index, next_slot, INITIAL_PRICE);
        let outcome = svm.svm.send_transaction(tx);
        let after = svm.svm.get_account(&svm.market).expect("market").data.len();
        let summary = match &outcome {
            Ok(_) => "ok".to_string(),
            Err(failed) => format!("{:?}", failed.err),
        };
        println!(
            "append asset {asset_index}: before={before} new_len={new_len} (+{growth}) after={after} -> {summary}"
        );
        if let Err(failed) = &outcome {
            for line in &failed.meta.logs {
                println!("    log: {line}");
            }
            failures.push((asset_index, new_len, summary));
            break;
        }
    }

    assert!(
        failures.is_empty(),
        "runtime-legal appends were rejected by the program: {failures:?}"
    );
}
