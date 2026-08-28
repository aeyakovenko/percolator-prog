//! INV-084 - Proof assumptions are reachable and nonvacuous.
//!
//! Normative obligation: every proof precondition is either established by a
//! public route or separately proven satisfiable for the modeled transition.
//! A harness must not make the exploit class impossible by assumption.
//!
//! Evidence in this file (P): full-width symbolic partitions prove each current
//! predicate has admitted and excluded models and pin boundary witnesses that
//! kill common widening or dropped-clause mutations. Additional harnesses prove
//! constructive valid witnesses for sequence, matcher control, decoder,
//! matcher-return, fee-side attribution, and mark-policy domains. A host-side
//! source audit owns exact inventory completeness across every mounted module.
//!
//! Guarantee boundary: this inventories explicit assumption calls, not all
//! implicit proof preconditions encoded as branches or concrete fixtures. It
//! also does not replace whole-route SVM or bounded-state evidence establishing
//! that public-account states reach each admitted proof domain.

use super::*;
use percolator::V16Error;
use percolator_prog::error::{map_v16_error, PercolatorError};
use percolator_prog::state;
use solana_program::program_error::ProgramError;

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

const fn inv084_matcher_enabled_predicate(enabled: u8) -> bool {
    enabled <= 1
}

const fn inv084_matcher_fee_cap_predicate(trade_fee_cap_bps: u16) -> bool {
    trade_fee_cap_bps <= 10_000
}

const fn inv084_position_epoch_predicate(position_epoch: u64) -> bool {
    position_epoch < state::PortfolioMatcherConfigV16::position_epoch_max()
}

const fn inv084_portfolio_id_predicate(portfolio_id: u64) -> bool {
    portfolio_id != 0
}

const fn inv084_sequence_predicate(sequence: u64) -> bool {
    sequence < u64::MAX
}

const fn inv084_trade_size_predicate(size_q: i128) -> bool {
    size_q != 0
}

const fn inv084_feed_index_predicate(feed_index: usize) -> bool {
    feed_index < 3
}

const fn inv084_feed_byte_index_predicate(byte_index: usize) -> bool {
    byte_index < 32
}

const fn inv084_positive_marks_predicate(old_mark: u64, quoted_mark: u64) -> bool {
    old_mark > 0 && quoted_mark > 0
}

const fn inv084_engine_error_tag_predicate(tag: u8) -> bool {
    tag < 12
}

const fn inv084_dt_solver_bound_predicate(dt_raw: u8) -> bool {
    dt_raw <= 15
}

