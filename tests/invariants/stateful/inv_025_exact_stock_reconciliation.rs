//! INV-025 - exact stock reconciliation.
//!
//! This public-SBF lifecycle exercises capital, insurance, counterparty backing,
//! realized PnL, route-switched close, released-PnL conversion, backing withdrawal,
//! and owner withdrawals. After every successful transition the shared census:
//!
//! - independently sums capital, positive PnL, escrow, stale flags, and negative-PnL
//!   counts from every materialized portfolio;
//! - independently sums source claims, fresh backing, insurance reservations,
//!   budgets, backing earnings, and resolved-payout blockers from every domain;
//! - compares those sums with both decoded state and the raw zero-copy market header;
//! - requires the engine vault to equal the real SPL vault; and
//! - partitions custody exactly into explicit senior stocks plus a nonnegative
//!   derived junior residual.
//!
//! The same census also runs after every generated action in the shared stateful
//! runner. This test does not claim independent persisted ledgers for rounding
//! residue or protocol surplus; those remain represented by the derived residual.

use super::*;

#[test]
fn v16_program_public_value_lifecycle_reconciles_every_materialized_stock_census() {
    verify_exact_stock_reconciliation_lifecycle([0x25; 32])
        .unwrap_or_else(|error| panic!("INV-025 stock lifecycle: {error}"));
}
