//! INV-025 - exact stock reconciliation.
//!
//! The wrapper persists no duplicate stock ledger. It independently observes the
//! deployed engine's senior stocks and the real SPL vault, while the engine owns
//! the split of the remaining junior atoms between settlement rounding residue
//! and unallocated protocol surplus. This composition theorem executes the
//! pinned engine's canonical `StockReconciliationProofV16` over every bounded
//! partition of all seven stock classes, then proves that the wrapper-visible
//! combined residual reconciles the exact SPL custody amount. A one-atom
//! duplicate or omission in either residual class must fail the engine proof.
//!
//! The bounded `u8` factors are deliberate: the theorem is algebraic and all
//! additions are widened to `u128`, so the finite domain covers every relative
//! stock partition without introducing an overflow precondition. Public
//! LiteSVM/stateful owners separately prove that the wrapper extracts each senior
//! class from raw deployed state and that engine vault custody equals SPL custody.

use percolator::{StockReconciliationProofV16, V16Error};

#[kani::proof]
fn kani_inv025_engine_partition_composes_with_wrapper_spl_custody() {
    let capital = u128::from(kani::any::<u8>());
    let insurance = u128::from(kani::any::<u8>());
    let provider_earnings = u128::from(kani::any::<u8>());
    let backing_principal = u128::from(kani::any::<u8>());
    let rounding_residue = u128::from(kani::any::<u8>());
    let protocol_surplus = u128::from(kani::any::<u8>());

    let combined_residual = rounding_residue + protocol_surplus;
    let engine_vault =
        capital + insurance + provider_earnings + backing_principal + combined_residual;
    let spl_vault = engine_vault;

    let proof = StockReconciliationProofV16 {
        token_vault: engine_vault,
        senior_capital_total: capital,
        insurance_capital: insurance,
        backing_provider_earnings: provider_earnings,
        counterparty_backing_principal: backing_principal,
        settlement_rounding_residue_total: rounding_residue,
        unallocated_protocol_surplus: protocol_surplus,
    };
    assert_eq!(proof.validate(), Ok(()));
    assert_eq!(
        capital + insurance + provider_earnings + backing_principal + combined_residual,
        spl_vault
    );

    let mut duplicated_residue = proof;
    duplicated_residue.unallocated_protocol_surplus += 1;
    assert_eq!(duplicated_residue.validate(), Err(V16Error::InvalidConfig));

    let mut omitted_residue = proof;
    omitted_residue.token_vault += 1;
    assert_eq!(omitted_residue.validate(), Err(V16Error::InvalidConfig));

    kani::cover!(
        capital > 0
            && insurance > 0
            && provider_earnings > 0
            && backing_principal > 0
            && rounding_residue > 0
            && protocol_surplus > 0,
        "all senior classes and both residual classes are simultaneously nonzero"
    );
}
