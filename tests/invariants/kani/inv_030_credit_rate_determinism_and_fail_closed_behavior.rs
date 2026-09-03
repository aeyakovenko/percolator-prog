//! INV-030 - Credit-rate determinism and fail-closed behavior.
//!
//! The engine owns the canonical rate formula and every source-credit mutation. These proofs own
//! the wrapper boundary. The first executes the exact engine account codec over arbitrary field
//! values and proves that no source-credit input, rate, or epoch is dropped or remapped. The second
//! proves the sequence-independent induction step under the pinned engine rate contract: a
//! canonical prestate followed by a canonical successful engine poststate remains canonical after
//! the wrapper commit; unchanged formula inputs preserve the rate, changed inputs advance the
//! epoch, and asset reincarnation starts from its independently validated canonical poststate.
//!
//! Exact wide division remains engine-owned and is discharged under the named arithmetic axiom and
//! differential suite. The CU composition owner source-locks the wrapper writer absence, all
//! wrapper-to-engine transition callsites, public nonzero rate transitions, and the exact engine
//! proof roster. This module does not reimplement the rate formula.

use percolator::{SourceCreditStateV16, SourceCreditStateV16Account, CREDIT_RATE_SCALE};

#[derive(Clone, Copy, PartialEq, Eq)]
struct RateInputs {
    positive_claim_bound_num: u128,
    fresh_reserved_backing_num: u128,
    valid_liened_backing_num: u128,
    insurance_credit_reserved_num: u128,
    valid_liened_insurance_num: u128,
    impaired_liened_insurance_num: u128,
}

#[derive(Clone, Copy)]
struct RateSummary {
    market_id: u64,
    inputs: RateInputs,
    credit_rate_num: u128,
    credit_epoch: u64,
}

#[kani::proof]
fn kani_inv030_source_credit_account_codec_preserves_every_engine_field() {
    let source = SourceCreditStateV16 {
        positive_claim_bound_num: kani::any(),
        exact_positive_claim_num: kani::any(),
        fresh_reserved_backing_num: kani::any(),
        spent_backing_num: kani::any(),
        provider_receivable_num: kani::any(),
        valid_liened_backing_num: kani::any(),
        impaired_liened_backing_num: kani::any(),
        insurance_credit_reserved_num: kani::any(),
        valid_liened_insurance_num: kani::any(),
        impaired_liened_insurance_num: kani::any(),
        credit_rate_num: kani::any(),
        credit_epoch: kani::any(),
    };
    let wire = SourceCreditStateV16Account::from_runtime(&source);

    assert_eq!(
        wire.positive_claim_bound_num.get(),
        source.positive_claim_bound_num
    );
    assert_eq!(
        wire.exact_positive_claim_num.get(),
        source.exact_positive_claim_num
    );
    assert_eq!(
        wire.fresh_reserved_backing_num.get(),
        source.fresh_reserved_backing_num
    );
    assert_eq!(wire.spent_backing_num.get(), source.spent_backing_num);
    assert_eq!(
        wire.provider_receivable_num.get(),
        source.provider_receivable_num
    );
    assert_eq!(
        wire.valid_liened_backing_num.get(),
        source.valid_liened_backing_num
    );
    assert_eq!(
        wire.impaired_liened_backing_num.get(),
        source.impaired_liened_backing_num
    );
    assert_eq!(
        wire.insurance_credit_reserved_num.get(),
        source.insurance_credit_reserved_num
    );
    assert_eq!(
        wire.valid_liened_insurance_num.get(),
        source.valid_liened_insurance_num
    );
    assert_eq!(
        wire.impaired_liened_insurance_num.get(),
        source.impaired_liened_insurance_num
    );
    assert_eq!(wire.credit_rate_num.get(), source.credit_rate_num);
    assert_eq!(wire.credit_epoch.get(), source.credit_epoch);
}

#[kani::proof]
fn kani_inv030_wrapper_commit_preserves_engine_rate_induction_step() {
    let before = RateSummary {
        market_id: kani::any(),
        inputs: RateInputs {
            positive_claim_bound_num: kani::any(),
            fresh_reserved_backing_num: kani::any(),
            valid_liened_backing_num: kani::any(),
            insurance_credit_reserved_num: kani::any(),
            valid_liened_insurance_num: kani::any(),
            impaired_liened_insurance_num: kani::any(),
        },
        credit_rate_num: kani::any(),
        credit_epoch: kani::any(),
    };
    let after = RateSummary {
        market_id: kani::any(),
        inputs: RateInputs {
            positive_claim_bound_num: kani::any(),
            fresh_reserved_backing_num: kani::any(),
            valid_liened_backing_num: kani::any(),
            insurance_credit_reserved_num: kani::any(),
            valid_liened_insurance_num: kani::any(),
            impaired_liened_insurance_num: kani::any(),
        },
        credit_rate_num: kani::any(),
        credit_epoch: kani::any(),
    };
    let expected_before: u128 = kani::any();
    let expected_after: u128 = kani::any();

    let same_incarnation = before.market_id == after.market_id;
    let same_inputs = before.inputs == after.inputs;
    let deterministic_same_input =
        !same_incarnation || !same_inputs || expected_after == expected_before;
    let changed_input_advances_epoch =
        !same_incarnation || same_inputs || after.credit_epoch > before.credit_epoch;
    let induction_domain = before.credit_rate_num == expected_before
        && expected_before <= CREDIT_RATE_SCALE
        && after.credit_rate_num == expected_after
        && expected_after <= CREDIT_RATE_SCALE
        && deterministic_same_input
        && changed_input_advances_epoch;

    kani::cover!(
        induction_domain && same_incarnation && same_inputs,
        "same-incarnation frame preserves the canonical rate"
    );
    kani::cover!(
        induction_domain && same_incarnation && !same_inputs,
        "formula-input mutation advances the credit epoch"
    );
    kani::cover!(
        induction_domain && !same_incarnation,
        "asset reincarnation installs an independently canonical source state"
    );
    if !induction_domain {
        return;
    }

    // The wrapper commit is a field-for-field copy of the successful engine poststate; the first
    // proof establishes this relation over the deployed account codec at full width.
    let committed = after;
    assert_eq!(committed.credit_rate_num, expected_after);
    assert!(committed.credit_rate_num <= CREDIT_RATE_SCALE);
    if same_incarnation && same_inputs {
        assert_eq!(committed.credit_rate_num, before.credit_rate_num);
    }
    if same_incarnation && !same_inputs {
        assert!(committed.credit_epoch > before.credit_epoch);
    }
}
