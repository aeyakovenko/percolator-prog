//! INV-038: a composite price is rounded once, from legs that carry enough digits (#462).
//!
//! A composite is a product or a quotient of feed values, so the relative error of
//! a rounded leg carries into it whole. The legs used to enter the composition at
//! six decimals. A leg worth 0.00041 is 410 at six decimals: it is known to one
//! part in 410, which is 24 bps, the whole per-slot price cap of a production
//! market. Two authenticated feed tuples with the same cross-rate could store
//! composites 24 bps apart, and an account within that distance of maintenance
//! could be liquidated on rounding alone.
//!
//! The legs now enter at twelve decimals, and the composite is one exact ratio of
//! 64-bit factors, floored once. The six-decimal value of a leg is still what is
//! stored per leg and what decides whether a feed is admissible.

use super::*;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use percolator_prog::oracle_v16::{
    compose_price_e6, floor_ratio_of_u64_products, floor_scaled_ratio_of_u64_products, ORACLE_E12,
    ORACLE_E12_PER_E6,
};

const MAX_E6: u64 = percolator::MAX_ORACLE_PRICE;
const DIVIDE_LEG2: u8 = percolator_prog::constants::ORACLE_LEG_FLAG_DIVIDE_LEG2;
const DIVIDE_LEG3: u8 = percolator_prog::constants::ORACLE_LEG_FLAG_DIVIDE_LEG3;

/// A fixed, dependency-free generator: every run sees the same cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A 64-bit factor: edges, powers of ten, and values of every bit length.
    fn factor(&mut self) -> u64 {
        match self.below(8) {
            0 => [0, 1, 2, u64::MAX, u64::MAX - 1, 1 << 63, (1 << 32) - 1][self.below(7) as usize],
            1 => 10u64.pow(self.below(20) as u32),
            _ => self.next() >> self.below(64),
        }
    }

    /// A leg at twelve decimals, anywhere in the admissible range and beside its edges.
    /// The largest admissible leg is just below `(MAX_ORACLE_PRICE + 1)` at six decimals.
    fn leg_e12(&mut self) -> u64 {
        let max = (MAX_E6 + 1) * ORACLE_E12_PER_E6 - 1;
        match self.below(10) {
            0 => ORACLE_E12_PER_E6 + self.below(3),
            1 => max - self.below(3),
            2 => (1 + self.below(2_000)) * ORACLE_E12_PER_E6,
            // a few significant digits, at any magnitude
            3..=5 => {
                let digits = 1 + self.below(9_999);
                let scaled = digits * 10u64.pow(self.below(15) as u32);
                scaled.clamp(ORACLE_E12_PER_E6, max)
            }
            _ => ORACLE_E12_PER_E6 + self.next() % (max - ORACLE_E12_PER_E6 + 1),
        }
    }
}

/// A feed value at both scales, through the byte-level reader: for every exponent
/// a feed may carry and mantissas across the range, the twelve-decimal value is
/// the floor of the feed value, the six-decimal value is its floor again, and a
/// value outside `0.000001..=1,000,000` is refused at either scale.
#[test]
fn v16_program_oracle_leg_at_twelve_decimals_is_the_floor_of_the_feed_value() {
    const NOW: i64 = 1_000;
    let feed = [7u8; 32];
    let key = Pubkey::new_unique();
    let mut rng = Rng(0x4622);
    let (mut admitted, mut refused, mut floored) = (0u32, 0u32, 0u32);
    for exponent in -18i32..=18 {
        for _ in 0..600 {
            let mantissa = match rng.below(4) {
                0 => 1 + rng.below(2_000) as i64,
                1 => 10i64.pow(rng.below(19) as u32) + rng.below(3) as i64 - 1,
                _ => (rng.next() >> (1 + rng.below(63))) as i64,
            }
            .max(1);
            let data = make_pyth_data(&feed, mantissa, exponent, 0, NOW);
            let read = percolator_prog::oracle_v16::read_oracle_leg_price_from_bytes(
                &percolator_prog::oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                &key,
                &data,
                &feed,
                NOW,
                60,
                10_000,
            );
            // floor(mantissa * 10^(exponent + decimals))
            let at = |decimals: i32| {
                let shift = exponent + decimals;
                let power = BigUint::from(10u8).pow(shift.unsigned_abs());
                if shift >= 0 {
                    BigUint::from(mantissa as u64) * power
                } else {
                    BigUint::from(mantissa as u64) / power
                }
            };
            let (e6, e12) = (at(6), at(12));
            if e6 == BigUint::from(0u8) || e6 > BigUint::from(MAX_E6) {
                assert!(read.is_err(), "{mantissa}e{exponent}: {read:?}");
                refused += 1;
                continue;
            }
            let leg = read.unwrap_or_else(|e| panic!("{mantissa}e{exponent}: {e:?}"));
            assert_eq!(BigUint::from(leg.price_e6), e6, "{mantissa}e{exponent}");
            assert_eq!(BigUint::from(leg.price_e12), e12, "{mantissa}e{exponent}");
            assert_eq!(leg.price_e12 / ORACLE_E12_PER_E6, leg.price_e6);
            assert_eq!(leg.publish_time, NOW);
            admitted += 1;
            floored += u32::from(leg.price_e12 % ORACLE_E12_PER_E6 != 0);
        }
    }
    assert!(admitted > 3_000 && refused > 3_000 && floored > 1_000);
}

