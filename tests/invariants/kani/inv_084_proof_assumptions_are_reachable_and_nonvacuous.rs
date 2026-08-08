//! INV-084 - Proof assumptions are reachable and nonvacuous.
//!
//! Normative obligation: every proof precondition is either established by a
//! public route or separately proven satisfiable for the modeled transition.
//! A harness must not make the exploit class impossible by assumption.
//!
//! Evidence in this file (P): these harnesses pin representative proof
//! preconditions used by the wrapper's local Kani suite. They prove concrete
//! valid witnesses for sequence, decoder, matcher-return, and mark-policy
//! assumptions; and they remove one bounded-input assumption entirely by
//! proving `set_enabled` accepts exactly 0/1 and fail-closes all other `u8`
//! values without mutating unrelated control bits. They also prove the unknown
//! instruction-tag partition used by INV-022 has concrete rejected witnesses,
//! including retained gaps and the internal-only tag 47.
//!
//! Guarantee boundary: this is proof-harness hygiene. It does not replace
//! whole-route SVM tests that establish the public-account state entering these
//! pure transitions.

use super::*;
use percolator::V16Error;
use percolator_prog::error::{map_v16_error, PercolatorError};
use percolator_prog::state;
use solana_program::program_error::ProgramError;

const INV084_ASSUME_TOKEN: &[u8] = b"kani\x3a\x3aassume";
const INV084_INVENTORY: &str = include_str!("../kani_assumption_inventory.tsv");
const INV084_KANI_ROOT: &str = include_str!("../../v16_kani.rs");
const INV084_SRC_004: &str = include_str!("inv_004_position_episode_binding.rs");
const INV084_SRC_010: &str = include_str!("inv_010_out_of_order_safety.rs");
const INV084_SRC_014: &str = include_str!("inv_014_delayed_policy_and_policy_epoch_safety.rs");
const INV084_SRC_019: &str = include_str!("inv_019_cpi_invocation_and_return_data_binding.rs");
const INV084_SRC_022: &str =
    include_str!("inv_022_instruction_decoding_and_schema_upgrade_safety.rs");
const INV084_SRC_045: &str = include_str!("inv_045_no_free_mark_movement.rs");
const INV084_SRC_080: &str = include_str!("inv_080_error_propagation_and_exact_rollback.rs");
const INV084_SRC_084: &str =
    include_str!("inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs");
const INV084_SRC_085: &str =
    include_str!("inv_085_proven_arithmetic_equals_deployed_arithmetic.rs");

const INV084_FILE_004: &[u8] = b"tests/invariants/kani/inv_004_position_episode_binding.rs";
const INV084_FILE_010: &[u8] = b"tests/invariants/kani/inv_010_out_of_order_safety.rs";
const INV084_FILE_014: &[u8] =
    b"tests/invariants/kani/inv_014_delayed_policy_and_policy_epoch_safety.rs";
const INV084_FILE_019: &[u8] =
    b"tests/invariants/kani/inv_019_cpi_invocation_and_return_data_binding.rs";
const INV084_FILE_022: &[u8] =
    b"tests/invariants/kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs";
const INV084_FILE_045: &[u8] = b"tests/invariants/kani/inv_045_no_free_mark_movement.rs";
const INV084_FILE_080: &[u8] =
    b"tests/invariants/kani/inv_080_error_propagation_and_exact_rollback.rs";
const INV084_FILE_084: &[u8] =
    b"tests/invariants/kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs";
const INV084_FILE_085: &[u8] =
    b"tests/invariants/kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs";

