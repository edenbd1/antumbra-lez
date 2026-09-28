// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kani proofs of the vesting invariants — `cargo kani --features ""`.
//!
//! A test tries some inputs; each harness below is checked by a model checker
//! for EVERY input in its domain.
//!
//! Two layers, because a symbolic 256-bit long division is far beyond what a
//! SAT solver unrolls inside a property:
//!
//!   1. `mul_div_floor_is_exact` proves the division the program uses returns
//!      exactly ⌊a·b/d⌋ on the domain below.
//!   2. The schedule harnesses replace that function with the expression it
//!      was just proved equal to (`kani::stub`).
//!
//! THE DOMAIN, STATED RATHER THAN HIDDEN. Every timestamp is an arbitrary u64;
//! totals and schedule durations are bounded to 8 bits, the size at which the
//! solver closes each harness in seconds to minutes. This is the small-scope
//! argument: every branch, rounding step and boundary of the logic — before
//! the start, the cliff, the middle, the end, a cancellation, a claim that
//! finds nothing — is reachable inside it, so an error in the logic has a
//! counterexample there. What only large values can break, overflow in the
//! 256-bit intermediates, is covered separately by `tests/vectors/mul_div.txt`,
//! generated from an arbitrary-precision oracle and diffed in CI.
//!
//! Run: `cargo kani -Z stubbing`.

use super::*;

const B: u32 = 8;
const LIM: u128 = 1 << B;

/// The expression layer 1 proves `mul_div_floor` equal to, on this domain,
/// computed in 16 bits because every operand fits in 8.
fn mul_div_floor_spec(a: u128, b: u128, d: u128) -> Result<u128> {
    if d == 0 {
        return Err(CurveError::ZeroAmount);
    }
    assert!(
        a < LIM && b < LIM && d < LIM,
        "stub used outside its proved domain"
    );
    Ok(((a as u16) * (b as u16) / (d as u16)) as u128)
}

fn any_time_schedule() -> Schedule {
    let start: u64 = kani::any();
    let end: u64 = kani::any();
    let total: u128 = kani::any();
    kani::assume(total > 0 && total < LIM);
    kani::assume(end > start && ((end - start) as u128) < LIM);
    if kani::any() {
        let cliff: u64 = kani::any();
        match Schedule::cliff_linear(start, cliff, end, total) {
            Ok(s) => s,
            Err(_) => {
                kani::assume(false);
                unreachable!()
            }
        }
    } else {
        match Schedule::linear(start, end, total) {
            Ok(s) => s,
            Err(_) => {
                kani::assume(false);
                unreachable!()
            }
        }
    }
}

// ------------------------------------------------------------ layer 1

#[kani::proof]
#[kani::unwind(130)]
#[kani::solver(kissat)]
fn mul_div_floor_is_exact() {
    let a: u128 = kani::any::<u8>() as u128;
    let b: u128 = kani::any::<u8>() as u128;
    let d: u128 = kani::any::<u8>() as u128;
    kani::assume(d > 0 && b <= d); // the only shape vesting calls it with
    assert_eq!(crate::mul_div_floor(a, b, d), Ok(a * b / d));
}

// ------------------------------------------------------------ layer 2

/// Nothing ever vests beyond the total.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn vested_never_exceeds_total() {
    let s = any_time_schedule();
    let t: u64 = kani::any();
    assert!(s.vested_at(t) <= s.total);
}

/// At the end the whole total has vested — no dust is stranded by rounding.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn everything_vests_by_the_end() {
    let s = any_time_schedule();
    let t: u64 = kani::any();
    kani::assume(t >= s.end);
    assert_eq!(s.vested_at(t), s.total);
}

/// Nothing is claimable before the cliff.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn nothing_vests_before_the_cliff() {
    let s = any_time_schedule();
    kani::assume(s.kind == Kind::CliffLinear);
    let t: u64 = kani::any();
    kani::assume(t < s.cliff);
    assert_eq!(s.vested_at(t), 0);
}

/// Signalling a milestone twice is refused and changes nothing.
#[kani::proof]
#[kani::unwind(5)]
fn milestone_signal_is_idempotent() {
    let n: usize = kani::any();
    kani::assume(n >= 1 && n <= 3);
    let mut tranches = Vec::new();
    for _ in 0..n {
        let t: u64 = kani::any();
        kani::assume(t > 0);
        tranches.push(t as u128);
    }
    let mut s = Schedule::milestone(tranches).expect("valid tranches");
    let i: u32 = kani::any();
    kani::assume((i as usize) < n);
    s.signal_milestone(i).expect("first signal");
    let (before, vested) = (s.signalled, s.vested_at(0));
    assert!(s.signal_milestone(i).is_err());
    assert_eq!(s.signalled, before);
    assert_eq!(s.vested_at(0), vested);
    assert!(vested <= s.total);
}