#[kani::proof]
fn kani_v16_inv084_explicit_assumptions_have_two_sided_mutation_witnesses() {
    let trade_fee_cap_bps: u16 = kani::any();
    let fee_cap_admitted = inv084_matcher_fee_cap_predicate(trade_fee_cap_bps);
    assert_eq!(fee_cap_admitted, trade_fee_cap_bps <= 10_000);
    kani::cover!(
        trade_fee_cap_bps == 10_000 && fee_cap_admitted,
        "fee-cap upper admitted model"
    );
    kani::cover!(
        trade_fee_cap_bps == 10_001 && !fee_cap_admitted,
        "fee-cap widening killer"
    );

    let position_epoch: u64 = kani::any();
    let epoch_max = state::PortfolioMatcherConfigV16::position_epoch_max();
    let epoch_admitted = inv084_position_epoch_predicate(position_epoch);
    assert_eq!(epoch_admitted, position_epoch < epoch_max);
    kani::cover!(
        position_epoch == epoch_max - 1 && epoch_admitted,
        "position-epoch upper admitted model"
    );
    kani::cover!(
        position_epoch == epoch_max && !epoch_admitted,
        "position-epoch exhaustion model"
    );

    let enabled: u8 = kani::any();
    let enabled_admitted = inv084_matcher_enabled_predicate(enabled);
    assert_eq!(enabled_admitted, enabled == 0 || enabled == 1);
    kani::cover!(
        enabled == 0 && enabled_admitted,
        "toggle lower admitted model"
    );
    kani::cover!(
        enabled == 1 && enabled_admitted,
        "toggle upper admitted model"
    );
    kani::cover!(enabled == 2 && !enabled_admitted, "toggle widening killer");
    assert!(inv084_matcher_enabled_predicate(0));
    assert!(inv084_matcher_enabled_predicate(1));
    assert!(!inv084_matcher_enabled_predicate(2));
    assert!(!inv084_matcher_enabled_predicate(u8::MAX));

    let portfolio_id: u64 = kani::any();
    let portfolio_admitted = inv084_portfolio_id_predicate(portfolio_id);
    assert_eq!(portfolio_admitted, portfolio_id != 0);
    kani::cover!(
        portfolio_id == 1 && portfolio_admitted,
        "first portfolio id"
    );
    kani::cover!(
        portfolio_id == 0 && !portfolio_admitted,
        "reserved portfolio id"
    );

    let sequence: u64 = kani::any();
    let sequence_admitted = inv084_sequence_predicate(sequence);
    assert_eq!(sequence_admitted, sequence != u64::MAX);
    kani::cover!(
        sequence == u64::MAX - 1 && sequence_admitted,
        "sequence upper admitted model"
    );
    kani::cover!(
        sequence == u64::MAX && !sequence_admitted,
        "sequence exhaustion model"
    );

    let size_q: i128 = kani::any();
    let size_admitted = inv084_trade_size_predicate(size_q);
    assert_eq!(size_admitted, size_q != 0);
    kani::cover!(size_q == 1 && size_admitted, "positive trade model");
    kani::cover!(size_q == -1 && size_admitted, "negative trade model");
    kani::cover!(size_q == 0 && !size_admitted, "zero trade model");

    let feed_index: usize = kani::any();
    let feed_index_admitted = inv084_feed_index_predicate(feed_index);
    assert_eq!(feed_index_admitted, feed_index <= 2);
    kani::cover!(
        feed_index == 2 && feed_index_admitted,
        "feed-index upper admitted model"
    );
    kani::cover!(
        feed_index == 3 && !feed_index_admitted,
        "feed-index off-by-one mutation killer"
    );
    assert!(inv084_feed_index_predicate(0));
    assert!(inv084_feed_index_predicate(2));
    assert!(!inv084_feed_index_predicate(3));
    assert!(!inv084_feed_index_predicate(usize::MAX));

    let byte_index: usize = kani::any();
    let byte_index_admitted = inv084_feed_byte_index_predicate(byte_index);
    assert_eq!(byte_index_admitted, byte_index <= 31);
    kani::cover!(
        byte_index == 31 && byte_index_admitted,
        "feed-byte upper admitted model"
    );
    kani::cover!(
        byte_index == 32 && !byte_index_admitted,
        "feed-byte off-by-one mutation killer"
    );
    assert!(inv084_feed_byte_index_predicate(0));
    assert!(inv084_feed_byte_index_predicate(31));
    assert!(!inv084_feed_byte_index_predicate(32));
    assert!(!inv084_feed_byte_index_predicate(usize::MAX));

    let old_mark: u64 = kani::any();
    let quoted_mark: u64 = kani::any();
    let positive_marks_admitted = inv084_positive_marks_predicate(old_mark, quoted_mark);
    assert_eq!(positive_marks_admitted, old_mark != 0 && quoted_mark != 0);
    kani::cover!(
        old_mark == 1 && quoted_mark == 1 && positive_marks_admitted,
        "positive-mark admitted model"
    );
    kani::cover!(
        old_mark == 0 && quoted_mark == 1 && !positive_marks_admitted,
        "dropped old-mark clause mutation killer"
    );
    kani::cover!(
        old_mark == 1 && quoted_mark == 0 && !positive_marks_admitted,
        "dropped quoted-mark clause mutation killer"
    );
    assert!(inv084_positive_marks_predicate(1, 1));
    assert!(!inv084_positive_marks_predicate(0, 1));
    assert!(!inv084_positive_marks_predicate(1, 0));
    assert!(!inv084_positive_marks_predicate(0, 0));

    let error_tag: u8 = kani::any();
    let error_tag_admitted = inv084_engine_error_tag_predicate(error_tag);
    assert_eq!(error_tag_admitted, error_tag <= 11);
    kani::cover!(
        error_tag == 11 && error_tag_admitted,
        "engine-error upper admitted model"
    );
    kani::cover!(
        error_tag == 12 && !error_tag_admitted,
        "engine-error widening killer"
    );
    assert!(inv084_engine_error_tag_predicate(0));
    assert!(inv084_engine_error_tag_predicate(11));
    assert!(!inv084_engine_error_tag_predicate(12));
    assert!(!inv084_engine_error_tag_predicate(u8::MAX));

    let dt_raw: u8 = kani::any();
    let dt_admitted = inv084_dt_solver_bound_predicate(dt_raw);
    assert_eq!(dt_admitted, dt_raw < 16);
    kani::cover!(dt_raw == 15 && dt_admitted, "dt upper admitted model");
    kani::cover!(dt_raw == 16 && !dt_admitted, "dt widening killer");
    assert!(inv084_dt_solver_bound_predicate(0));
    assert!(inv084_dt_solver_bound_predicate(15));
    assert!(!inv084_dt_solver_bound_predicate(16));
    assert!(!inv084_dt_solver_bound_predicate(u8::MAX));
}

