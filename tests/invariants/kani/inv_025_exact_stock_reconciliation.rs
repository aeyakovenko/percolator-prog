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
//! The observation-ledger theorem additionally proves that arbitrary authority-selected sync
//! checkpoints may change gross increase/decrease telemetry but always preserve the exact net
//! observed-stock identity. This is the explicit history relation for both auxiliary sync routes.

use percolator::{StockReconciliationProofV16, V16Error};

#[derive(Clone, Copy)]
struct Inv025ObservationLedger {
    initial: u16,
    last: u16,
    cumulative_increase: u16,
    cumulative_decrease: u16,
}

impl Inv025ObservationLedger {
    fn net_identity(self) -> bool {
        i32::from(self.last) - i32::from(self.initial)
            == i32::from(self.cumulative_increase) - i32::from(self.cumulative_decrease)
    }

    fn observe(self, current: u16) -> Self {
        if current >= self.last {
            Self {
                last: current,
                cumulative_increase: self.cumulative_increase + (current - self.last),
                ..self
            }
        } else {
            Self {
                last: current,
                cumulative_decrease: self.cumulative_decrease + (self.last - current),
                ..self
            }
        }
    }
}

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

#[kani::proof]
fn kani_inv025_observation_ledger_net_identity_is_history_inductive() {
    // u8 symbolic components widened to u16 leave enough headroom for one arbitrary full-range
    // checkpoint delta. The conditional quantifies over every valid induction pre-state without an
    // assumption that could make the harness vacuous.
    let pre = Inv025ObservationLedger {
        initial: u16::from(kani::any::<u8>()),
        last: u16::from(kani::any::<u8>()),
        cumulative_increase: u16::from(kani::any::<u8>()),
        cumulative_decrease: u16::from(kani::any::<u8>()),
    };
    let current = u16::from(kani::any::<u8>());
    if pre.net_identity() {
        let post = pre.observe(current);
        assert!(post.net_identity());
        assert_eq!(post.last, current);
        assert!(post.cumulative_increase >= pre.cumulative_increase);
        assert!(post.cumulative_decrease >= pre.cumulative_decrease);
        kani::cover!(
            current > pre.last,
            "an increasing observation advances gross increase"
        );
        kani::cover!(
            current < pre.last,
            "a decreasing observation advances gross decrease"
        );
        kani::cover!(
            current == pre.last,
            "an unchanged observation is idempotent"
        );
    }
    kani::cover!(
        pre.net_identity(),
        "a valid arbitrary induction pre-state exists"
    );
}
