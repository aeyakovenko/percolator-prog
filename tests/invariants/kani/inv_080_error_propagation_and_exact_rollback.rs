//! INV-080 - Error propagation and exact rollback.
//!
//! Normative obligation: every non-success engine result must become an
//! instruction error so the SVM rollback guarantee can discard all partial
//! persistent effects.
//!
//! Evidence in this file (P): Kani checks the deployed wrapper error mapping
//! for every current `V16Error` variant. This is the wrapper-side proof
//! obligation; transaction rollback itself is an SVM semantic assumption and is
//! covered by LiteSVM route tests in the CU invariant file.

use percolator::V16Error;
use percolator_prog::error::{map_v16_error, PercolatorError};
use solana_program::program_error::ProgramError;

fn inv080_engine_error_from_tag(tag: u8) -> V16Error {
    match tag {
        0 => V16Error::InvalidConfig,
        1 => V16Error::ArithmeticOverflow,
        2 => V16Error::ProvenanceMismatch,
        3 => V16Error::HiddenLeg,
        4 => V16Error::InvalidLeg,
        5 => V16Error::Stale,
        6 => V16Error::BStale,
        7 => V16Error::LockActive,
        8 => V16Error::NonProgress,
        9 => V16Error::RecoveryRequired,
        10 => V16Error::CounterOverflow,
        _ => V16Error::CounterUnderflow,
    }
}

fn inv080_expected_program_error(tag: u8) -> ProgramError {
    let expected = match tag {
        0 => PercolatorError::EngineInvalidConfig,
        1 => PercolatorError::EngineArithmeticOverflow,
        2 => PercolatorError::EngineProvenanceMismatch,
        3 => PercolatorError::EngineHiddenLeg,
        4 => PercolatorError::EngineInvalidLeg,
        5 => PercolatorError::EngineStale,
        6 => PercolatorError::EngineBStale,
        7 => PercolatorError::EngineLockActive,
        8 => PercolatorError::EngineNonProgress,
        9 => PercolatorError::EngineRecoveryRequired,
        10 => PercolatorError::EngineCounterOverflow,
        _ => PercolatorError::EngineCounterUnderflow,
    };
    expected.into()
}

#[kani::proof]
fn kani_v16_inv080_every_engine_error_maps_to_instruction_error() {
    let tag: u8 = kani::any();
    kani::assume(tag < 12);

    let engine_err = inv080_engine_error_from_tag(tag);
    let mapped = map_v16_error(engine_err);
    assert_eq!(mapped, inv080_expected_program_error(tag));
    assert!(matches!(mapped, ProgramError::Custom(code) if code != 0));

    let engine_result: Result<(), V16Error> = Err(inv080_engine_error_from_tag(tag));
    let instruction_result: Result<(), ProgramError> = engine_result.map_err(map_v16_error);
    assert!(instruction_result.is_err());

    kani::cover!(tag == 0, "InvalidConfig maps to an instruction error");
    kani::cover!(tag == 5, "Stale maps to an instruction error");
    kani::cover!(tag == 8, "NonProgress maps to an instruction error");
    kani::cover!(tag == 11, "CounterUnderflow maps to an instruction error");
}
