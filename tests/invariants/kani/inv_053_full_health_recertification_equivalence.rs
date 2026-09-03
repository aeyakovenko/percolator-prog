//! INV-053 - Full-health recertification equivalence.
//!
//! The engine owns health arithmetic and the exact postcondition of each certificate writer. These
//! proofs own the wrapper boundary without reimplementing that arithmetic. They prove that the
//! deployed account codec preserves every certificate field, that the conservative lane ordering
//! is transitive, and that the three safety-relevant engine dispositions compose over arbitrary
//! certificates: exact refresh, preservation across a monotone full-health frame, or invalidation.
//!
//! INV-088 source-locks every wrapper-to-engine transition callsite to one of those dispositions
//! (terminal-only routes do not consume a favorable-action certificate). The public INV-053
//! differential products establish the pinned engine obligations against an independent raw-state
//! full-refresh model at every generated checkpoint and at maximum supported shape.

use percolator::{HealthCertV16, HealthCertV16Account, V16Error};

#[derive(Clone, Copy)]
struct CertificateStateKey {
    oracle_epoch: u64,
    funding_epoch: u64,
    risk_epoch: u64,
    asset_set_epoch: u64,
    active_bitmap: percolator::V16ActiveBitmap,
}

fn inv053_certificate_is_current(cert: HealthCertV16, state: CertificateStateKey) -> bool {
    cert.valid
        && cert.cert_oracle_epoch == state.oracle_epoch
        && cert.cert_funding_epoch == state.funding_epoch
        && cert.cert_risk_epoch == state.risk_epoch
        && cert.cert_asset_set_epoch == state.asset_set_epoch
        && cert.active_bitmap_at_cert == state.active_bitmap
}

fn inv053_certificate_is_no_healthier_than(cached: HealthCertV16, fresh: HealthCertV16) -> bool {
    cached.valid
        && fresh.valid
        && cached.cert_oracle_epoch == fresh.cert_oracle_epoch
        && cached.cert_funding_epoch == fresh.cert_funding_epoch
        && cached.cert_risk_epoch == fresh.cert_risk_epoch
        && cached.cert_asset_set_epoch == fresh.cert_asset_set_epoch
        && cached.active_bitmap_at_cert == fresh.active_bitmap_at_cert
        && cached.certified_equity <= fresh.certified_equity
        && cached.certified_initial_req >= fresh.certified_initial_req
        && cached.certified_maintenance_req >= fresh.certified_maintenance_req
        && cached.certified_liq_deficit >= fresh.certified_liq_deficit
        && cached.certified_worst_case_loss >= fresh.certified_worst_case_loss
}

fn inv053_arbitrary_certificate() -> HealthCertV16 {
    HealthCertV16 {
        certified_equity: kani::any(),
        certified_initial_req: kani::any(),
        certified_maintenance_req: kani::any(),
        certified_liq_deficit: kani::any(),
        certified_worst_case_loss: kani::any(),
        cert_oracle_epoch: kani::any(),
        cert_funding_epoch: kani::any(),
        cert_risk_epoch: kani::any(),
        cert_asset_set_epoch: kani::any(),
        active_bitmap_at_cert: kani::any(),
        valid: kani::any(),
    }
}

#[kani::proof]
fn kani_inv053_health_certificate_account_codec_preserves_every_engine_field() {
    let source = inv053_arbitrary_certificate();
    let wire = HealthCertV16Account::from_runtime(&source);

    assert_eq!(wire.certified_equity.get(), source.certified_equity);
    assert_eq!(
        wire.certified_initial_req.get(),
        source.certified_initial_req
    );
    assert_eq!(
        wire.certified_maintenance_req.get(),
        source.certified_maintenance_req
    );
    assert_eq!(
        wire.certified_liq_deficit.get(),
        source.certified_liq_deficit
    );
    assert_eq!(
        wire.certified_worst_case_loss.get(),
        source.certified_worst_case_loss
    );
    assert_eq!(wire.cert_oracle_epoch.get(), source.cert_oracle_epoch);
    assert_eq!(wire.cert_funding_epoch.get(), source.cert_funding_epoch);
    assert_eq!(wire.cert_risk_epoch.get(), source.cert_risk_epoch);
    assert_eq!(wire.cert_asset_set_epoch.get(), source.cert_asset_set_epoch);
    assert_eq!(
        wire.active_bitmap_at_cert.map(|word| word.get()),
        source.active_bitmap_at_cert
    );
    assert_eq!(wire.valid != 0, source.valid);

    let decoded = wire.try_to_runtime();
    assert_eq!(decoded.is_ok(), source.certified_equity != i128::MIN);
    assert!(source.certified_equity == i128::MIN || decoded == Ok(source));
    assert!(source.certified_equity != i128::MIN || decoded == Err(V16Error::ArithmeticOverflow));
}

#[kani::proof]
fn kani_inv053_no_healthier_relation_is_reflexive_and_transitive() {
    let first = inv053_arbitrary_certificate();
    let second = inv053_arbitrary_certificate();
    let third = inv053_arbitrary_certificate();

    assert_eq!(
        inv053_certificate_is_no_healthier_than(first, first),
        first.valid
    );
    let adjacent = inv053_certificate_is_no_healthier_than(first, second)
        && inv053_certificate_is_no_healthier_than(second, third);
    assert!(!adjacent || inv053_certificate_is_no_healthier_than(first, third));
}

#[kani::proof]
fn kani_inv053_wrapper_commit_is_invalid_or_no_healthier_under_engine_disposition() {
    let disposition: u8 = kani::any();
    let before_cached = inv053_arbitrary_certificate();
    let before_full = inv053_arbitrary_certificate();
    let after_full = inv053_arbitrary_certificate();
    let committed = inv053_arbitrary_certificate();
    let after_state = CertificateStateKey {
        oracle_epoch: kani::any(),
        funding_epoch: kani::any(),
        risk_epoch: kani::any(),
        asset_set_epoch: kani::any(),
        active_bitmap: kani::any(),
    };

    // These are the exact postconditions owned by the pinned engine writers. They contain no
    // health arithmetic: exact refresh installs the full result; a framed cache composes two
    // conservative relations; invalidation makes the cache unusable by a favorable action.
    let exact_refresh =
        committed == after_full && inv053_certificate_is_current(after_full, after_state);
    let monotone_frame = committed == before_cached
        && inv053_certificate_is_no_healthier_than(before_cached, before_full)
        && inv053_certificate_is_no_healthier_than(before_full, after_full)
        && inv053_certificate_is_current(after_full, after_state);
    let invalidated = !inv053_certificate_is_current(committed, after_state);
    let known_disposition = disposition < 3;
    let engine_postcondition = (disposition != 0 || exact_refresh)
        && (disposition != 1 || monotone_frame)
        && (disposition != 2 || invalidated);

    kani::cover!(
        known_disposition && disposition == 0 && engine_postcondition,
        "exact full-health recertification disposition"
    );
    kani::cover!(
        known_disposition && disposition == 1 && engine_postcondition,
        "conservative untouched-cache disposition"
    );
    kani::cover!(
        known_disposition && disposition == 2 && engine_postcondition,
        "certificate invalidation disposition"
    );

    let committed_is_safe = !inv053_certificate_is_current(committed, after_state)
        || inv053_certificate_is_no_healthier_than(committed, after_full);
    assert!(!(known_disposition && engine_postcondition) || committed_is_safe);
}
