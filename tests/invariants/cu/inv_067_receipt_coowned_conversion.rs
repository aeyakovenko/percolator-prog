//! INV-067 / row 417: co-owned receipt destinations across source conversion
//! and later backing expiry. The ordinary conversion/expiry history proves
//! separate receipt owners; this selector reuses the same public route while
//! forcing both claimant portfolios to share one owner and one SPL destination.
//! It checks that receipt identity and per-portfolio entitlement do not collapse
//! into destination-level aggregate accounting.

use super::*;

#[test]
fn v16_program_coowned_receipts_preserve_attribution_across_conversion_and_late_expiry() {
    receipt_conversion_then_expiry::run_committed_conversion_then_late_expiry(true);
}
