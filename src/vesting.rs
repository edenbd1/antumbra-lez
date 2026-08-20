// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vesting schedule arithmetic for LEZ, integer only.
//!
//! WHAT THIS IS FOR
//!
//! RFP-017 asks for three schedule shapes — cliff+linear, fully linear, and
//! milestone — plus cancellation that splits an escrow three ways, a
//! transferable beneficiary, and idempotent milestone signalling. All of that
//! is arithmetic and state transitions, so all of it can be settled before any
//! account model exists. This module is that part.
//!
//! NOTHING IS STORED THAT CAN GO STALE
//!
//! `vested_at` is a pure function of the schedule and a timestamp. There is no
//! cached "current vested amount" to refresh, so there is no window in which a
//! claim prices against a stale value, and no poke to forget. The same choice
//! is made in [`crate::weighted::weight_at`] for the same reason.
//!
//! ROUNDING
//!
//! Linear accrual rounds DOWN, so the residue stays in escrow and is paid on a
//! later claim rather than early. Two properties make that safe rather than
//! merely conservative, and both are asserted in the tests:
//!
//!   * over a fully elapsed schedule, the claims sum to the total EXACTLY —
//!     rounding down each step never strands dust, because the final step is
//!     computed against the total rather than accumulated from the steps;
//!   * no prefix of claims ever exceeds the amount vested at that instant.
//!
//! THE CANCELLATION SPLIT IS WHERE THIS GETS SUBTLE
//!
//! Cancelling divides the original total into three parts: already claimed
//! (gone), vested-but-unclaimed (still the beneficiary's, after cancellation),
//! and unvested (returned to the creator). Computing any of them independently
//! is how the three stop summing to the total. Here all three come out of one
//! function, from the same `vested_at` the claim path uses, and the test sweeps
//! every cancellation instant across a schedule asserting the sum.

use crate::CurveError;

type Result<T> = core::result::Result<T, CurveError>;

/// Milestone tranches are a fixed-width bitmap: signalling is a compare-and-set
/// on one bit, which is what makes it idempotent structurally rather than by a
/// check somebody has to remember to write.
pub const MAX_MILESTONES: u32 = 64;

/// Which accrual rule a schedule follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Nothing before `cliff`; from `cliff` to `end`, linear in elapsed time.
    /// The cliff itself pays the lump that has accrued up to it, which for this
    /// shape is zero — the RFP's "lump sum at the cliff" is the linear amount
    /// measured from the cliff, not an extra payment.
    CliffLinear,
    /// Linear from `start` to `end`, no cliff.
    Linear,
    /// `tranches[i]` unlocks when milestone `i` is signalled. Time plays no
    /// part, so `vested_at` ignores the timestamp entirely.
    Milestone,
}

/// One vesting position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub kind: Kind,
    pub start: u64,
    /// Only meaningful for [`Kind::CliffLinear`]; must satisfy
    /// `start <= cliff < end`.
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    pub claimed: u128,
    pub cancelable: bool,
    pub transferable: bool,
    pub cancelled_at: Option<u64>,
    /// Tranche amounts for [`Kind::Milestone`]; empty otherwise.
    pub tranches: Vec<u128>,
    /// Bit `i` set means milestone `i` has been signalled.
    pub signalled: u64,
}

/// What a cancellation owes each party. The three always sum to `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelSplit {
    /// Already paid out before the cancellation.
    pub already_claimed: u128,
    /// Vested but not yet claimed; the beneficiary keeps the right to it.
    pub to_beneficiary: u128,
    /// Never vested; returns to the creator.
    pub to_creator: u128,
}

impl Schedule {
    /// A cliff+linear schedule. `start <= cliff < end` and `total > 0`.
    pub fn cliff_linear(start: u64, cliff: u64, end: u64, total: u128) -> Result<Self> {
        if total == 0 {
            return Err(CurveError::ZeroAmount);
        }
        if !(start <= cliff && cliff < end) {
            return Err(CurveError::ZeroAmount);
        }
        Ok(Self {
            kind: Kind::CliffLinear,
            start,
            cliff,
            end,
            total,
            claimed: 0,
            cancelable: true,
            transferable: false,
            cancelled_at: None,
            tranches: Vec::new(),
            signalled: 0,
        })
    }

    /// A fully linear schedule. `start < end` and `total > 0`.
    pub fn linear(start: u64, end: u64, total: u128) -> Result<Self> {
        if total == 0 || start >= end {
            return Err(CurveError::ZeroAmount);
        }
        Ok(Self {
            kind: Kind::Linear,
            start,
            cliff: start,
            end,
            total,
            claimed: 0,
            cancelable: true,
            transferable: false,
            cancelled_at: None,
            tranches: Vec::new(),
            signalled: 0,
        })
    }

    /// A milestone schedule. `total` is the sum of the tranches, checked here
    /// rather than trusted, so a schedule can never escrow an amount its
    /// tranches cannot pay out.
    pub fn milestone(tranches: Vec<u128>) -> Result<Self> {
        if tranches.is_empty() || tranches.len() as u32 > MAX_MILESTONES {
            return Err(CurveError::ZeroAmount);
        }
        let mut total: u128 = 0;
        for t in &tranches {
            if *t == 0 {
                return Err(CurveError::ZeroAmount);
            }
            total = total.checked_add(*t).ok_or(CurveError::Overflow)?;
        }
        Ok(Self {
            kind: Kind::Milestone,
            start: 0,
            cliff: 0,
            end: 0,
            total,
            claimed: 0,
            cancelable: true,
            transferable: false,
            cancelled_at: None,
            tranches,
            signalled: 0,
        })
    }

