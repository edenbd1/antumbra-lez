// SPDX-License-Identifier: MIT OR Apache-2.0
//! The same fractional power as [`crate::weighted`], on a binary working scale.
//!
//! WHY THIS EXISTS
//!
//! `weighted::pow_frac` is accurate to 8.6e-17 and costs 314,248 zkVM cycles.
//! Measurement said why: the working scale is decimal, so each of its 24 atanh
//! terms carries a `mul_div` — a full 256-bit division — where a binary scale
//! would need a shift.
//!
//! Two changes, and they compound:
//!
//!   * **Work at 2^62.** Division by the scale becomes `>> 62`.
//!   * **Reduce symmetrically into `[1/sqrt(2), sqrt(2))`** instead of
//!     `[1/2, 1)`. That bounds the series argument at |t| <= 0.1716 rather than
//!     1/3, so `t^2 <= 0.0295` and thirteen terms leave a tail of 4.6e-22 —
//!     where the decimal version needed twenty-four to reach 1e-19.
//!
//! Half the terms, and each one a shift instead of a division.
//!
//! WHAT IS LEFT THAT STILL DIVIDES
//!
//! Three divisions per call, none of them in a loop: converting the argument
//! from the 1e18 public scale, forming `t = (m-1)/(m+1)` once, and applying the
//! weight ratio to the logarithm. The return trip to 1e18 is a multiply and a
//! shift because 2^62 is a power of two.
//!
//! SIGNS
//!
//! The reduction is symmetric, so `ln(m)` can be either sign and the
//! intermediates are `i128`. That is the cost of the tighter bound, and it is
//! why this module does not simply reuse the unsigned helpers next door.

use crate::CurveError;

type Result<T> = core::result::Result<T, CurveError>;

/// Internal working scale: values are fixed-point with 62 fractional bits.
pub const LN_SCALE: u32 = 62;
const ONE62: i128 = 1i128 << LN_SCALE;

/// The public scale, matching [`crate::weighted::ONE`].
const ONE18: i128 = 1_000_000_000_000_000_000;

/// round(ln(2) * 2^62)
const LN2_62: i128 = 3_196_577_161_300_663_915;
/// round(2^30 / ln(2)), used to estimate the exp range reduction without a division.
const INV_LN2_30: i128 = 1_549_082_005;
/// round(2^62 / sqrt(2)) — the lower edge of the reduction window.
const SQRT2_HALF_62: i128 = 3_260_954_456_333_195_553;

/// Beyond this the true `exp(-y)` is below 2^-63 and the result is zero at any
/// scale we can represent. Capping here also keeps `y * INV_LN2_30` far inside
/// `i128`.
const EXP_ZERO_ABOVE: i128 = 100 * LN2_62;

/// round(2^62 / (2i+1)) for i = 0..=12.
/// `ln(m) = 2t * (1 + t^2/3 + t^4/5 + ...)` with `t = (m-1)/(m+1)`.
const ATANH_62: [i128; 13] = [
    4_611_686_018_427_387_904,
    1_537_228_672_809_129_301,
    922_337_203_685_477_581,
    658_812_288_346_769_701,
    512_409_557_603_043_100,
    419_244_183_493_398_900,
    354_745_078_340_568_300,
    307_445_734_561_825_860,
    271_275_648_142_787_524,
    242_720_316_759_336_205,
    219_604_096_115_589_900,
    200_508_087_757_712_518,
    184_467_440_737_095_516,
];

/// `(-1)^i * round(2^62 / i!)` for i = 0..=20, so a Horner pass in `s >= 0`
/// evaluates `exp(-s)` directly.
const EXP_62: [i128; 21] = [
    4_611_686_018_427_387_904,
    -4_611_686_018_427_387_904,
    2_305_843_009_213_693_952,
    -768_614_336_404_564_651,
    192_153_584_101_141_163,
    -38_430_716_820_228_233,
    6_405_119_470_038_039,
    -915_017_067_148_291,
    114_377_133_393_536,
    -12_708_570_377_060,
    1_270_857_037_706,
    -115_532_457_973,
    9_627_704_831,
    -740_592_679,
    52_899_477,
    -3_526_632,
    220_414,
    -12_966,
    720,
    -38,
    2,
];

/// `-ln(x)` at scale 2^62, for `x` at scale 2^62 in `(0, 2^62]`. Result `>= 0`.
///
/// Shift-normalises `x = m * 2^-k` with `m` in `[1/sqrt(2), sqrt(2))`, forms
/// `t = (m-1)/(m+1)` with the one division, then sums the odd atanh series.
/// `-ln x = k*ln2 - ln m`, and because the window straddles 1 the second term
/// is small and signed rather than large and cancelling.
pub fn neg_ln_62(x62: i128) -> Result<i128> {
    if x62 <= 0 || x62 > ONE62 {
        return Err(CurveError::ZeroAmount);
    }
    let mut m = x62;
    let mut k: i128 = 0;
    while m < SQRT2_HALF_62 {
        m <<= 1;
        k += 1;
    }
    // m is now in [2^62/sqrt(2), 2^62*sqrt(2)), so |m - ONE62| <= 0.4143 * 2^62
    // and (m - ONE62) << 62 stays well inside i128.
    let num = m - ONE62;
    let den = m + ONE62;
    let t = (num << LN_SCALE) / den; // |t| <= 0.1716 * 2^62
    let u = (t * t) >> LN_SCALE; // t^2 <= 0.0295 * 2^62
    let mut p = ATANH_62[12];
    for i in (0..12).rev() {
        p = ((p * u) >> LN_SCALE) + ATANH_62[i];
    }
    // 2 * t * P(t^2), so shift by 61 rather than 62 and skip the doubling.
    let ln_m = (t * p) >> (LN_SCALE - 1);
    Ok(k * LN2_62 - ln_m)
}

