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
use percolator_prog::state;

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
