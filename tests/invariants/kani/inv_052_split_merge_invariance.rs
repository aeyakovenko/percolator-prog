//! INV-052 - split/merge invariance (wrapper persistence contract).
//!
//! The engine proves the canonical accrual arithmetic and path partition theorem. This wrapper
//! proof owns only the persisted carry boundary: every representable carry below the denominator
//! is accepted, while an out-of-range carry, invalid provenance, or any nonzero reserved byte
//! fails closed.

use percolator_prog::state;

#[kani::proof]
#[kani::unwind(34)]
fn kani_v16_inv052_oracle_profile_accepts_exact_remainder_domain() {
    let remainder: u16 = kani::any();
    let provenance: u8 = kani::any();
    let reserved: [u8; 3] = kani::any();
    let mut profile = state::manual_asset_oracle_profile(100, 0);
    profile.oracle_mode = percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS;
    profile.oracle_leg_count = 1;
    profile.max_staleness_secs = 1;
    profile.hybrid_soft_stale_slots = 1;
    profile.oracle_leg_feeds[0] = [1u8; 32];
    profile.price_move_remainder_bps_num = remainder;
    profile.effective_price_provenance = provenance;
    profile._padding0 = reserved;

    let accepted = state::validate_asset_oracle_profile(&profile).is_ok();
    assert_eq!(
        accepted,
        remainder < 10_000
            && provenance <= percolator_prog::constants::EFFECTIVE_PRICE_PROVENANCE_TRADE_DRIVEN
            && reserved == [0; 3]
    );

    kani::cover!(
        remainder == 0 && provenance == 0 && reserved == [0; 3],
        "zero carry with authenticated provenance is accepted"
    );
    kani::cover!(
        remainder == 9_999 && provenance == 1 && reserved == [0; 3],
        "maximum canonical carry is accepted"
    );
    kani::cover!(
        remainder == 10_000 && provenance == 0 && reserved == [0; 3],
        "denominator-sized carry is rejected"
    );
    kani::cover!(
        remainder < 10_000 && provenance > 1 && reserved == [0; 3],
        "invalid provenance fails closed"
    );
    kani::cover!(
        remainder < 10_000 && provenance <= 1 && reserved != [0; 3],
        "reserved bytes fail closed"
    );
}