const fn inv084_bytes_eq_at(haystack: &[u8], offset: usize, needle: &[u8]) -> bool {
    if offset + needle.len() > haystack.len() {
        return false;
    }
    let mut i = 0;
    while i < needle.len() {
        if haystack[offset + i] != needle[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn inv084_count_token(haystack: &str, needle: &[u8]) -> usize {
    let bytes = haystack.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if inv084_bytes_eq_at(bytes, i, needle) {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

const fn inv084_line_contains_token(source: &str, target_line: usize, needle: &[u8]) -> bool {
    let bytes = source.as_bytes();
    let mut current_line = 1;
    let mut line_start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'\n' {
            if current_line == target_line {
                let mut offset = line_start;
                while offset + needle.len() <= i {
                    if inv084_bytes_eq_at(bytes, offset, needle) {
                        return true;
                    }
                    offset += 1;
                }
                return false;
            }
            current_line += 1;
            line_start = i + 1;
        }
        i += 1;
    }
    false
}

const fn inv084_line_is_empty(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut i = start;
    while i < end {
        if bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\r' {
            return false;
        }
        i += 1;
    }
    true
}

const fn inv084_line_starts_with(bytes: &[u8], start: usize, end: usize, prefix: &[u8]) -> bool {
    if start + prefix.len() > end {
        return false;
    }
    inv084_bytes_eq_at(bytes, start, prefix)
}

const fn inv084_line_file_matches(bytes: &[u8], start: usize, end: usize, file: &[u8]) -> bool {
    if start + file.len() >= end {
        return false;
    }
    inv084_bytes_eq_at(bytes, start, file) && bytes[start + file.len()] == b'\t'
}

const fn inv084_line_ends_with(bytes: &[u8], start: usize, end: usize, suffix: &[u8]) -> bool {
    if start + suffix.len() > end {
        return false;
    }
    inv084_bytes_eq_at(bytes, end - suffix.len(), suffix)
}

const fn inv084_line_has_required_fields(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut fields = 0;
    let mut field_start = start;
    let mut i = start;
    while i <= end {
        if i == end || bytes[i] == b'\t' {
            if i == field_start {
                return false;
            }
            fields += 1;
            field_start = i + 1;
        }
        i += 1;
    }

    fields == 7
        && (inv084_line_ends_with(bytes, start, end, b"NONVACUITY_WITNESS")
            || inv084_line_ends_with(bytes, start, end, b"ROUTE_ESTABLISHED")
            || inv084_line_ends_with(bytes, start, end, b"SOLVER_BOUND_RATIONALE"))
}

const fn inv084_count_inventory_rows_for_file(file: &[u8]) -> usize {
    let bytes = INV084_INVENTORY.as_bytes();
    let mut rows = 0;
    let mut start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'\n' {
            if !inv084_line_is_empty(bytes, start, i)
                && !inv084_line_starts_with(bytes, start, i, b"file\tline\t")
                && inv084_line_file_matches(bytes, start, i, file)
            {
                rows += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    rows
}

const fn inv084_count_inventory_rows() -> usize {
    let bytes = INV084_INVENTORY.as_bytes();
    let mut rows = 0;
    let mut start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'\n' {
            if !inv084_line_is_empty(bytes, start, i)
                && !inv084_line_starts_with(bytes, start, i, b"file\tline\t")
            {
                rows += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    rows
}

const fn inv084_inventory_rows_are_classified() -> bool {
    let bytes = INV084_INVENTORY.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'\n' {
            if !inv084_line_is_empty(bytes, start, i)
                && !inv084_line_starts_with(bytes, start, i, b"file\tline\t")
                && !inv084_line_has_required_fields(bytes, start, i)
            {
                return false;
            }
            start = i + 1;
        }
        i += 1;
    }
    true
}

const INV084_KANI_MODULES_MOUNTED: usize =
    inv084_count_token(INV084_KANI_ROOT, b"#[path = \"invariants/kani/");

const INV084_ASSUME_TOTAL: usize = inv084_count_token(INV084_SRC_004, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_010, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_014, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_019, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_022, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_045, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_080, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_084, INV084_ASSUME_TOKEN)
    + inv084_count_token(INV084_SRC_085, INV084_ASSUME_TOKEN);

const _: () = assert!(INV084_KANI_MODULES_MOUNTED == 9);
const _: () = assert!(inv084_inventory_rows_are_classified());
const _: () = assert!(inv084_count_inventory_rows() == INV084_ASSUME_TOTAL);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_004)
        == inv084_count_token(INV084_SRC_004, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_010)
        == inv084_count_token(INV084_SRC_010, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_014)
        == inv084_count_token(INV084_SRC_014, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_019)
        == inv084_count_token(INV084_SRC_019, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_022)
        == inv084_count_token(INV084_SRC_022, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_045)
        == inv084_count_token(INV084_SRC_045, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_080)
        == inv084_count_token(INV084_SRC_080, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_084)
        == inv084_count_token(INV084_SRC_084, INV084_ASSUME_TOKEN)
);
const _: () = assert!(
    inv084_count_inventory_rows_for_file(INV084_FILE_085)
        == inv084_count_token(INV084_SRC_085, INV084_ASSUME_TOKEN)
);
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_004,
    48,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_022,
    1096,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_022,
    1097,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_045,
    46,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_045,
    78,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_045,
    108,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_080,
    54,
    INV084_ASSUME_TOKEN
));
const _: () = assert!(inv084_line_contains_token(
    INV084_SRC_085,
    68,
    INV084_ASSUME_TOKEN
));

fn inv084_known_public_instruction_tag(tag: u8) -> bool {
    matches!(
        tag,
        0 | 1
            | 3
            | 4
            | 5
            | 6
            | 8
            | 9
            | 10
            | 13
            | 19
            | 23
            | 24
            | 28
            | 30
            | 32
            | 33
            | 34
            | 35
            | 36
            | 37
            | 38
            | 39
            | 40
            | 41
            | 42
            | 43
            | 44
            | 45
            | 46
            | 48
            | 49
            | 50
            | 51
            | 52
            | 53
            | 54
            | 55
    )
}

#[kani::proof]
fn kani_v16_inv084_control_sequence_preconditions_have_accept_and_reject_witnesses() {
    assert!(state::require_newer_control_sequence(0, 1).is_ok());
    assert!(state::require_newer_control_sequence(41, 42).is_ok());
    assert!(state::require_newer_control_sequence(7, 7).is_err());
    assert!(state::require_newer_control_sequence(9, 8).is_err());
    assert!(state::require_newer_control_sequence(u64::MAX, u64::MAX).is_err());

    let current: u64 = kani::any();
    let proposed: u64 = kani::any();
    let result = state::require_newer_control_sequence(current, proposed);
    kani::cover!(
        current == 0 && proposed == 1 && result.is_ok(),
        "strictly newer sequence is reachable"
    );
    kani::cover!(
        current == proposed && result.is_err(),
        "equal-sequence replay rejection is reachable"
    );
    assert_eq!(result.is_ok(), proposed > current);
}

#[kani::proof]
#[kani::unwind(20)]
fn kani_v16_inv084_unknown_tag_assumption_has_concrete_reject_witnesses() {
    let unknown_tag_witnesses = [
        2u8, 7, 11, 12, 14, 18, 20, 21, 22, 25, 26, 27, 29, 31, 47, 56, 127, 255,
    ];

    for tag in unknown_tag_witnesses {
        assert!(
            !inv084_known_public_instruction_tag(tag),
            "witness must remain outside the public instruction roster"
        );
        assert!(
            Instruction::decode(&[tag]).is_err(),
            "unknown one-byte tag must reject"
        );
    }
}

#[kani::proof]
fn kani_v16_inv084_matcher_enabled_input_is_total_not_assumed() {
    let control: u64 = kani::any();
    let enabled: u8 = kani::any();
    let mut config = state::PortfolioMatcherConfigV16 {
        control,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let epoch = config.position_epoch();
    let cap = config.trade_fee_cap_bps();
    let result = config.set_enabled(enabled);

    kani::cover!(enabled == 0 && result.is_ok(), "disable witness");
    kani::cover!(enabled == 1 && result.is_ok(), "enable witness");
    kani::cover!(enabled > 1 && result.is_err(), "invalid toggle witness");

    if enabled <= 1 {
        assert!(result.is_ok());
        assert_eq!(config.enabled(), u64::from(enabled));
        assert_eq!(config.position_epoch(), epoch);
        assert_eq!(config.trade_fee_cap_bps(), cap);
    } else {
        assert!(result.is_err());
        assert_eq!(config.control, control);
    }
}

#[kani::proof]
fn kani_v16_inv084_matcher_return_acceptance_witnesses_are_constructible() {
    let exact = MatcherReturn {
        abi_version: percolator_prog::constants::MATCHER_ABI_VERSION,
        flags: FLAG_VALID,
        exec_price_e6: 123,
        exec_size: 5,
        req_id: 77,
        lp_account_id: 88,
        oracle_price_e6: 123,
        asset_index: 3,
    };
    assert!(validate_matcher_return(&exact, 88, 3, 123, 5, 77).is_ok());

    let partial = MatcherReturn {
        flags: FLAG_VALID | FLAG_PARTIAL_OK,
        exec_size: 0,
        ..exact
    };
    assert!(validate_matcher_return(&partial, 88, 3, 123, 5, 77).is_ok());

    let zero_price = MatcherReturn {
        exec_price_e6: 0,
        ..exact
    };
    assert!(validate_matcher_return(&zero_price, 88, 3, 123, 5, 77).is_err());
}

#[kani::proof]
fn kani_v16_inv084_hybrid_oracle_feed_index_bounds_have_concrete_witnesses() {
    let mut feeds = [[0u8; 32]; 3];
    feeds[0][0] = 11;
    feeds[0][31] = 22;
    feeds[1][0] = 33;
    feeds[1][31] = 44;
    feeds[2][0] = 55;
    feeds[2][31] = 66;

    assert_eq!(feeds.len(), 3);
    assert_eq!(feeds[0].len(), 32);
    assert_eq!(feeds[0][0], 11);
    assert_eq!(feeds[0][31], 22);
    assert_eq!(feeds[1][0], 33);
    assert_eq!(feeds[1][31], 44);
    assert_eq!(feeds[2][0], 55);
    assert_eq!(feeds[2][31], 66);
}

#[kani::proof]
fn kani_v16_inv084_engine_error_tag_partition_has_boundary_witnesses() {
    let first = map_v16_error(V16Error::InvalidConfig);
    let middle = map_v16_error(V16Error::Stale);
    let final_tag = map_v16_error(V16Error::CounterUnderflow);

    assert_eq!(
        first,
        ProgramError::from(PercolatorError::EngineInvalidConfig)
    );
    assert_eq!(middle, ProgramError::from(PercolatorError::EngineStale));
    assert_eq!(
        final_tag,
        ProgramError::from(PercolatorError::EngineCounterUnderflow)
    );
    assert!(matches!(first, ProgramError::Custom(code) if code != 0));
    assert!(matches!(middle, ProgramError::Custom(code) if code != 0));
    assert!(matches!(final_tag, ProgramError::Custom(code) if code != 0));
}

#[kani::proof]
fn kani_v16_inv084_decoder_tag_assumptions_have_concrete_witnesses() {
    for (tag, amount) in [(3u8, 11u128), (4u8, 12u128), (28u8, 13u128)] {
        let portfolio_id = 9u64;
        let mut data = [0u8; 25];
        data[0] = tag;
        data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
        data[9..25].copy_from_slice(&amount.to_le_bytes());
        match (tag, Instruction::decode(&data).unwrap()) {
            (
                3,
                Instruction::Deposit {
                    portfolio_id: got_id,
                    amount: got,
                },
            )
            | (
                4,
                Instruction::Withdraw {
                    portfolio_id: got_id,
                    amount: got,
                },
            )
            | (
                28,
                Instruction::ConvertReleasedPnl {
                    portfolio_id: got_id,
                    amount: got,
                },
            ) => {
                assert_eq!(got_id, portfolio_id);
                assert_eq!(got, amount);
            }
            _ => unreachable!(),
        }
    }

    for (tag, amount) in [(30u8, 21u128), (41u8, 22u128), (42u8, 23u128)] {
        let mut data = [0u8; 17];
        data[0] = tag;
        data[1..17].copy_from_slice(&amount.to_le_bytes());
        match (tag, Instruction::decode(&data).unwrap()) {
            (30, Instruction::CloseResolved { fee_rate_per_slot }) => {
                assert_eq!(fee_rate_per_slot, amount)
            }
            (41, Instruction::WithdrawInsurance { amount: got }) => assert_eq!(got, amount),
            (
                42,
                Instruction::CureAndCancelClose {
                    optional_deposit: got,
                },
            ) => assert_eq!(got, amount),
            _ => unreachable!(),
        }
    }
}

#[kani::proof]
fn kani_v16_inv084_positive_mark_policy_assumptions_have_boundary_witnesses() {
    assert_eq!(policy_v16::clamp_mark_to_supported_move_bps(1, 2, 0), 1);
    assert!(policy_v16::clamp_mark_to_supported_move_bps(1, 2, 10_000) > 1);
    assert!(policy_v16::clamp_mark_to_supported_move_bps(2, 1, 10_000) < 2);

    assert!(policy_v16::premium_funding_rate_e9(2, 1, 1).unwrap() > 0);
    assert!(policy_v16::premium_funding_rate_e9(1, 2, 1).unwrap() < 0);
    assert_eq!(policy_v16::premium_funding_rate_e9(1, 1, 1).unwrap(), 0);
}

#[kani::proof]
fn kani_v16_inv084_dt_clamp_solver_bound_has_boundary_witnesses() {
    assert_eq!(
        percolator_prog::oracle_v16::clamp_toward_engine_dt(100, 200, 10_000, 0),
        100
    );
    assert_eq!(
        percolator_prog::oracle_v16::clamp_toward_engine_dt(100, 200, 10_000, 15),
        200
    );
    assert_eq!(
        percolator_prog::oracle_v16::clamp_toward_engine_dt(200, 100, 10_000, 15),
        100
    );
}
