// SPDX-License-Identifier: MIT OR Apache-2.0
//! Constant-product bonding curve math for LEZ, integer only.
//!
//! WHY THIS CRATE EXISTS, AND WHAT IT FOUND
//!
//! RFP-015's reference implementation says: "`k = Vt × Vc` (computed and stored
//! at creation)". For any realistic 18-decimal token pair that number does not
//! exist in a `u128`:
//!
//! ```text
//! Vt = 1e9 tokens x 1e18 = 1e27
//! Vc = 1e6 tokens x 1e18 = 1e24
//! k  = 1e51                       u128::MAX = 3.4e38
//! ```
//!
//! So `k` cannot be stored as a `u128`, and every pricing call that divides by
//! it needs a 256-bit intermediate. This crate never materialises `k`. Each of
//! the three formulas folds into one `mul_div` whose product is computed in 256
//! bits and whose quotient is proven to fit in 128:
//!
//! ```text
//! buy      tokens_out = Vt - k/(Vc + C_in)  = Vt - mul_div(Vt, Vc, Vc + C_in)
//! inverse  C_in       = k/(Vt - Q) - Vc     = mul_div(Vt, Vc, Vt - Q) - Vc
//! sell     C_out      = Vc - k/(Vt + t_in)  = Vc - mul_div(Vc, Vt, Vt + t_in)
//! ```
//!
//! The identity is exact: `k/(x) == (Vt·Vc)/x`, so folding changes nothing about
//! the invariant and removes the overflow.
//!
//! ROUNDING
//!
//! Every rounding decision favours the pool, which is what keeps it solvent:
//!
//!   * buy      `tokens_out` rounds DOWN  → the pool keeps the dust
//!   * inverse  `C_in`       rounds UP    → the buyer pays the dust
//!   * sell     `C_out`      rounds DOWN  → the pool keeps the dust
//!
//! A quotient that rounds the other way is not a rounding bug, it is a
//! withdrawal: it lets a trader extract value the invariant did not create.
//!
//! OVERFLOW
//!
//! Rust release builds wrap silently on integer overflow unless told otherwise.
//! Every arithmetic operation here is `checked_*` and returns `Err`, and the
//! release profile additionally sets `overflow-checks = true` so a missed one
//! panics in test rather than corrupting a reserve in production.

#![forbid(unsafe_code)]

/// Every way a pricing call can refuse. Each maps to one documented on-chain
/// error code; none of them is "an error occurred".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveError {
    /// The virtual token reserve must exceed the sale quantity at creation.
    VirtualTokenReserveTooSmall,
    /// A reserve or amount was zero where the formula requires it positive.
    ZeroAmount,
    /// The requested quantity is at or beyond the virtual token reserve, where
    /// the curve's price is unbounded.
    QuantityAtOrAboveReserve,
    /// The buy would take more than the sale reserve still holds.
    ExceedsSaleReserve,
    /// The sell would take more collateral than the real reserve holds.
    ExceedsRealCollateral,
    /// A 256-bit quotient did not fit in 128 bits.
    Overflow,
    /// Slippage: the computed output is below the caller's stated minimum.
    SlippageExceeded,
}

pub type Result<T> = core::result::Result<T, CurveError>;

// ---------------------------------------------------------------------------
// 256-bit intermediates
// ---------------------------------------------------------------------------

/// Full 128×128 → 256 product, as (hi, lo). Schoolbook over 64-bit limbs.
#[inline]
fn wide_mul(a: u128, b: u128) -> (u128, u128) {
    let (a_hi, a_lo) = (a >> 64, a & u64::MAX as u128);
    let (b_hi, b_lo) = (b >> 64, b & u64::MAX as u128);

    let lo_lo = a_lo * b_lo;
    let hi_lo = a_hi * b_lo;
    let lo_hi = a_lo * b_hi;
    let hi_hi = a_hi * b_hi;

    let mid = (lo_lo >> 64) + (hi_lo & u64::MAX as u128) + (lo_hi & u64::MAX as u128);
    let lo = (mid << 64) | (lo_lo & u64::MAX as u128);
    let hi = hi_hi + (hi_lo >> 64) + (lo_hi >> 64) + (mid >> 64);
    (hi, lo)
}

/// 256 ÷ 128 → (quotient, remainder), refusing a quotient that exceeds 128 bits.
/// Restoring long division, one bit at a time: 256 iterations, no unsafe, no
/// floating point, and no dependency.
fn wide_div(hi: u128, lo: u128, d: u128) -> Result<(u128, u128)> {
    if d == 0 {
        return Err(CurveError::ZeroAmount);
    }
    if hi >= d {
        // The quotient needs more than 128 bits.
        return Err(CurveError::Overflow);
    }
    let mut rem: u128 = hi;
    let mut quo: u128 = 0;
    let mut i = 128;
    while i > 0 {
        i -= 1;
        // rem = rem*2 + bit i of lo.
        //
        // `rem << 1` OVERFLOWS whenever rem carries its top bit, which happens
        // for any large divisor — and Rust drops the bit silently in release.
        // The first version of this function did exactly that and mispriced
        // roughly half the vectors; the differential test is what caught it.
        // So the top bit is read before the shift, and a set bit means the true
        // value is already >= 2^128 > d, hence larger than the divisor whatever
        // the comparison below would say.
        let carried = rem >> 127;
        let bit = (lo >> i) & 1;
        rem = (rem << 1) | bit;
        quo <<= 1;
        if carried == 1 || rem >= d {
            // wrapping_sub is exact here: true_rem - d < 2^128 by construction,
            // because rem < d before the shift makes true_rem < 2d.
            rem = rem.wrapping_sub(d);
            quo |= 1;
        }
    }
    Ok((quo, rem))
}

