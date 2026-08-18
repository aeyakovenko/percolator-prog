#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::{CrankObservationHint, Instruction};
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_BACKING_FEE_CAP_MASK, FLAG_BACKING_FEE_CAP_SHIFT,
    FLAG_PARTIAL_OK, FLAG_REJECTED, FLAG_VALID,
};
use percolator_prog::policy_v16;

#[path = "invariants/kani/inv_004_position_episode_binding.rs"]
mod inv_004_position_episode_binding;

#[path = "invariants/kani/inv_010_out_of_order_safety.rs"]
mod inv_010_out_of_order_safety;

#[path = "invariants/kani/inv_013_destructive_consent_scope.rs"]
mod inv_013_destructive_consent_scope;

#[path = "invariants/kani/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

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

#[path = "invariants/kani/inv_080_error_propagation_and_exact_rollback.rs"]
mod inv_080_error_propagation_and_exact_rollback;

#[path = "invariants/kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs"]
mod inv_084_proof_assumptions_are_reachable_and_nonvacuous;

#[path = "invariants/kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs"]
mod inv_085_proven_arithmetic_equals_deployed_arithmetic;
