//! INV-024 - attributed quote-value conservation.
//!
//! The wrapper owns SPL custody and authority attribution; the engine owns the
//! canonical 17-class internal value-flow proof. This theorem executes the
//! exact pinned engine validator over arbitrary bounded debit/credit vectors
//! and independently recomputes both obligations visible at the wrapper
//! boundary: every internal debit has one credit, and net external quote flow
//! equals the signed vault delta. No assumptions exclude malformed flows.
//!
//! The bounded `u8` factors are widened to `u128`, so the proof covers every
//! relative attribution partition without an overflow precondition. The
//! public LiteSVM/stateful owners separately bind `ExternalQuote` and
//! `TokenVault` to real SPL balances, owner identities, and all wrapper routes.

use percolator::{
    TokenValueClassV16, TokenValueFlowProofV16, V16Error, V16_TOKEN_VALUE_CLASS_COUNT,
};

fn inv024_sum(values: &[u128; V16_TOKEN_VALUE_CLASS_COUNT]) -> u128 {
    let mut total = 0u128;
    let mut index = 0usize;
    while index < V16_TOKEN_VALUE_CLASS_COUNT {
        total = total
            .checked_add(values[index])
            .expect("bounded attribution factors cannot overflow u128");
        index += 1;
    }
    total
}

#[kani::proof]
fn kani_inv024_engine_flow_validator_equals_wrapper_value_equation() {
    let raw_debits: [u8; V16_TOKEN_VALUE_CLASS_COUNT] = kani::any();
    let raw_credits: [u8; V16_TOKEN_VALUE_CLASS_COUNT] = kani::any();
    let mut debits = [0u128; V16_TOKEN_VALUE_CLASS_COUNT];
    let mut credits = [0u128; V16_TOKEN_VALUE_CLASS_COUNT];
    let mut index = 0usize;
    while index < V16_TOKEN_VALUE_CLASS_COUNT {
        debits[index] = u128::from(raw_debits[index]);
        credits[index] = u128::from(raw_credits[index]);
        index += 1;
    }

    let external_quote_in = u128::from(kani::any::<u8>());
    let external_quote_out = u128::from(kani::any::<u8>());
    let vault_before = u128::from(kani::any::<u16>());
    let vault_after = u128::from(kani::any::<u16>());
    let proof = TokenValueFlowProofV16 {
        debits,
        credits,
        external_quote_in,
        external_quote_out,
        vault_before,
        vault_after,
    };

    let total_debits = inv024_sum(&proof.debits);
    let total_credits = inv024_sum(&proof.credits);
    let external_matches_vault = if vault_after >= vault_before {
        external_quote_in >= external_quote_out
            && external_quote_in - external_quote_out == vault_after - vault_before
    } else {
        external_quote_out >= external_quote_in
            && external_quote_out - external_quote_in == vault_before - vault_after
    };
    let independently_valid = total_debits == total_credits && external_matches_vault;
    assert_eq!(proof.validate().is_ok(), independently_valid);

    kani::cover!(proof.validate().is_ok(), "a balanced flow is admitted");
    kani::cover!(proof.validate().is_err(), "an unbalanced flow is rejected");

    let mut balanced = TokenValueFlowProofV16::empty(7, 7);
    balanced.debits[TokenValueClassV16::AccountCapital as usize] = 1;
    balanced.credits[TokenValueClassV16::InsuranceCapital as usize] = 1;
    assert_eq!(balanced.validate(), Ok(()));

    let mut duplicated = balanced;
    duplicated.credits[TokenValueClassV16::ProtocolFeePaid as usize] = 1;
    assert_eq!(duplicated.validate(), Err(V16Error::InvalidConfig));

    let mut custody_mismatch = balanced;
    custody_mismatch.vault_after = 8;
    assert_eq!(custody_mismatch.validate(), Err(V16Error::InvalidConfig));
}