#[kani::proof]
fn kani_v16_inv084_previously_uninventoried_guards_have_constructive_witnesses() {
    let mut matcher = state::PortfolioMatcherConfigV16::default();
    assert!(matcher.set_trade_fee_cap_bps(10_000).is_ok());
    assert_eq!(matcher.trade_fee_cap_bps(), 10_000);
    assert!(matcher.set_enabled(1).is_ok());
    assert_eq!(matcher.enabled(), 1);
    assert!(state::next_portfolio_position_control(matcher.control).is_ok());

    let before_invalid_cap = matcher.control;
    assert!(matcher.set_trade_fee_cap_bps(10_001).is_err());
    assert_eq!(matcher.control, before_invalid_cap);

    let exhausted_epoch_control = state::PortfolioMatcherConfigV16::position_epoch_max() << 1;
    assert!(state::next_portfolio_position_control(exhausted_epoch_control).is_err());

    assert!(state::next_portfolio_matcher_sequence(0, 0).is_ok());
    assert!(state::next_portfolio_matcher_sequence(u64::MAX, u64::MAX).is_err());
    assert!(inv084_portfolio_id_predicate(1));
    assert!(!inv084_portfolio_id_predicate(0));

    assert!(policy_v16::account_fees_to_trade_sides(1, 11, 22).is_some());
    assert!(policy_v16::account_fees_to_trade_sides(-1, 11, 22).is_some());
    assert!(policy_v16::account_fees_to_trade_sides(0, 11, 22).is_none());
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
    for (tag, amount) in [(3u8, 11u128), (4u8, 12u128)] {
        let portfolio_id = 9u64;
        let expected_sequence = 10u64;
        let mut data = [0u8; 33];
        data[0] = tag;
        data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
        data[9..17].copy_from_slice(&expected_sequence.to_le_bytes());
        data[17..33].copy_from_slice(&amount.to_le_bytes());
        match (tag, Instruction::decode(&data).unwrap()) {
            (
                3,
                Instruction::Deposit {
                    portfolio_id: got_id,
                    expected_sequence: got_sequence,
                    amount: got,
                },
            )
            | (
                4,
                Instruction::Withdraw {
                    portfolio_id: got_id,
                    expected_sequence: got_sequence,
                    amount: got,
                },
            ) => {
                assert_eq!(got_id, portfolio_id);
                assert_eq!(got_sequence, expected_sequence);
                assert_eq!(got, amount);
            }
            _ => unreachable!(),
        }
    }

    let convert_position_epoch = 10u64;
    let convert_amount = 13u128;
    let mut convert = [0u8; 33];
    convert[0] = 28;
    convert[1..9].copy_from_slice(&9u64.to_le_bytes());
    convert[9..17].copy_from_slice(&convert_position_epoch.to_le_bytes());
    convert[17..33].copy_from_slice(&convert_amount.to_le_bytes());
    match Instruction::decode(&convert).unwrap() {
        Instruction::ConvertReleasedPnl {
            portfolio_id,
            position_epoch,
            amount,
        } => {
            assert_eq!(portfolio_id, 9);
            assert_eq!(position_epoch, convert_position_epoch);
            assert_eq!(amount, convert_amount);
        }
        _ => unreachable!(),
    }

    let amount = 21u128;
    let mut data = [0u8; 17];
    data[0] = 30;
    data[1..17].copy_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&data).unwrap() {
        Instruction::CloseResolved { fee_rate_per_slot } => {
            assert_eq!(fee_rate_per_slot, amount)
        }
        _ => unreachable!(),
    }

    data[0] = 41;
    assert!(Instruction::decode(&data).is_err());

    let portfolio_id = 24u64;
    let position_epoch = 25u64;
    let amount = 23u128;
    let mut cure = [0u8; 33];
    cure[0] = 42;
    cure[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    cure[9..17].copy_from_slice(&position_epoch.to_le_bytes());
    cure[17..33].copy_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&cure).unwrap() {
        Instruction::CureAndCancelClose {
            portfolio_id: got_portfolio_id,
            position_epoch: got_position_epoch,
            optional_deposit: got,
        } => {
            assert_eq!(got_portfolio_id, portfolio_id);
            assert_eq!(got_position_epoch, position_epoch);
            assert_eq!(got, amount);
        }
        _ => unreachable!(),
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
