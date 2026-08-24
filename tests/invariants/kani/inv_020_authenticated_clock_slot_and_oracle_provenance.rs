//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: every selected leg in one composite price shares one authenticated
//! observation epoch. The deployed wrapper predicate must accept one-leg prices and reject every
//! two- or three-leg timestamp disagreement.
//!
//! Evidence in this file (P): Kani exhausts all full-width `i64` timestamp triples through the
//! exact production predicate and proves full-width confidence comparison totality plus zero-side
//! semantics. Canonical Pyth and Chainlink byte layouts compose symbolic price, freshness,
//! identity, and structural fields through the shipping parser. Switchboard's selected timestamp
//! table and typed validation seam are proven separately, with concrete scale boundaries for all
//! three providers. Independent host arithmetic covers the solver-bound relational product and
//! division semantics; public SBF tests bind these predicates to account parsing, rollback/ignore
//! semantics, terminal payout, and owner exit.

use percolator_prog::{
    error::PercolatorError,
    oracle_v16::{
        oracle_confidence_is_too_wide, oracle_publish_time_is_fresh,
        oracle_publish_times_are_coherent, read_oracle_price_e6_from_bytes,
        read_switchboard_selected_publish_time, validate_switchboard_observation_e6,
        SwitchboardObservationV16, CHAINLINK_STORE_PROGRAM_ID, PYTH_RECEIVER_PROGRAM_ID,
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
    let age = i128::from(now_unix_ts) - i128::from(publish_time);

    assert_eq!(
        oracle_publish_time_is_fresh(publish_time, now_unix_ts, max_staleness_secs),
        age >= 0 && age as u128 <= u128::from(max_staleness_secs)
    );
}

#[kani::proof]
fn kani_v16_oracle_confidence_is_total_at_full_width() {
    let uncertainty: u128 = kani::any();
    let value: u128 = kani::any();
    let conf_bps: u16 = kani::any();
    let is_too_wide = oracle_confidence_is_too_wide(uncertainty, value, conf_bps);

    assert!(!is_too_wide || (conf_bps != 0 && uncertainty != 0));
    assert!(value != 0 || uncertainty == 0 || conf_bps == 0 || is_too_wide);
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

fn canonical_pyth_bytes(
    feed_id: [u8; 32],
    price: i64,
    exponent: i32,
    confidence: u64,
    publish_time: i64,
) -> [u8; 134] {
    let mut data = [0u8; 134];
    data[..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    data[40] = 1;
    data[41..73].copy_from_slice(&feed_id);
    data[73..81].copy_from_slice(&price.to_le_bytes());
    data[81..89].copy_from_slice(&confidence.to_le_bytes());
    data[89..93].copy_from_slice(&exponent.to_le_bytes());
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
    data
}

fn canonical_chainlink_bytes(decimals: u8, publish_time: u32, answer: i128) -> [u8; 248] {
    let mut data = [0u8; 248];
    data[..8].copy_from_slice(&[96, 179, 69, 66, 128, 129, 73, 117]);
    data[8] = 1;
    data[138] = decimals;
    data[143..147].copy_from_slice(&1u32.to_le_bytes());
    data[148..152].copy_from_slice(&1u32.to_le_bytes());
    data[200..208].copy_from_slice(&1u64.to_le_bytes());
    data[208..212].copy_from_slice(&publish_time.to_le_bytes());
    data[216..232].copy_from_slice(&answer.to_le_bytes());
    data
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_pyth_canonical_bytes_compose_price_and_full_width_freshness() {
    let price: i64 = kani::any();
    let publish_time: i64 = kani::any();
    let now_unix_ts: i64 = kani::any();
    let max_staleness_secs: u64 = kani::any();
    let feed_id = [0x41; 32];
    let data = canonical_pyth_bytes(feed_id, price, -6, 0, publish_time);
    let expected = if price <= 0 {
        Err(PercolatorError::OracleInvalid.into())
    } else if !oracle_publish_time_is_fresh(publish_time, now_unix_ts, max_staleness_secs) {
        Err(PercolatorError::OracleStale.into())
    } else if price as u128 > percolator::MAX_ORACLE_PRICE as u128 {
        Err(PercolatorError::OracleInvalid.into())
    } else {
        Ok((price as u64, publish_time))
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &PYTH_RECEIVER_PROGRAM_ID,
            &Pubkey::new_from_array([0x42; 32]),
            &data,
            &feed_id,
            now_unix_ts,
            max_staleness_secs,
            0,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_pyth_canonical_bytes_bind_feed_and_verification_fields() {
    let actual_feed: [u8; 32] = kani::any();
    let expected_feed: [u8; 32] = kani::any();
    let verification: u8 = kani::any();
    let mut data = canonical_pyth_bytes(actual_feed, 1_000_000, -6, 0, 1);
    data[40] = verification;
    let expected = if verification != 1 {
        Err(PercolatorError::OracleInvalid.into())
    } else if actual_feed != expected_feed {
        Err(PercolatorError::InvalidOracleKey.into())
    } else {
        Ok((1_000_000, 1))
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &PYTH_RECEIVER_PROGRAM_ID,
            &Pubkey::new_from_array([0x45; 32]),
            &data,
            &expected_feed,
            1,
            0,
            0,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_pyth_confidence_routing_has_no_zero_cost_rejection() {
    let confidence: u64 = kani::any();
    let conf_bps: u16 = kani::any();
    let feed_id = [0x46; 32];
    let data = canonical_pyth_bytes(feed_id, 1_000_000, -6, confidence, 1);
    let result = read_oracle_price_e6_from_bytes(
        &PYTH_RECEIVER_PROGRAM_ID,
        &Pubkey::new_from_array([0x47; 32]),
        &data,
        &feed_id,
        1,
        0,
        conf_bps,
    );

    if result == Err(PercolatorError::OracleConfTooWide.into()) {
        assert!(conf_bps != 0 && confidence != 0);
    }
    if conf_bps == 0 || confidence == 0 {
        assert_eq!(result, Ok((1_000_000, 1)));
    }
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_pyth_full_width_invalid_exponents_reject_before_scale() {
    let exponent: i32 = kani::any();
    kani::cover!(exponent < -18);
    kani::cover!(exponent > 18);
    let feed_id = [0x48; 32];
    let data = canonical_pyth_bytes(feed_id, 1, exponent, 0, 1);

    if !(-18..=18).contains(&exponent) {
        assert_eq!(
            read_oracle_price_e6_from_bytes(
                &PYTH_RECEIVER_PROGRAM_ID,
                &Pubkey::new_from_array([0x49; 32]),
                &data,
                &feed_id,
                1,
                0,
                0,
            ),
            Err(PercolatorError::OracleInvalid.into())
        );
    }
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_pyth_scale_boundaries_match_deployed_outputs() {
    let feed_id = [0x4a; 32];
    let parse = |price, exponent| {
        let data = canonical_pyth_bytes(feed_id, price, exponent, 0, 1);
        read_oracle_price_e6_from_bytes(
            &PYTH_RECEIVER_PROGRAM_ID,
            &Pubkey::new_from_array([0x4b; 32]),
            &data,
            &feed_id,
            1,
            0,
            0,
        )
    };

    assert_eq!(parse(1, -18), Err(PercolatorError::OracleInvalid.into()));
    assert_eq!(parse(1_000_000_000_000, -18), Ok((1, 1)));
    assert_eq!(parse(10, -7), Ok((1, 1)));
    assert_eq!(parse(1, -6), Ok((1, 1)));
    assert_eq!(parse(1, 0), Ok((1_000_000, 1)));
    assert_eq!(parse(1, 6), Ok((percolator::MAX_ORACLE_PRICE, 1)));
    assert_eq!(parse(1, 7), Err(PercolatorError::OracleInvalid.into()));
    assert_eq!(
        parse(i64::MAX, 18),
        Err(PercolatorError::EngineArithmeticOverflow.into())
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_switchboard_selected_timestamp_table_is_exact_and_bounded() {
    let timestamps: [i64; 32] = kani::any();
    let submission_idx: u8 = kani::any();
    let mut bytes = [0u8; 32 * core::mem::size_of::<i64>()];
    for (index, timestamp) in timestamps.iter().enumerate() {
        let offset = index * core::mem::size_of::<i64>();
        bytes[offset..offset + core::mem::size_of::<i64>()]
            .copy_from_slice(&timestamp.to_le_bytes());
    }
    let expected = if usize::from(submission_idx) < timestamps.len() {
        Ok(timestamps[usize::from(submission_idx)])
    } else {
        Err(PercolatorError::OracleInvalid.into())
    };

    assert_eq!(
        read_switchboard_selected_publish_time(&bytes, submission_idx),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_switchboard_typed_validation_composes_structure_and_freshness() {
    const SCALE: i128 = 1_000_000_000_000;
    let observation = SwitchboardObservationV16 {
        feed_hash: kani::any(),
        min_sample_size: kani::any(),
        account_update_time: kani::any(),
        value: 1_000_000 * SCALE,
        std_dev: 0,
        num_samples: kani::any(),
        result_slot: kani::any(),
        publish_time: kani::any(),
    };
    let now_unix_ts: i64 = kani::any();
    let max_staleness_secs: u64 = kani::any();
    let expected = if observation.feed_hash == [0u8; 32]
        || observation.min_sample_size == 0
        || observation.num_samples < observation.min_sample_size
        || observation.result_slot == 0
        || observation.account_update_time <= 0
    {
        Err(PercolatorError::OracleInvalid.into())
    } else if observation.publish_time <= 0
        || !oracle_publish_time_is_fresh(observation.publish_time, now_unix_ts, max_staleness_secs)
    {
        Err(PercolatorError::OracleStale.into())
    } else {
        Ok((1_000_000, observation.publish_time))
    };

    assert_eq!(
        validate_switchboard_observation_e6(observation, now_unix_ts, max_staleness_secs, 0),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_switchboard_confidence_routing_has_no_zero_cost_rejection() {
    const SCALE: i128 = 1_000_000_000_000;
    let std_dev: i128 = kani::any();
    let conf_bps: u16 = kani::any();
    let observation = SwitchboardObservationV16 {
        feed_hash: [1u8; 32],
        min_sample_size: 1,
        account_update_time: 1,
        value: 1_000_000 * SCALE,
        std_dev,
        num_samples: 1,
        result_slot: 1,
        publish_time: 1,
    };
    let result = validate_switchboard_observation_e6(observation, 1, 0, conf_bps);

    if std_dev < 0 {
        assert_eq!(result, Err(PercolatorError::OracleInvalid.into()));
    } else {
        if result == Err(PercolatorError::OracleConfTooWide.into()) {
            assert!(conf_bps != 0 && std_dev != 0);
        }
        if conf_bps == 0 || std_dev == 0 {
            assert_eq!(result, Ok((1_000_000, 1)));
        }
    }
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_switchboard_nonpositive_values_reject_before_time_and_scale() {
    let value: i128 = kani::any();
    kani::cover!(value == 0);
    kani::cover!(value < 0);
    let observation = SwitchboardObservationV16 {
        feed_hash: [1u8; 32],
        min_sample_size: 1,
        account_update_time: 1,
        value,
        std_dev: 0,
        num_samples: 1,
        result_slot: 1,
        publish_time: kani::any(),
    };

    if value <= 0 {
        assert_eq!(
            validate_switchboard_observation_e6(observation, kani::any(), kani::any(), 0),
            Err(PercolatorError::OracleInvalid.into())
        );
    }
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_switchboard_scale_boundaries_match_deployed_outputs() {
    const SCALE: i128 = 1_000_000_000_000;
    const MAX_PRICE: i128 = percolator::MAX_ORACLE_PRICE as i128;
    let validate = |value| {
        validate_switchboard_observation_e6(
            SwitchboardObservationV16 {
                feed_hash: [1u8; 32],
                min_sample_size: 1,
                account_update_time: 1,
                value,
                std_dev: 0,
                num_samples: 1,
                result_slot: 1,
                publish_time: 1,
            },
            1,
            0,
            0,
        )
    };

    assert_eq!(
        validate(SCALE - 1),
        Err(PercolatorError::OracleInvalid.into())
    );
    assert_eq!(validate(SCALE), Ok((1, 1)));
    assert_eq!(
        validate(MAX_PRICE * SCALE + SCALE - 1),
        Ok((percolator::MAX_ORACLE_PRICE, 1))
    );
    assert_eq!(
        validate((MAX_PRICE + 1) * SCALE),
        Err(PercolatorError::OracleInvalid.into())
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_chainlink_canonical_bytes_compose_price_and_full_width_freshness() {
    let answer: i128 = kani::any();
    let publish_time: u32 = kani::any();
    let now_unix_ts: i64 = kani::any();
    let max_staleness_secs: u64 = kani::any();
    let account_key = Pubkey::new_from_array([0x44; 32]);
    let data = canonical_chainlink_bytes(6, publish_time, answer);
    let publish_time_i64 = i64::from(publish_time);
    let expected = if publish_time == 0 {
        Err(PercolatorError::OracleInvalid.into())
    } else if !oracle_publish_time_is_fresh(publish_time_i64, now_unix_ts, max_staleness_secs) {
        Err(PercolatorError::OracleStale.into())
    } else if answer <= 0 || answer as u128 > percolator::MAX_ORACLE_PRICE as u128 {
        Err(PercolatorError::OracleInvalid.into())
    } else {
        Ok((answer as u64, publish_time_i64))
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &CHAINLINK_STORE_PROGRAM_ID,
            &account_key,
            &data,
            &account_key.to_bytes(),
            now_unix_ts,
            max_staleness_secs,
            0,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_chainlink_canonical_bytes_compose_structural_fields() {
    let version: u8 = kani::any();
    let latest_round_id: u32 = kani::any();
    let live_length: u32 = kani::any();
    let result_slot: u64 = kani::any();
    let account_key = Pubkey::new_from_array([0x4c; 32]);
    let mut data = canonical_chainlink_bytes(6, 1, 1_000_000);
    data[8] = version;
    data[143..147].copy_from_slice(&latest_round_id.to_le_bytes());
    data[148..152].copy_from_slice(&live_length.to_le_bytes());
    data[200..208].copy_from_slice(&result_slot.to_le_bytes());
    let expected = if version == 0 || latest_round_id == 0 || live_length != 1 || result_slot == 0 {
        Err(PercolatorError::OracleInvalid.into())
    } else {
        Ok((1_000_000, 1))
    };

    assert_eq!(
        read_oracle_price_e6_from_bytes(
            &CHAINLINK_STORE_PROGRAM_ID,
            &account_key,
            &data,
            &account_key.to_bytes(),
            1,
            0,
            0,
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_chainlink_full_width_invalid_decimals_reject_before_scale() {
    let decimals: u8 = kani::any();
    kani::cover!(decimals == 19);
    kani::cover!(decimals == u8::MAX);
    let account_key = Pubkey::new_from_array([0x4d; 32]);
    let data = canonical_chainlink_bytes(decimals, 1, 1);

    if decimals > 18 {
        assert_eq!(
            read_oracle_price_e6_from_bytes(
                &CHAINLINK_STORE_PROGRAM_ID,
                &account_key,
                &data,
                &account_key.to_bytes(),
                1,
                0,
                0,
            ),
            Err(PercolatorError::OracleInvalid.into())
        );
    }
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_v16_chainlink_scale_boundaries_match_deployed_outputs() {
    let account_key = Pubkey::new_from_array([0x4e; 32]);
    let parse = |answer, decimals| {
        let data = canonical_chainlink_bytes(decimals, 1, answer);
        read_oracle_price_e6_from_bytes(
            &CHAINLINK_STORE_PROGRAM_ID,
            &account_key,
            &data,
            &account_key.to_bytes(),
            1,
            0,
            0,
        )
    };

    assert_eq!(parse(1, 18), Err(PercolatorError::OracleInvalid.into()));
    assert_eq!(parse(1_000_000_000_000, 18), Ok((1, 1)));
    assert_eq!(parse(10, 7), Ok((1, 1)));
    assert_eq!(parse(1, 6), Ok((1, 1)));
    assert_eq!(parse(1, 0), Ok((1_000_000, 1)));
    assert_eq!(parse(1_000_000, 0), Ok((percolator::MAX_ORACLE_PRICE, 1)));
    assert_eq!(
        parse(1_000_001, 0),
        Err(PercolatorError::OracleInvalid.into())
    );
    assert_eq!(
        parse(i128::MAX, 0),
        Err(PercolatorError::EngineArithmeticOverflow.into())
    );
}
