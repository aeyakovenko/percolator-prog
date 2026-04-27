//! Base-token / units conversion at the deposit / withdraw boundary.
//! Verbatim port of the legacy `mod units`. Pure math, no deps.

/// Convert base token amount to units, returning `(units, dust)`.
/// Base token is the collateral (e.g., lamports for SOL, satoshis for BTC).
/// `scale == 0` disables scaling: returns `(base, 0)`.
#[inline]
pub fn base_to_units(base: u64, scale: u32) -> (u64, u64) {
    if scale == 0 {
        return (base, 0);
    }
    let s = scale as u64;
    (base / s, base % s)
}

/// Convert units to base token amount with overflow check.
/// Returns `None` if overflow would occur.
#[inline]
pub fn units_to_base_checked(units: u64, scale: u32) -> Option<u64> {
    if scale == 0 {
        return Some(units);
    }
    units.checked_mul(scale as u64)
}
