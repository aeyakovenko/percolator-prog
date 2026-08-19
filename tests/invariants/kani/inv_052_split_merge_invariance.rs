//! INV-052 - split/merge invariance (wrapper persistence contract).
//!
//! The engine proves the canonical accrual arithmetic and path partition theorem. This wrapper
//! proof owns only the persisted carry boundary: every representable carry below the denominator
//! is accepted, while an out-of-range carry or any nonzero reserved byte fails closed.

use percolator_prog::state;

#[kani::proof]
#[kani::unwind(34)]
fn kani_v16_inv052_oracle_profile_accepts_exact_remainder_domain() {
    let remainder: u16 = kani::any();
    let reserved: u32 = kani::any();
    let mut profile = state::manual_asset_oracle_profile(100, 0);
    profile.price_move_remainder_bps_num = remainder;
    profile._padding0 = reserved.to_le_bytes();

    let accepted = state::validate_asset_oracle_profile(&profile).is_ok();
    assert_eq!(accepted, remainder < 10_000 && reserved == 0);

    kani::cover!(remainder == 0 && reserved == 0, "zero carry is accepted");
    kani::cover!(
        remainder == 9_999 && reserved == 0,
        "maximum canonical carry is accepted"
    );
    kani::cover!(
        remainder == 10_000 && reserved == 0,
        "denominator-sized carry is rejected"
    );
    kani::cover!(
        remainder < 10_000 && reserved != 0,
        "reserved bytes fail closed"
    );
}
