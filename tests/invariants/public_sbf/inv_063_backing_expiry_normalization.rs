//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr367_post_expiry_backing_fee_rejects_and_preserves_exit`, `v16_program_pr363_expired_backing_conversion_rejects_and_preserves_senior_exit`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: these fixed-pin regressions cover the minimized PR367 trade and PR363 PnL
//! conversion traces. The generated expiry matrices supply broader bounded route/boundary evidence.

use super::*;

#[test]
fn v16_program_pr367_post_expiry_backing_fee_rejects_and_preserves_exit() {
    let reproduction = reproduce_post_expiry_backing_fee(
        [0x67; 32],
        PostExpiryBackingCase {
            fee_bps: 5_000,
            expiry_offset: 2,
            mark_move_bps: 500,
            increase_divisor: 20,
        },
    )
    .expect("verify the fixed PR367 public trace");

    assert_eq!(reproduction.blocker, KnownBlocker::PostExpiryBackingFee);
    assert!(
        reproduction.risk_increase_rejected_stale,
        "the retained post-expiry risk increase did not return EngineStale"
    );
    assert!(
        reproduction.rejected_exact_rollback,
        "the rejected retained trade did not roll back tracked economic accounts"
    );
    assert_eq!(reproduction.victim_capital_loss, 0);
    assert_eq!(reproduction.provider_earnings, 0);
    assert_eq!(reproduction.extracted_tokens, 0);
    assert!(reproduction.risk_reduction_landed);
    assert!(reproduction.position_after_reduction_q < reproduction.position_before_reduction_q);
    assert!(reproduction.token_supply_conserved);
}

#[test]
fn v16_program_pr363_expired_backing_conversion_rejects_and_preserves_senior_exit() {
    let mut discoveries = discover_expired_backing_consumers([0x63; 32], 2)
        .expect("verify the fixed PR363 public trace");
    assert_eq!(discoveries.len(), 1);
    let discovery = discoveries.pop().unwrap();
    assert_eq!(
        discovery.kind,
        ExpiredBackingConsumerKind::ReleasedPnlConversion
    );
    assert!(
        discovery.rejects_lapsed_conversion_and_preserves_senior_exit(),
        "fixed PR363 trace did not reject safely with a senior exit: {discovery:?}"
    );
}