fn product(factors: &[u64]) -> BigUint {
    factors.iter().fold(BigUint::from(1u8), |acc, factor| {
        acc * BigUint::from(*factor)
    })
}

/// The limb routine against arbitrary-precision arithmetic: up to four 64-bit
/// numerator factors and up to six denominator factors, the floor of the whole
/// ratio, `None` exactly when a denominator is zero, there are more than four
/// numerator factors, or the result does not fit `u128`.
#[test]
fn v16_program_floor_ratio_of_u64_products_matches_bigint() {
    let mut rng = Rng(0x462);
    let mut cases = 0u32;
    let mut none_zero = 0u32;
    let mut none_wide = 0u32;
    let mut four_by_four = 0u32;
    for _ in 0..200_000 {
        let numerators: Vec<u64> = (0..rng.below(5)).map(|_| rng.factor()).collect();
        let denominators: Vec<u64> = (0..rng.below(7)).map(|_| rng.factor()).collect();
        let got = floor_ratio_of_u64_products(&numerators, &denominators);
        let expected = if denominators.contains(&0) {
            none_zero += 1;
            None
        } else {
            let quotient = product(&numerators) / product(&denominators);
            let fits = quotient.to_u128();
            none_wide += u32::from(fits.is_none());
            fits
        };
        assert_eq!(got, expected, "{numerators:?} / {denominators:?}");
        four_by_four += u32::from(numerators.len() == 4 && denominators.len() == 4);
        cases += 1;
    }
    assert_eq!(floor_ratio_of_u64_products(&[1; 5], &[1]), None);
    assert_eq!(floor_ratio_of_u64_products(&[7], &[1; 9]), Some(7));
    assert_eq!(floor_ratio_of_u64_products(&[], &[]), Some(1));
    assert_eq!(
        floor_ratio_of_u64_products(&[u64::MAX; 4], &[u64::MAX, u64::MAX]),
        Some(u128::from(u64::MAX) * u128::from(u64::MAX))
    );
    assert!(none_zero > 10_000 && none_wide > 1_000 && four_by_four > 5_000);
    println!("floor ratio: {cases} cases, {none_zero} zero divisors, {none_wide} too wide");
}

