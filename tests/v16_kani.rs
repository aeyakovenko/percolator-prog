#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::{CrankObservationHint, Instruction};
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_REJECTED, FLAG_VALID,
};
use percolator_prog::policy_v16;

fn assert_rejects_trailing_byte(ix: Instruction, extra: u8) {
    let mut data = ix.encode();
    data.push(extra);
    assert!(Instruction::decode(&data).is_err());
}

#[path = "invariants/kani/inv_019_cpi_invocation_and_return_data_binding.rs"]
mod inv_019_cpi_invocation_and_return_data_binding;

#[path = "invariants/kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs"]
mod inv_022_instruction_decoding_and_schema_upgrade_safety;

#[path = "invariants/kani/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;