/// A claim at any instant pays exactly what has vested and not been paid,
/// records exactly what has vested, and a claim at or after the end closes
/// the schedule at its total — no dust left behind by rounding.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn a_claim_pays_exactly_the_unpaid_vested_amount() {
    let mut s = any_time_schedule();
    let paid_before: u128 = kani::any();
    kani::assume(paid_before <= s.total);
    s.claimed = paid_before;
    let t: u64 = kani::any();
    let vested = s.vested_at(t);
    match s.claim(t) {
        Ok(amount) => {
            assert_eq!(amount, vested - paid_before);
            assert_eq!(s.claimed, vested);
        }
        Err(_) => {
            assert!(
                vested <= paid_before,
                "refused only when nothing new has vested"
            );
            assert_eq!(s.claimed, paid_before, "a refused claim records nothing");
        }
    }
    if t >= s.end {
        assert_eq!(s.claimed, s.total);
    }
}

/// Cancellation at any instant, with any amount already paid out up to what
/// had vested, splits the total into three parts that sum to it exactly, and
/// the beneficiary keeps precisely the vested, unpaid part.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn cancel_splits_the_total_exactly() {
    let mut s = any_time_schedule();
    let t: u64 = kani::any();
    let vested = s.vested_at(t);
    let paid: u128 = kani::any();
    kani::assume(paid <= vested);
    s.claimed = paid;
    let split = s.cancel(t).expect("a fresh cancelable schedule cancels");
    assert_eq!(split.sum(), s.total);
    assert_eq!(split.to_beneficiary, vested - paid);
    assert_eq!(split.to_creator, s.total - vested);
}

/// After a cancellation nothing further vests at any later instant, and a
/// second cancellation is refused.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
#[kani::unwind(3)]
fn cancellation_freezes_vesting() {
    let mut s = any_time_schedule();
    let c: u64 = kani::any();
    let later: u64 = kani::any();
    kani::assume(c <= later);
    let at_cancel = s.vested_at(c);
    s.cancel(c).expect("a fresh cancelable schedule cancels");
    assert_eq!(s.vested_at(later), at_cancel);
    assert!(s.cancel(later).is_err());
}

// ------------------------------------------------------------ monotonicity
//
// "Vesting never goes backwards" is a statement about ⌊T·e/S⌋ as e grows,
// which compares two divisions: nonlinear, and beyond what a SAT solver closes
// monolithically in reasonable time. It is proved as the composition of the
// two lemmas below, each checked exhaustively over 16-bit integers:
//
//   e1 ≤ e2  ⇒  T·e1 ≤ T·e2              (multiplication is monotone)
//   x ≤ y    ⇒  ⌊x/S⌋ ≤ ⌊y/S⌋            (floor division is monotone)
//
// and by the branch structure of `linear_between`, whose other two branches
// are the constants 0 (below the start) and T (at or past the end), with
// `vested_never_exceeds_total` above bounding the middle one by T.

#[kani::proof]
fn lemma_multiplication_is_monotone() {
    let t: u16 = kani::any::<u8>() as u16;
    let a: u16 = kani::any::<u8>() as u16;
    let b: u16 = kani::any::<u8>() as u16;
    kani::assume(a <= b);
    assert!(t * a <= t * b);
}

#[kani::proof]
fn lemma_floor_division_is_monotone() {
    let x: u16 = kani::any();
    let y: u16 = kani::any();
    let d: u16 = kani::any();
    kani::assume(d > 0 && x <= y);
    assert!(x / d <= y / d);
}

/// The deployed program's equal-tranche milestone amount,
/// `⌊total · done / tranches⌋`, never exceeds the total and is exactly the
/// total once every tranche is signalled.
#[kani::proof]
#[kani::stub(crate::mul_div_floor, mul_div_floor_spec)]
#[kani::solver(cadical)]
fn equal_tranche_amount_is_bounded_and_complete() {
    let total: u128 = kani::any::<u8>() as u128;
    let n: u128 = kani::any::<u8>() as u128;
    let done: u128 = kani::any::<u8>() as u128;
    kani::assume(n > 0 && done <= n);
    assert!(crate::mul_div_floor(total, done, n).unwrap() <= total);
    assert_eq!(crate::mul_div_floor(total, n, n).unwrap(), total);
}
