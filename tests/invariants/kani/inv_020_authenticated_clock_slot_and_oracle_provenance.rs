//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: every selected leg in one composite price shares one authenticated
//! observation epoch. The deployed wrapper predicate must accept one-leg prices and reject every
//! two- or three-leg timestamp disagreement.
//!
//! Evidence in this file (P): Kani exhausts all full-width `i64` timestamp triples through the
//! exact production predicate. Public SBF tests separately bind this predicate to account parsing,
//! rollback/ignore semantics, terminal payout, and owner exit.

use percolator_prog::{
    error::PercolatorError,
    oracle_v16::{
        oracle_publish_time_is_fresh, oracle_publish_times_are_coherent,
        read_oracle_price_e6_from_bytes, CHAINLINK_STORE_PROGRAM_ID, PYTH_RECEIVER_PROGRAM_ID,
        SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID, SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
    },
};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

#[kani::proof]
fn kani_v16_composite_oracle_epochs_are_exactly_coherent() {
    let publish_times: [i64; 3] = kani::any();

    assert!(oracle_publish_times_are_coherent(&publish_times[..1]));
    assert_eq!(
        oracle_publish_times_are_coherent(&publish_times[..2]),
        publish_times[0] == publish_times[1]
    );
    assert_eq!(
        oracle_publish_times_are_coherent(&publish_times),
        publish_times[0] == publish_times[1] && publish_times[0] == publish_times[2]
    );
}

#[kani::proof]
fn kani_v16_empty_composite_epoch_is_invalid() {
    assert!(!oracle_publish_times_are_coherent(&[]));
}

#[kani::proof]
fn kani_v16_oracle_freshness_matches_full_width_elapsed_time() {
    let publish_time: i64 = kani::any();
    let now_unix_ts: i64 = kani::any();
    let max_staleness_secs: u64 = kani::any();
    let age = now_unix_ts.saturating_sub(publish_time);

    assert_eq!(
        oracle_publish_time_is_fresh(publish_time, now_unix_ts, max_staleness_secs),
        age >= 0 && age as u64 <= max_staleness_secs
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_oracle_owner_dispatch_is_total_before_parsing() {
    let owner = Pubkey::new_from_array(kani::any());
    let expected_feed: [u8; 32] = kani::any();
    let account_key = Pubkey::new_from_array(expected_feed);
    let is_known = owner == PYTH_RECEIVER_PROGRAM_ID
        || owner == SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID
        || owner == SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID
        || owner == CHAINLINK_STORE_PROGRAM_ID;
    let expected = if is_known {
        Err(ProgramError::InvalidAccountData)
    } else {
        Err(ProgramError::IllegalOwner)
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &owner,
            &account_key,
            &[],
            &expected_feed,
            0,
            u64::MAX,
            u16::MAX,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_key_bound_provider_identity_partition_is_total() {
    let provider: bool = kani::any();
    let owner = if provider {
        SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID
    } else {
        CHAINLINK_STORE_PROGRAM_ID
    };
    let account_key = Pubkey::new_from_array(kani::any());
    let expected_feed: [u8; 32] = kani::any();
    let expected = if account_key.to_bytes() == expected_feed {
        Err(ProgramError::InvalidAccountData)
    } else {
        Err(PercolatorError::InvalidOracleKey.into())
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &owner,
            &account_key,
            &[],
            &expected_feed,
            0,
            u64::MAX,
            u16::MAX,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_known_provider_short_data_rejects_after_identity_binding() {
    let provider = kani::any::<u8>() & 3;
    let expected_feed: [u8; 32] = kani::any();
    let account_key = Pubkey::new_from_array(expected_feed);
    let owner = match provider {
        0 => PYTH_RECEIVER_PROGRAM_ID,
        1 => SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
        2 => SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID,
        _ => CHAINLINK_STORE_PROGRAM_ID,
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &owner,
            &account_key,
            &[],
            &expected_feed,
            0,
            u64::MAX,
            u16::MAX,
        ),
        Err(ProgramError::InvalidAccountData)
    );
}