/// `exp(-y)` at scale 2^62 for `y >= 0` at scale 2^62.
///
/// Range-reduces by `ln2` using a multiply by `1/ln2` rather than a division,
/// then runs the alternating Taylor series on the remainder `s` in `[0, ln2)`,
/// where twenty-one terms leave a tail of 8.9e-24.
pub fn exp_neg_62(y: i128) -> Result<i128> {
    if y <= 0 {
        return Ok(ONE62);
    }
    if y >= EXP_ZERO_ABOVE {
        return Ok(0);
    }
    let mut k = (y * INV_LN2_30) >> (30 + LN_SCALE);
    let mut s = y - k * LN2_62;
    // The estimate can land one step either side; correct it rather than
    // trusting a rounded reciprocal.
    if s < 0 {
        k -= 1;
        s += LN2_62;
    } else if s >= LN2_62 {
        k += 1;
        s -= LN2_62;
    }
    if k >= 63 {
        return Ok(0);
    }
    let mut acc = EXP_62[20];
    for i in (0..20).rev() {
        acc = ((acc * s) >> LN_SCALE) + EXP_62[i];
    }
    if acc < 0 {
        acc = 0;
    }
    Ok(acc >> k)
}

/// `x^(num/den)` with `x` and the result at scale 1e18, `x` in `(0, 1e18]`.
///
/// Contract-identical to [`crate::weighted::pow_frac`]; only the internals
/// differ. `x^(n/n)` returns `x` unchanged rather than `x` plus a rounding
/// error, which matters because an LBP sits at equal weights whenever its
/// schedule crosses 50/50.
pub fn pow_frac(x: u128, num: u128, den: u128) -> Result<u128> {
    if den == 0 || num == 0 {
        return Err(CurveError::ZeroAmount);
    }
    if x == 0 {
        return Ok(0);
    }
    if x > ONE18 as u128 {
        return Err(CurveError::QuantityAtOrAboveReserve);
    }
    if num == den {
        return Ok(x);
    }
    // Normalise BEFORE converting scale. Converting straight from 1e18 to 2^62
    // is a factor of only 4.6, so a small `x` arrives with almost no mantissa
    // left: x = 19 (1.9e-17) becomes 87, which is six bits, and the error that
    // survives the exponent is 1e-4 rather than 1e-16. Measured, not guessed —
    // it is what the first version of this function did.
    //
    // So shift `x` up into [1e18/2, 1e18) first, counting the shifts, and
    // convert a full-precision mantissa. `x = m * 2^-j`, hence
    // `-ln x = j*ln2 + (-ln m)`, an addition of two non-negative terms.
    let mut xx = x;
    let mut j: i128 = 0;
    while xx < (ONE18 as u128) / 2 {
        xx <<= 1;
        j += 1;
    }
    // xx is in [5e17, 1e18], so m0 lands in [2^61, 2^62] with every bit used.
    let m0 = crate::mul_div_floor(xx, ONE62 as u128, ONE18 as u128)? as i128;
    if m0 <= 0 {
        return Ok(0);
    }
    let x62 = m0.min(ONE62);
    let nl = j * LN2_62 + neg_ln_62(x62)?;
    // Apply the weight ratio to the logarithm: the second division.
    let y = crate::mul_div_floor(nl as u128, num, den)? as i128;
    let r = exp_neg_62(y)?;
    // Back to 1e18: a multiply and a shift, since 2^62 is a power of two.
    Ok(((r * ONE18) >> LN_SCALE) as u128)
}

/// The RFP-016 swap, on the binary kernel.
///
/// `tokens_out = Rt * (1 - (Rc / (Rc + C_in))^(w_c / w_t))`, with the payout
/// rounded down so the residue stays with the pool — the same rule the
/// constant-product side follows. `Rt * (1 - p)` is taken through `mul_div`
/// rather than multiplied directly: at 18 decimals the product reaches 1e42 and
/// a `u128` stops at 3.4e38, which is the same overflow the bonding curve's `k`
/// runs into.
pub fn weighted_buy(
    reserve_token: u128,
    reserve_collateral: u128,
    c_in: u128,
    w_token: u128,
    w_collateral: u128,
) -> Result<u128> {
    if c_in == 0 || reserve_token == 0 || reserve_collateral == 0 {
        return Err(CurveError::ZeroAmount);
    }
    if w_token == 0 || w_collateral == 0 {
        return Err(CurveError::ZeroAmount);
    }
    let denom = reserve_collateral
        .checked_add(c_in)
        .ok_or(CurveError::Overflow)?;
    let base = crate::mul_div_floor(reserve_collateral, ONE18 as u128, denom)?;
    let p = pow_frac(base, w_collateral, w_token)?;
    let one_minus = (ONE18 as u128).checked_sub(p).ok_or(CurveError::Overflow)?;
    let out = crate::mul_div_floor(reserve_token, one_minus, ONE18 as u128)?;
    if out > reserve_token {
        return Err(CurveError::ExceedsSaleReserve);
    }
    Ok(out)
}
