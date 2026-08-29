//! INV-037 - Exact residual partition.
//!
//! Normative obligation: every close-loss atom is classified exactly once. This owner mutation-
//! tests the independent deployed-ledger oracle, including the distinction between retired junior
//! claim face and realizable support atoms. Its own four-route public matrix creates 1,000 atoms of
//! source-attributed junior face against only 250 atoms of source principal plus one backing atom,
//! then lands a larger stale loss through `ForfeitRecoveryLeg`. Every route must retire all 1,000
//! face atoms while counting exactly 251 realizable support atoms once. Shared INV-076 evidence
//! separately checks the equation before and after a real residual-decreasing continuation under
//! price and funding drift.
//!
//! Guarantee boundary: the deployed ledger has `drift_consumed`, `support_consumed`, insurance,
//! B, explicit loss, and remaining residual. It does not expose separate fields for every abstract
//! provenance category named by the charter. This test proves the implemented partition oracle is
//! nonvacuous on synthetic and public state; it does not manufacture evidence for absent
//! provenance fields.

use crate::support::fuzz_model::{
    run_recovery_support_partition_probe, verify_close_residual_partition,
};
use percolator::{CloseProgressLedgerV16, SideV16};

fn valid_partition() -> CloseProgressLedgerV16 {
    CloseProgressLedgerV16 {
        active: true,
        finalized: false,
        canceled: false,
        close_id: 1,
        asset_index: 0,
        market_id: 1,
        domain_side: SideV16::Long,
        gross_loss_at_close_start: 17,
        drift_reference_slot: 3,
        max_close_slot: 9,
        support_consumed: 4,
        junior_face_burned: 11,
        insurance_spent: 5,
        b_loss_booked: 3,
        explicit_loss_assigned: 2,
        quantity_adl_applied_q: 0,
        drift_consumed: 3,
        residual_remaining: 6,
    }
}

#[test]
fn inv037_partition_oracle_counts_value_once_and_excludes_retired_face_metadata() {
    let valid = valid_partition();
    verify_close_residual_partition("valid", &valid).expect("valid partition");

    let larger_retired_face = CloseProgressLedgerV16 {
        junior_face_burned: 19,
        ..valid
    };
    verify_close_residual_partition("larger retired face", &larger_retired_face)
        .expect("retired claim face is not a second value payment");

    for (label, invalid) in [
        (
            "gross loss",
            CloseProgressLedgerV16 {
                gross_loss_at_close_start: valid.gross_loss_at_close_start + 1,
                ..valid
            },
        ),
        (
            "drift",
            CloseProgressLedgerV16 {
                drift_consumed: valid.drift_consumed + 1,
                ..valid
            },
        ),
        (
            "support",
            CloseProgressLedgerV16 {
                support_consumed: valid.support_consumed + 1,
                ..valid
            },
        ),
        (
            "insurance",
            CloseProgressLedgerV16 {
                insurance_spent: valid.insurance_spent + 1,
                ..valid
            },
        ),
        (
            "B loss",
            CloseProgressLedgerV16 {
                b_loss_booked: valid.b_loss_booked + 1,
                ..valid
            },
        ),
        (
            "explicit loss",
            CloseProgressLedgerV16 {
                explicit_loss_assigned: valid.explicit_loss_assigned + 1,
                ..valid
            },
        ),
        (
            "residual",
            CloseProgressLedgerV16 {
                residual_remaining: valid.residual_remaining + 1,
                ..valid
            },
        ),
    ] {
        assert!(
            verify_close_residual_partition(label, &invalid).is_err(),
            "mutating {label} must break the exact equation"
        );
    }

    let support_exceeds_face = CloseProgressLedgerV16 {
        junior_face_burned: valid.support_consumed - 1,
        ..valid
    };
    assert!(
        verify_close_residual_partition("support exceeds face", &support_exceeds_face).is_err()
    );
}

#[test]
fn inv037_public_recovery_counts_support_once_and_excludes_larger_retired_face() {
    let evidence = run_recovery_support_partition_probe([0x37; 32])
        .expect("public Recovery support partition matrix");
    assert_eq!(evidence.route_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.exact_partition_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.nonzero_support_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.face_exceeds_support_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.minimum_retired_face, 1_000, "{evidence:?}");
    assert_eq!(evidence.maximum_support_consumed, 251, "{evidence:?}");
}
