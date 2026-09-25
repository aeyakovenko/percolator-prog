//! Public-route regressions for issues reported against percolator-prog.
//!
//! Each test drives the deployed SBF through LiteSVM with public instructions only and pins the
//! behavior the corresponding issue requires. Retained transactions are built before the state
//! they authorized changes; every build carries a distinct compute-unit price, so a rejected
//! retry is rejected by the program, not by duplicate-signature filtering.

#[allow(dead_code)]
mod support;

use percolator_prog::processor;
use support::v16_svm::{MarketConfig, V16Svm};

fn env(seed: u8, config: MarketConfig) -> V16Svm {
    V16Svm::new([seed; 32], config)
}

/// Issue #170: matched, value-free spent-backing history left by a round trip must not block
/// restart of an emptied asset forever; tag 70, signed by the asset's backing authority, clears
/// it. The history is installed directly (the engine-level test drives it through public
/// transitions).
#[test]
fn issue170_backing_authority_canonicalizes_history_then_reuse_succeeds() {
    use percolator::{BackingBucketStatusV16, BackingBucketV16, BackingBucketV16Account};
    use percolator::{SourceCreditStateV16, SourceCreditStateV16Account, BOUND_SCALE};
    use percolator_prog::constants::{ASSET_ORACLE_WRAPPER_LEN, MARKET_ASSET_SLOT_LEN};
    use percolator_prog::constants::{MARKET_GROUP_LEN, MARKET_GROUP_OFF};
    const ASSET: u16 = 1;
    const PROVIDER: usize = 1;
    const OTHER: usize = 2;
    let mut env = env(17, MarketConfig::default());
    env.warp_to_slot(1);
    env.retire_asset(ASSET, 1).expect("retire empty asset");
    env.update_asset_authority_from_admin(ASSET, processor::ASSET_AUTH_BACKING_BUCKET, PROVIDER)
        .expect("hand the backing role to the provider");
    let market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;

    let engine = MARKET_GROUP_OFF
        + MARKET_GROUP_LEN
        + ASSET as usize * MARKET_ASSET_SLOT_LEN
        + ASSET_ORACLE_WRAPPER_LEN;
    let history = 25 * BOUND_SCALE;
    let mut account = env.svm.get_account(&env.market).unwrap();
    let src_off = engine + core::mem::offset_of!(percolator::EngineAssetSlotV16Account, source_credit_long);
    let bucket_off = engine + core::mem::offset_of!(percolator::EngineAssetSlotV16Account, backing_long);
    let src = SourceCreditStateV16Account::from_runtime(&SourceCreditStateV16 {
        spent_backing_num: history,
        provider_receivable_num: history,
        credit_rate_num: percolator::CREDIT_RATE_SCALE,
        ..SourceCreditStateV16::EMPTY
    });
    let bucket = BackingBucketV16Account::from_runtime(&BackingBucketV16 {
        market_id,
        consumed_liened_backing_num: history,
        status: BackingBucketStatusV16::Expired,
        ..BackingBucketV16::EMPTY
    });
    account.data[src_off..src_off + core::mem::size_of::<SourceCreditStateV16Account>()]
        .copy_from_slice(bytemuck::bytes_of(&src));
    account.data[bucket_off..bucket_off + core::mem::size_of::<BackingBucketV16Account>()]
        .copy_from_slice(bytemuck::bytes_of(&bucket));
    env.svm.set_account(env.market, account).unwrap();

    // A retired slot returns through lifecycle ACTIVATE (reuse); matched history blocks it.
    let slot = 1_000_000;
    env.warp_to_slot(slot);
    let reuse = env.build_retained_activate_asset(ASSET, slot, support::v16_svm::INITIAL_PRICE);
    assert!(env.land_retained(reuse).is_err(), "matched history blocks reuse");
    let before = env.market_data(false);
    assert!(
        env.canonicalize_spent_backing_history_for_actor(OTHER, ASSET).is_err(),
        "only the asset's backing authority may clear its history"
    );
    assert_eq!(env.market_data(false), before);
    env.canonicalize_spent_backing_history_for_actor(PROVIDER, ASSET)
        .expect("backing authority clears value-free history");
    env.expire_blockhash();
    let reuse = env.build_retained_activate_asset(ASSET, slot, support::v16_svm::INITIAL_PRICE);
    env.land_retained(reuse)
        .expect("reuse succeeds once only history remained");
}

