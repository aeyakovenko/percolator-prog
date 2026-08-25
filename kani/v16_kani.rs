#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::{CrankObservationHint, Instruction};
use percolator_prog::matcher_abi::{
    validate_atomic_batch_matcher_return, validate_matcher_return, MatcherReturn,
    FLAG_BACKING_FEE_CAP_MASK, FLAG_BACKING_FEE_CAP_SHIFT, FLAG_PARTIAL_OK, FLAG_REJECTED,
    FLAG_VALID,
};
use percolator_prog::policy_v16;

#[path = "../tests/invariants/kani/inv_002_asset_generation_binding.rs"]
mod inv_002_asset_generation_binding;

#[path = "../tests/invariants/kani/inv_003_portfolio_incarnation_binding.rs"]
mod inv_003_portfolio_incarnation_binding;

#[path = "../tests/invariants/kani/inv_004_position_episode_binding.rs"]
mod inv_004_position_episode_binding;

#[path = "../tests/invariants/kani/inv_008_intent_uniqueness_and_bounded_replay.rs"]
mod inv_008_intent_uniqueness_and_bounded_replay;

#[path = "../tests/invariants/kani/inv_009_partial_fill_and_retry_accounting.rs"]
mod inv_009_partial_fill_and_retry_accounting;

#[path = "../tests/invariants/kani/inv_010_out_of_order_safety.rs"]
mod inv_010_out_of_order_safety;

#[path = "../tests/invariants/kani/inv_013_destructive_consent_scope.rs"]
mod inv_013_destructive_consent_scope;

#[path = "../tests/invariants/kani/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "../tests/invariants/kani/inv_015_account_ownership_layout_discriminator_and_length_validity.rs"]
mod inv_015_account_ownership_layout_discriminator_and_length_validity;

fn assert_rejects_trailing_byte(ix: Instruction, extra: u8) {
    let mut data = ix.encode();
    data.push(extra);
    assert!(Instruction::decode(&data).is_err());
}

#[path = "../tests/invariants/kani/inv_019_cpi_invocation_and_return_data_binding.rs"]
mod inv_019_cpi_invocation_and_return_data_binding;

#[path = "../tests/invariants/kani/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "../tests/invariants/kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs"]
mod inv_022_instruction_decoding_and_schema_upgrade_safety;

#[path = "../tests/invariants/kani/inv_036_fee_destination_and_policy_version_integrity.rs"]
mod inv_036_fee_destination_and_policy_version_integrity;

#[path = "../tests/invariants/kani/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;

#[path = "../tests/invariants/kani/inv_052_split_merge_invariance.rs"]
mod inv_052_split_merge_invariance;

#[path = "../tests/invariants/kani/inv_063_backing_expiry_normalization.rs"]
mod inv_063_backing_expiry_normalization;

#[path = "../tests/invariants/kani/inv_074_scope_locality.rs"]
mod inv_074_scope_locality;

#[path = "../tests/invariants/kani/inv_080_error_propagation_and_exact_rollback.rs"]
mod inv_080_error_propagation_and_exact_rollback;

#[path = "../tests/invariants/kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs"]
mod inv_084_proof_assumptions_are_reachable_and_nonvacuous;

#[path = "../tests/invariants/kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs"]
mod inv_085_proven_arithmetic_equals_deployed_arithmetic;

fn main() {}
