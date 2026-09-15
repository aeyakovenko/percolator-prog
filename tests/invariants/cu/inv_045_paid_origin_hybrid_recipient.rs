//! INV-045 / row 422: retained paid-origin penalties with a Hybrid reward recipient.
//! This covers the missing partition where the stale-price liquidation source and
//! the reward recipient both require authenticated report catchup in the same
//! public crank, across all four paid-discovery routes and both publication
//! orders. It is still bounded evidence, not an arbitrary-history entitlement proof.

use super::*;

#[test]
fn v16_program_paid_origin_penalty_survives_dual_hybrid_recipient_catchup() {
    let mut reference = None;
    let mut peak = 0;
    let mut worlds = 0;
    for cpi in [false, true] {
        for batch in [false, true] {
            for publish_first in [false, true] {
                let (values, budgets, fees, cu) =
                    run_retained_handoff_with_hybrid_recipient((batch, cpi, publish_first));
                let outcome = (values, budgets, fees);
                if let Some(expected) = &reference {
                    assert_eq!(
                        &outcome, expected,
                        "cpi={cpi} batch={batch} publish_first={publish_first}"
                    );
                } else {
                    reference = Some(outcome);
                }
                peak = peak.max(cu);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    eprintln!("INV-045 paid-origin Hybrid recipient: worlds={worlds}, peak_cu={peak}");
}
