fn add_mod_with_carry(lhs: u128, rhs: u128, modulus: u128) -> (u128, u128) {
    debug_assert!(modulus != 0 && lhs < modulus && rhs < modulus);
    let gap = modulus - rhs;
    if lhs >= gap {
        (1, lhs - gap)
    } else {
        (0, lhs + rhs)
    }
}

/// Exact `floor(lhs * rhs / denominator)` without using the engine's wide arithmetic.
pub fn mul_div_floor(lhs: u128, rhs: u128, denominator: u128) -> Result<u128, String> {
    mul_div_floor_with_remainder(lhs, rhs, denominator).map(|(quotient, _)| quotient)
}

/// Exact quotient and remainder for `lhs * rhs / denominator` without wide multiplication.
pub fn mul_div_floor_with_remainder(
    lhs: u128,
    rhs: u128,
    denominator: u128,
) -> Result<(u128, u128), String> {
    if denominator == 0 {
        return Err("reference division by zero".into());
    }
    let whole = lhs / denominator;
    let quotient = whole
        .checked_mul(rhs)
        .ok_or("reference quotient overflow")?;
    let reduced_lhs = lhs % denominator;
    let mut remainder = 0u128;
    let mut fractional = 0u128;
    for bit in (0..u128::BITS).rev() {
        fractional = fractional
            .checked_mul(2)
            .ok_or("reference fractional overflow")?;
        let (double_carry, doubled) = add_mod_with_carry(remainder, remainder, denominator);
        fractional = fractional
            .checked_add(double_carry)
            .ok_or("reference fractional carry overflow")?;
        remainder = doubled;
        if rhs & (1u128 << bit) != 0 {
            let (add_carry, next) = add_mod_with_carry(remainder, reduced_lhs, denominator);
            fractional = fractional
                .checked_add(add_carry)
                .ok_or("reference add carry overflow")?;
            remainder = next;
        }
    }
    let quotient = quotient
        .checked_add(fractional)
        .ok_or_else(|| "reference result overflow".to_string())?;
    Ok((quotient, remainder))
}

fn gcd(mut lhs: u128, mut rhs: u128) -> u128 {
    while rhs != 0 {
        let remainder = lhs % rhs;
        lhs = rhs;
        rhs = remainder;
    }
    lhs
}

/// Exact `ceil(lhs * rhs / denominator)` without using the engine's wide arithmetic.
pub fn mul_div_ceil(lhs: u128, rhs: u128, denominator: u128) -> Result<u128, String> {
    if denominator == 0 {
        return Err("reference ceil division by zero".into());
    }
    let floor = mul_div_floor(lhs, rhs, denominator)?;
    let reduced_denominator = denominator / gcd(lhs, denominator);
    floor
        .checked_add(u128::from(rhs % reduced_denominator != 0))
        .ok_or_else(|| "reference ceil result overflow".into())
}