    /// Amount vested at `now`.
    ///
    /// Once cancelled, vesting stops at the cancellation instant: a cancelled
    /// schedule keeps paying what had already accrued and nothing more. Passing
    /// a later `now` therefore cannot increase the answer.
    pub fn vested_at(&self, now: u64) -> u128 {
        let t = match self.cancelled_at {
            Some(c) => now.min(c),
            None => now,
        };
        match self.kind {
            Kind::Milestone => self
                .tranches
                .iter()
                .enumerate()
                .filter(|(i, _)| self.signalled & (1u64 << i) != 0)
                .map(|(_, amt)| *amt)
                .sum(),
            Kind::Linear => linear_between(self.start, self.end, t, self.total),
            Kind::CliffLinear => {
                if t < self.cliff {
                    0
                } else {
                    linear_between(self.cliff, self.end, t, self.total)
                }
            }
        }
    }

    /// Vested minus already claimed. Saturating rather than checked because a
    /// cancelled schedule can leave `claimed` above the frozen vested amount
    /// only if a claim raced the cancellation, and the correct answer there is
    /// "nothing further is owed", not an error.
    pub fn claimable_at(&self, now: u64) -> u128 {
        self.vested_at(now).saturating_sub(self.claimed)
    }

    /// Claim everything currently claimable. Returns the amount paid.
    ///
    /// The caller records `claimed` only after the transfer succeeds; that
    /// ordering is the program's job, and it is the single most dangerous
    /// choice in a vesting contract — recording first burns a beneficiary's
    /// tokens on a failed transfer.
    pub fn claim(&mut self, now: u64) -> Result<u128> {
        let amount = self.claimable_at(now);
        if amount == 0 {
            return Err(CurveError::ZeroAmount);
        }
        self.claimed = self
            .claimed
            .checked_add(amount)
            .ok_or(CurveError::Overflow)?;
        Ok(amount)
    }

    /// Signal milestone `index`. Rejects a second signal of the same index
    /// deterministically, and rejects an index outside the tranche list.
    pub fn signal_milestone(&mut self, index: u32) -> Result<u128> {
        if self.kind != Kind::Milestone {
            return Err(CurveError::ZeroAmount);
        }
        if self.cancelled_at.is_some() {
            return Err(CurveError::ZeroAmount);
        }
        if index as usize >= self.tranches.len() {
            return Err(CurveError::ExceedsSaleReserve);
        }
        let bit = 1u64 << index;
        if self.signalled & bit != 0 {
            return Err(CurveError::SlippageExceeded);
        }
        self.signalled |= bit;
        Ok(self.tranches[index as usize])
    }

    /// Make a cancelable schedule permanent. One-way, by construction: there is
    /// no inverse of this call anywhere in the module.
    pub fn make_non_cancelable(&mut self) -> Result<()> {
        if !self.cancelable {
            return Err(CurveError::ZeroAmount);
        }
        self.cancelable = false;
        Ok(())
    }

    /// Cancel at `now`, returning the three-way split.
    ///
    /// All three parts come from one `vested_at` call so they cannot drift
    /// apart, and they are guaranteed to sum to `total`.
    pub fn cancel(&mut self, now: u64) -> Result<CancelSplit> {
        if !self.cancelable {
            return Err(CurveError::ZeroAmount);
        }
        if self.cancelled_at.is_some() {
            return Err(CurveError::ZeroAmount);
        }
        let vested = self.vested_at(now);
        let already_claimed = self.claimed;
        let to_beneficiary = vested.saturating_sub(already_claimed);
        let to_creator = self
            .total
            .checked_sub(vested)
            .ok_or(CurveError::Overflow)?;
        self.cancelled_at = Some(now);
        Ok(CancelSplit {
            already_claimed,
            to_beneficiary,
            to_creator,
        })
    }

    /// True once every token has been paid out.
    pub fn is_fully_claimed(&self) -> bool {
        self.claimed >= self.total
    }
}

impl CancelSplit {
    /// The invariant the whole split exists to preserve.
    pub fn sum(&self) -> u128 {
        self.already_claimed + self.to_beneficiary + self.to_creator
    }
}

/// Linear accrual over `[from, to)`, rounding down, saturating at both ends.
///
/// The final step is deliberately computed as `total` rather than as
/// `mul_div(elapsed, total, span)` with `elapsed == span`: the two agree
/// mathematically, but reaching the end through the general branch would make
/// exactness depend on the division being exact, which it is not in general.
/// Handling it as its own case is why claims sum to the total with no residue.
fn linear_between(from: u64, to: u64, now: u64, total: u128) -> u128 {
    if now <= from {
        return 0;
    }
    if now >= to {
        return total;
    }
    let elapsed = (now - from) as u128;
    let span = (to - from) as u128;
    // span > elapsed > 0 here, so the quotient cannot exceed total and
    // mul_div_floor cannot fail.
    crate::mul_div_floor(total, elapsed, span).unwrap_or(0)
}