/// The scaled ratio the composition is evaluated with, against arbitrary-precision
/// arithmetic: up to three 64-bit factors a side and a power of ten on either
/// side. It must give the same floor whether the terms fit `u128` (one native
/// division) or not (the limb routine), and `None` exactly when a denominator is
/// zero, an argument is out of range, or the result does not fit `u128`.
#[test]
fn v16_program_floor_scaled_ratio_of_u64_products_matches_bigint() {
    let mut rng = Rng(0x4623);
    let (mut some, mut none, mut wide, mut refused_wide) = (0u32, 0u32, 0u32, 0u32);
    for _ in 0..200_000 {
        let numerators: Vec<u64> = (0..rng.below(4)).map(|_| rng.factor()).collect();
        let denominators: Vec<u64> = (0..rng.below(4)).map(|_| rng.factor()).collect();
        let pow10 = rng.below(109) as i32 - 54;
        let got = floor_scaled_ratio_of_u64_products(&numerators, &denominators, pow10);
        let expected = if denominators.contains(&0) {
            None
        } else {
            let ten = BigUint::from(10u8).pow(pow10.unsigned_abs());
            let (mut numerator, mut denominator) = (product(&numerators), product(&denominators));
            // The terms: both products, the power of ten, and the scaled product.
            let mut narrow = numerator.to_u128().is_some()
                && denominator.to_u128().is_some()
                && ten.to_u128().is_some();
            if pow10 >= 0 {
                numerator *= &ten;
            } else {
                denominator *= &ten;
            }
            narrow &= numerator.to_u128().is_some() && denominator.to_u128().is_some();
            // Beyond `u128` the numerator is held as at most four 64-bit factors, the power
            // of ten in pieces of at most 10^18: more than that is refused.
            wide += u32::from(!narrow);
            let pieces = if pow10 > 0 {
                (pow10 as usize + 17) / 18
            } else {
                0
            };
            if !narrow && numerators.len() + pieces > 4 {
                refused_wide += 1;
                None
            } else {
                (numerator / denominator).to_u128()
            }
        };
        assert_eq!(
            got, expected,
            "{numerators:?} / {denominators:?} * 10^{pow10}"
        );
        some += u32::from(expected.is_some());
        none += u32::from(expected.is_none());
    }
    assert_eq!(floor_scaled_ratio_of_u64_products(&[1; 4], &[1], 0), None);
    assert_eq!(floor_scaled_ratio_of_u64_products(&[1], &[1; 4], 0), None);
    assert_eq!(floor_scaled_ratio_of_u64_products(&[1], &[1], 55), None);
    assert_eq!(floor_scaled_ratio_of_u64_products(&[1], &[1], -55), None);
    assert_eq!(
        floor_scaled_ratio_of_u64_products(&[], &[], 38),
        Some(10u128.pow(38))
    );
    assert_eq!(floor_scaled_ratio_of_u64_products(&[], &[], 39), None);
    assert_eq!(
        floor_scaled_ratio_of_u64_products(&[], &[1_000_000], 42),
        Some(10u128.pow(36))
    );
    assert!(some > 50_000 && none > 20_000 && wide > 50_000 && refused_wide > 1_000);
    println!(
        "scaled ratio: {some} values, {none} refused ({refused_wide} for a numerator too wide), \
         {wide} beyond u128 on the way"
    );
}

/// What the composite must be, in arbitrary precision: the legs at twelve
/// decimals as exact fractions, multiplied or divided, inverted if configured,
/// scaled to six decimals and floored once; then the unit scale. `None` where the
/// program refuses: an intermediate or final price outside `1..=MAX_ORACLE_PRICE`.
fn reference(legs_e12: &[u64], flags: u8, invert: u8, unit_scale: u32) -> Option<u64> {
    let one = BigUint::from(ORACLE_E12);
    let max = BigUint::from(MAX_E6);
    // price at six decimals = numerator / denominator
    let mut numerator = BigUint::from(legs_e12[0]);
    let mut denominator = BigUint::from(ORACLE_E12_PER_E6);
    for (index, leg) in legs_e12.iter().enumerate().skip(1) {
        let divides = match index {
            1 => flags & DIVIDE_LEG2 != 0,
            _ => flags & DIVIDE_LEG3 != 0,
        };
        if divides {
            numerator *= &one;
            denominator *= BigUint::from(*leg);
        } else {
            numerator *= BigUint::from(*leg);
            denominator *= &one;
        }
        let intermediate = &numerator / &denominator;
        if intermediate == BigUint::from(0u8) || intermediate > max {
            return None;
        }
    }
    let mut price = if invert != 0 {
        (denominator * BigUint::from(1_000_000_000_000u64)) / numerator
    } else {
        numerator / denominator
    };
    if unit_scale > 1 {
        price /= BigUint::from(unit_scale);
    }
    if price == BigUint::from(0u8) || price > max {
        return None;
    }
    price.to_u64()
}

fn flag_sets(legs: usize) -> &'static [u8] {
    match legs {
        1 => &[0],
        2 => &[0, DIVIDE_LEG2],
        _ => &[0, DIVIDE_LEG2, DIVIDE_LEG3, DIVIDE_LEG2 | DIVIDE_LEG3],
    }
}

