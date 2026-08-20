// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol fee arithmetic for both launchpads.
//!
//! The two RFPs specify different collection models for the same reason, and
//! the reason is worth stating because it decides the code:
//!
//!   * **RFP-015, per swap.** A bonding curve is demand-bounded. Roughly 0.7%
//!     to 1.4% of curves ever reach their supply target, so a fee taken at
//!     close earns nothing on 98%+ of launches.
//!   * **RFP-016, at close.** An LBP is time-bounded. Every sale reaches its end
//!     timestamp whatever demand does, so the fee is always collectible and
//!     taking it per swap would only tax the participants.
//!
//! ROUNDING
//!
//! Every fee rounds **up**, and therefore against the party paying it: the
//! trader on a swap, the creator at close. This is the same rule the pricing
//! code follows and for the same reason — the residue belongs to the side that
//! cannot choose when to transact.
//!
//! THE CAP IS IN THE PROGRAM, NOT IN THE POLICY
//!
//! Both proposals ship at a zero rate with a governance-activatable switch.
//! A switch with no ceiling is a promise; a switch with a ceiling compiled into
//! the program is a constraint. [`FeeConfig::new`] refuses a rate above the
//! cap, so no admin authority can set one later, and the refusal is a named
//! error rather than a clamp — silently clamping an out-of-range rate would
//! hide a misconfiguration that someone should see.

use crate::{mul_div_ceil, CurveError};

type Result<T> = core::result::Result<T, CurveError>;

/// Fee rates are fixed point with six fractional digits: 1_000_000 is 100%,
/// so 1 unit is one ten-thousandth of a percent. Six digits is enough to
/// express the dynamic 0.05%–0.95% band Pump.fun uses without rounding the
/// rate itself, which would be a silent change to someone's economics.
pub const RATE_ONE: u128 = 1_000_000;

/// RFP-015's per-swap cap: 1% of the collateral moved.
pub const CAP_PER_SWAP: u128 = 10_000;

/// RFP-016's at-close cap: 5% of collateral raised, the rate Fjord Foundry
/// charges and the figure the RFP cites.
pub const CAP_AT_CLOSE: u128 = 50_000;

/// A rate and the ceiling it may never exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeConfig {
    rate: u128,
    cap: u128,
}

impl FeeConfig {
    /// Refuses a rate above the cap rather than clamping it.
    pub fn new(rate: u128, cap: u128) -> Result<Self> {
        if cap > RATE_ONE {
            return Err(CurveError::ZeroAmount);
        }
        if rate > cap {
            return Err(CurveError::ExceedsRealCollateral);
        }
        Ok(Self { rate, cap })
    }

    /// The rate both proposals ship at.
    pub fn zero(cap: u128) -> Result<Self> {
        Self::new(0, cap)
    }

    pub fn rate(&self) -> u128 {
        self.rate
    }

    pub fn cap(&self) -> u128 {
        self.cap
    }

    /// Change the rate under the same ceiling. This is the whole surface a
    /// governance fee switch needs, and it cannot be used to raise the cap.
    pub fn set_rate(&mut self, rate: u128) -> Result<()> {
        if rate > self.cap {
            return Err(CurveError::ExceedsRealCollateral);
        }
        self.rate = rate;
        Ok(())
    }

    /// `amount * rate`, rounded up.
    pub fn fee_on(&self, amount: u128) -> Result<u128> {
        if self.rate == 0 {
            return Ok(0);
        }
        let fee = mul_div_ceil(amount, self.rate, RATE_ONE)?;
        // Rounding up can only ever reach the amount itself when the rate is
        // 100%, which the caps forbid; asserting it here means a future cap
        // change cannot quietly produce a fee that consumes the principal.
        if fee > amount {
            return Err(CurveError::ExceedsRealCollateral);
        }
        Ok(fee)
    }
}

/// RFP-015 buy: the fee comes off the input **before** pricing, so the constant
/// product sees `c_in - fee`. Returns `(fee, effective_input)`.
///
/// Taking it after pricing would let the trader buy against collateral the pool
/// never receives, which inflates the curve by exactly the fee on every trade.
pub fn buy_fee(cfg: &FeeConfig, c_in: u128) -> Result<(u128, u128)> {
    if c_in == 0 {
        return Err(CurveError::ZeroAmount);
    }
    let fee = cfg.fee_on(c_in)?;
    let effective = c_in.checked_sub(fee).ok_or(CurveError::Overflow)?;
    if effective == 0 {
        // A trade whose entire input is fee is not a trade.
        return Err(CurveError::ZeroAmount);
    }
    Ok((fee, effective))
}

/// RFP-015 sell: the fee comes off the raw output **after** pricing. Returns
/// `(fee, paid_to_seller)`.
pub fn sell_fee(cfg: &FeeConfig, c_out_raw: u128) -> Result<(u128, u128)> {
    let fee = cfg.fee_on(c_out_raw)?;
    let paid = c_out_raw.checked_sub(fee).ok_or(CurveError::Overflow)?;
    Ok((fee, paid))
}

/// RFP-016 close: the fee comes off the collateral balance when the creator
/// withdraws. Returns `(fee, paid_to_creator)`.
pub fn close_fee(cfg: &FeeConfig, collateral_balance: u128) -> Result<(u128, u128)> {
    let fee = cfg.fee_on(collateral_balance)?;
    let paid = collateral_balance
        .checked_sub(fee)
        .ok_or(CurveError::Overflow)?;
    Ok((fee, paid))
}