/// floor(a·b / d), with the product taken in 256 bits.
pub fn mul_div_floor(a: u128, b: u128, d: u128) -> Result<u128> {
    let (hi, lo) = wide_mul(a, b);
    Ok(wide_div(hi, lo, d)?.0)
}

/// ceil(a·b / d), with the product taken in 256 bits.
pub fn mul_div_ceil(a: u128, b: u128, d: u128) -> Result<u128> {
    let (hi, lo) = wide_mul(a, b);
    let (q, r) = wide_div(hi, lo, d)?;
    if r == 0 {
        Ok(q)
    } else {
        q.checked_add(1).ok_or(CurveError::Overflow)
    }
}

// ---------------------------------------------------------------------------
// The curve
// ---------------------------------------------------------------------------

/// A sale's live state. `k` is deliberately absent: see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curve {
    /// Virtual token reserve.
    pub vt: u128,
    /// Virtual collateral reserve.
    pub vc: u128,
    /// Sale reserve: real project tokens still available to buy.
    pub sale_reserve: u128,
    /// Real collateral the pool holds and a seller can draw from.
    pub real_collateral: u128,
}

impl Curve {
    /// `Vt > D` is the creation rule the RFP states; we enforce it here rather
    /// than trusting the caller, because a curve created below it prices the
    /// last token at infinity.
    pub fn new(vt: u128, vc: u128, sale_reserve: u128) -> Result<Self> {
        if vt == 0 || vc == 0 {
            return Err(CurveError::ZeroAmount);
        }
        if vt <= sale_reserve {
            return Err(CurveError::VirtualTokenReserveTooSmall);
        }
        Ok(Self {
            vt,
            vc,
            sale_reserve,
            real_collateral: 0,
        })
    }

    /// Spot price numerator/denominator, exact, no division: `p = Vc / Vt`.
    pub fn spot(&self) -> (u128, u128) {
        (self.vc, self.vt)
    }

    /// Tokens out for `c_in` collateral. Rounds DOWN, in the pool's favour.
    pub fn quote_buy(&self, c_in: u128) -> Result<u128> {
        if c_in == 0 {
            return Err(CurveError::ZeroAmount);
        }
        let denom = self.vc.checked_add(c_in).ok_or(CurveError::Overflow)?;
        // floor keeps the remaining virtual reserve HIGH, so tokens_out is low.
        let remaining = mul_div_ceil(self.vt, self.vc, denom)?;
        let out = self.vt.checked_sub(remaining).ok_or(CurveError::Overflow)?;
        if out > self.sale_reserve {
            return Err(CurveError::ExceedsSaleReserve);
        }
        Ok(out)
    }

    /// Exact collateral cost for `q` tokens. Rounds UP, against the buyer.
    pub fn quote_buy_exact_out(&self, q: u128) -> Result<u128> {
        if q == 0 {
            return Err(CurveError::ZeroAmount);
        }
        if q > self.sale_reserve {
            return Err(CurveError::ExceedsSaleReserve);
        }
        if q >= self.vt {
            return Err(CurveError::QuantityAtOrAboveReserve);
        }
        let denom = self.vt - q;
        let target = mul_div_ceil(self.vt, self.vc, denom)?;
        target.checked_sub(self.vc).ok_or(CurveError::Overflow)
    }

    /// Collateral out for `t_in` tokens sold back. Rounds DOWN, in the pool's
    /// favour, and never exceeds the real collateral the pool actually holds.
    pub fn quote_sell(&self, t_in: u128) -> Result<u128> {
        if t_in == 0 {
            return Err(CurveError::ZeroAmount);
        }
        let denom = self.vt.checked_add(t_in).ok_or(CurveError::Overflow)?;
        let remaining = mul_div_ceil(self.vc, self.vt, denom)?;
        let out = self.vc.checked_sub(remaining).ok_or(CurveError::Overflow)?;
        if out > self.real_collateral {
            return Err(CurveError::ExceedsRealCollateral);
        }
        Ok(out)
    }

    /// Apply a buy: state moves only if every check passed.
    pub fn buy(&mut self, c_in: u128, min_tokens_out: u128) -> Result<u128> {
        let out = self.quote_buy(c_in)?;
        if out < min_tokens_out {
            return Err(CurveError::SlippageExceeded);
        }
        self.vt = self.vt.checked_sub(out).ok_or(CurveError::Overflow)?;
        self.vc = self.vc.checked_add(c_in).ok_or(CurveError::Overflow)?;
        self.sale_reserve = self
            .sale_reserve
            .checked_sub(out)
            .ok_or(CurveError::Overflow)?;
        self.real_collateral = self
            .real_collateral
            .checked_add(c_in)
            .ok_or(CurveError::Overflow)?;
        Ok(out)
    }

    /// Apply a sell.
    pub fn sell(&mut self, t_in: u128, min_collateral_out: u128) -> Result<u128> {
        let out = self.quote_sell(t_in)?;
        if out < min_collateral_out {
            return Err(CurveError::SlippageExceeded);
        }
        self.vt = self.vt.checked_add(t_in).ok_or(CurveError::Overflow)?;
        self.vc = self.vc.checked_sub(out).ok_or(CurveError::Overflow)?;
        self.sale_reserve = self
            .sale_reserve
            .checked_add(t_in)
            .ok_or(CurveError::Overflow)?;
        self.real_collateral = self
            .real_collateral
            .checked_sub(out)
            .ok_or(CurveError::Overflow)?;
        Ok(out)
    }

    /// The sale closes when its reserve is exhausted — a consequence, not a call.
    pub fn is_closed(&self) -> bool {
        self.sale_reserve == 0
    }
}
pub mod weighted;

pub mod binfixed;
pub mod vesting;