/// The composition against the reference for one, two and three legs, every
/// multiply/divide pattern, with and without inversion, and unit scales of 1, 7
/// and 1,000,000: equal where the reference has a price, refused where it has
/// none.
#[test]
fn v16_program_composite_price_is_the_floor_of_the_exact_ratio_of_twelve_decimal_legs() {
    let mut rng = Rng(0x4620);
    let mut priced = 0u32;
    let mut refused = 0u32;
    let mut three_multiplied = 0u32;
    for _ in 0..40_000 {
        let legs: Vec<u64> = (0..1 + rng.below(3)).map(|_| rng.leg_e12()).collect();
        for flags in flag_sets(legs.len()) {
            for invert in [0u8, 1] {
                for unit_scale in [1u32, 7, 1_000_000] {
                    let expected = reference(&legs, *flags, invert, unit_scale);
                    let got = compose_price_e6(&legs, *flags, invert, unit_scale);
                    match expected {
                        Some(price) => {
                            assert_eq!(
                                got,
                                Ok(price),
                                "{legs:?} flags {flags} invert {invert} scale {unit_scale}"
                            );
                            priced += 1;
                            three_multiplied += u32::from(legs.len() == 3 && *flags == 0);
                        }
                        None => {
                            assert_eq!(
                                got,
                                Err(PercolatorError::OracleInvalid.into()),
                                "{legs:?} flags {flags} invert {invert} scale {unit_scale}"
                            );
                            refused += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(priced > 100_000 && refused > 100_000 && three_multiplied > 2_000);
    println!(
        "composite: {priced} priced, {refused} refused, {three_multiplied} three-leg products"
    );

    // A leg outside the admissible range, and a malformed configuration, are refused
    // as before.
    for bad in [0, ORACLE_E12_PER_E6 - 1, (MAX_E6 + 1) * ORACLE_E12_PER_E6] {
        assert_eq!(
            compose_price_e6(&[bad], 0, 0, 1),
            Err(PercolatorError::OracleInvalid.into())
        );
        assert_eq!(
            compose_price_e6(&[ORACLE_E12, bad], 0, 0, 1),
            Err(PercolatorError::OracleInvalid.into())
        );
    }
    let malformed: [(&[u64], u8, u8); 6] = [
        (&[], 0, 0),
        (&[ORACLE_E12; 4], 0, 0),
        (&[ORACLE_E12], DIVIDE_LEG2, 0),
        (&[ORACLE_E12; 2], DIVIDE_LEG3, 0),
        (&[ORACLE_E12], 0, 2),
        (&[ORACLE_E12], 8, 0),
    ];
    for (legs, flags, invert) in malformed {
        assert_eq!(
            compose_price_e6(legs, flags, invert, 1),
            Err(PercolatorError::EngineInvalidConfig.into()),
            "{legs:?} flags {flags} invert {invert}"
        );
    }
}

/// The composition as it was: every leg floored to six decimals first.
fn composite_of_six_decimal_legs(
    legs_e6: &[u64],
    flags: u8,
    invert: u8,
    unit_scale: u32,
) -> Option<u64> {
    let legs_e12: Vec<u64> = legs_e6.iter().map(|leg| leg * ORACLE_E12_PER_E6).collect();
    reference(&legs_e12, flags, invert, unit_scale)
}

/// Feeds that carry no more than six decimals compose exactly as they did. Only
/// what a leg had beyond its sixth decimal is new information.
#[test]
fn v16_program_composite_price_of_six_decimal_legs_is_unchanged() {
    let mut rng = Rng(0x4621);
    let mut priced = 0u32;
    for _ in 0..40_000 {
        let legs_e6: Vec<u64> = (0..1 + rng.below(3))
            .map(|_| rng.leg_e12() / ORACLE_E12_PER_E6)
            .collect();
        let legs_e12: Vec<u64> = legs_e6.iter().map(|leg| leg * ORACLE_E12_PER_E6).collect();
        for flags in flag_sets(legs_e6.len()) {
            for invert in [0u8, 1] {
                // The six-decimal composition, written out independently of the
                // twelve-decimal reference above.
                let mut numerator = BigUint::from(legs_e6[0]);
                let mut denominator = BigUint::from(1u8);
                let mut valid = true;
                for (index, leg) in legs_e6.iter().enumerate().skip(1) {
                    let divides = match index {
                        1 => flags & DIVIDE_LEG2 != 0,
                        _ => flags & DIVIDE_LEG3 != 0,
                    };
                    if divides {
                        numerator *= BigUint::from(1_000_000u64);
                        denominator *= BigUint::from(*leg);
                    } else {
                        numerator *= BigUint::from(*leg);
                        denominator *= BigUint::from(1_000_000u64);
                    }
                    let intermediate = &numerator / &denominator;
                    valid &=
                        intermediate != BigUint::from(0u8) && intermediate <= BigUint::from(MAX_E6);
                }
                let old = valid
                    .then(|| {
                        if invert != 0 {
                            (denominator * BigUint::from(1_000_000_000_000u64)) / numerator
                        } else {
                            numerator / denominator
                        }
                    })
                    .filter(|price| *price != BigUint::from(0u8) && *price <= BigUint::from(MAX_E6))
                    .and_then(|price| price.to_u64());
                assert_eq!(
                    compose_price_e6(&legs_e12, *flags, invert, 1).ok(),
                    old,
                    "{legs_e6:?} flags {flags} invert {invert}"
                );
                assert_eq!(
                    composite_of_six_decimal_legs(&legs_e6, *flags, invert, 1),
                    old
                );
                priced += u32::from(old.is_some());
            }
        }
    }
    assert!(priced > 50_000);
}

/// The two feed tuples of #462. Their cross-rate is the same to seven digits:
/// 0.00041000 * 150.000000 = 0.0615000000 and 0.00041099 * 149.638677 =
/// 0.0614999999. From six-decimal legs the second was stored as 61,351, a move of
/// 24 bps that no feed reported.
const TUPLES: [((i64, i32), (i64, i32), u64, u64); 2] = [
    ((4_100_000, -10), (150_000_000, -6), 61_500, 61_500),
    ((4_109_900, -10), (149_638_677, -6), 61_499, 61_351),
];

#[test]
fn v16_program_composite_of_a_low_priced_leg_follows_the_cross_rate() {
    for ((x, x_expo), (jpy, jpy_expo), exact_floor, from_six_decimals) in TUPLES {
        let e12 = |mantissa: i64, expo: i32| {
            (BigUint::from(mantissa as u64) * BigUint::from(10u8).pow((12 + expo) as u32))
                .to_u64()
                .unwrap()
        };
        let legs_e12 = [e12(x, x_expo), e12(jpy, jpy_expo)];
        assert_eq!(compose_price_e6(&legs_e12, 0, 0, 1), Ok(exact_floor));
        let legs_e6 = legs_e12.map(|leg| leg / ORACLE_E12_PER_E6);
        assert_eq!(
            composite_of_six_decimal_legs(&legs_e6, 0, 0, 1),
            Some(from_six_decimals),
            "what the six-decimal legs gave"
        );
    }
    // The inverse of a low-priced single feed had the same defect: 1 / 0.00041099.
    assert_eq!(compose_price_e6(&[410_990_000], 0, 1, 1), Ok(2_433_149_225));
    assert_eq!(
        composite_of_six_decimal_legs(&[410], 0, 1, 1),
        Some(2_439_024_390)
    );
}

fn crank_with_reports(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    slot: u64,
    oracle_accounts: &[Pubkey],
) -> Result<u64, String> {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(
        oracle_accounts
            .iter()
            .copied()
            .map(|key| AccountMeta::new_readonly(key, false)),
    );
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations_with_accounts(0, oracle_accounts.len() as u8),
        },
        accounts,
        &[],
    )
}

/// The same two tuples through the program: two fully verified Pyth reports per
/// tuple, a two-leg product configured by `ConfigureHybridOracle` and observed by
/// a permissionless crank. The stored target follows the cross-rate, 61,500 and
/// then 61,499, where it used to fall to 61,351.
#[test]
fn v16_program_composite_target_does_not_move_on_the_rounding_of_a_low_priced_leg() {
    let mut env = V16CuEnv::new();
    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    let feed_x = [0x46; 32];
    let feed_jpy = [0x62; 32];

    let (slot, time) = (10u64, 1_000i64);
    set_test_clock(&mut env, slot, time);
    let ((x, x_expo), (jpy, jpy_expo), first, _) = TUPLES[0];
    let reports = [
        env.set_pyth_price(&feed_x, x, x_expo, time),
        env.set_pyth_price(&feed_jpy, jpy, jpy_expo, time),
    ];
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        2,
        0,
        [feed_x, feed_jpy, [0u8; 32]],
        &reports,
        slot,
        time,
        0,
        1,
        3,
        100,
    )
    .expect("a two-leg product of a low-priced feed and a currency feed");
    let config = env.market_state().0;
    assert_eq!(config.oracle_target_price_e6, first);
    // What is stored per leg is still the six-decimal value.
    assert_eq!(config.oracle_leg_prices_e6[..2], [410, 150_000_000]);

    let (slot, time) = (slot + 1, time + 1);
    set_test_clock(&mut env, slot, time);
    let ((x, x_expo), (jpy, jpy_expo), second, from_six_decimals) = TUPLES[1];
    let reports = [
        env.set_pyth_price(&feed_x, x, x_expo, time),
        env.set_pyth_price(&feed_jpy, jpy, jpy_expo, time),
    ];
    crank_with_reports(&mut env, keeper_portfolio, slot, &reports)
        .expect("the second tuple is observed");
    let config = env.market_state().0;
    assert_eq!(config.oracle_target_publish_time, time);
    assert_eq!(config.oracle_leg_prices_e6[..2], [410, 149_638_677]);
    assert_eq!(
        config.oracle_target_price_e6, second,
        "the cross-rate moved by less than one unit of the sixth decimal"
    );
    assert_ne!(config.oracle_target_price_e6, from_six_decimals);
}

/// What the extra digits cost. A feed that carries no digit beyond the sixth
/// decimal gets its twelve-decimal value by one 64-bit multiplication; a feed
/// that does (Pyth at exponent -8 or -10, Switchboard at eighteen decimals) needs
/// one wide multiplication or division per leg. The same three-leg composite is
/// observed by a crank from exponent -6 reports and from exponent -10 reports of
/// the same values: the stored target is the same, and the finer reports cost at
/// most 1,000 CU more for the whole composite. The maximum-shape refresh
/// (`v16_bpf_public_full_14_leg_three_feed_oracle_refresh_is_bounded`, fourteen
/// such composites from exponent -6 reports, 975,000 CU guardrail) therefore has
/// room for fourteen composites of finer reports.
#[test]
fn v16_program_composite_from_finer_reports_costs_a_bounded_amount_more() {
    const VALUES_E6: [i64; 3] = [3_000_000, 150_000_000, 200_000_000];
    let observe = |exponent: i32| {
        let scale = 10i64.pow((-6 - exponent) as u32);
        let mut env = V16CuEnv::new();
        let keeper = Keypair::new();
        let keeper_portfolio = env.create_portfolio(&keeper);
        let feeds = [[0x31u8; 32], [0x32u8; 32], [0x33u8; 32]];
        let (slot, time) = (10u64, 1_000i64);
        set_test_clock(&mut env, slot, time);
        let reports: Vec<Pubkey> = (0..3)
            .map(|leg| env.set_pyth_price(&feeds[leg], VALUES_E6[leg] * scale, exponent, time))
            .collect();
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            3,
            DIVIDE_LEG2 | DIVIDE_LEG3,
            feeds,
            &reports,
            slot,
            time,
            0,
            1,
            3,
            100,
        )
        .expect("a three-leg composite");
        let (slot, time) = (slot + 1, time + 1);
        set_test_clock(&mut env, slot, time);
        // 2.850000 / 150 / 200 = 0.000095
        let reports: Vec<Pubkey> = (0..3)
            .map(|leg| {
                let value = if leg == 0 { 2_850_000 } else { VALUES_E6[leg] };
                env.set_pyth_price(&feeds[leg], value * scale, exponent, time)
            })
            .collect();
        let cu = crank_with_reports(&mut env, keeper_portfolio, slot, &reports)
            .expect("the composite is observed");
        (env.market_state().0.oracle_target_price_e6, cu)
    };
    let (coarse_target, coarse_cu) = observe(-6);
    let (fine_target, fine_cu) = observe(-10);
    assert_eq!(coarse_target, 95);
    assert_eq!(fine_target, coarse_target);
    println!("three-leg composite crank: {coarse_cu} CU from exponent -6, {fine_cu} from -10");
    assert!(
        fine_cu >= coarse_cu && fine_cu - coarse_cu <= 1_000,
        "finer reports cost {fine_cu} against {coarse_cu}"
    );
}
