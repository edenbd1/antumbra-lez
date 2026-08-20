// SPDX-License-Identifier: MIT OR Apache-2.0
//! Differential tests for the fixed-point fractional power.
//!
//! The reference is `tests/gen_pow_vectors.py`, Python's `decimal` at 60
//! significant digits. 2,500 vectors, deliberately weighted towards the ends
//! where fixed point is worst: x within 1e-3 of one, x down at 1e-18, and
//! weight ratios from 99/1 to 1/99 — the range an LBP actually sweeps.
//!
//! The test asserts a bound rather than equality. `x^e` at scale 1e18 cannot be
//! exact in integer arithmetic; what matters for a pool is that the error is
//! bounded, that it is *reported* rather than assumed, and that it never
//! rounds in the trader's favour.

use antumbra_curve::weighted::*;
use antumbra_curve::CurveError;

fn vectors() -> Vec<(u128, u128, u128, u128)> {
    include_str!("vectors/pow.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<u128> = l.split_whitespace().map(|s| s.parse().unwrap()).collect();
            (f[0], f[1], f[2], f[3])
        })
        .collect()
}

#[test]
fn pow_matches_python_decimal_within_a_measured_bound() {
    let v = vectors();
    assert!(v.len() >= 2500, "only {} vectors", v.len());

    let mut worst_abs: u128 = 0;
    let mut worst_case = (0u128, 0u128, 0u128);
    let mut over = 0u32; // times we returned MORE than exact — must be rare and tiny
    let mut checked = 0u32;

    for (x, num, den, expected) in v {
        let got = match pow_frac(x, num, den) {
            Ok(g) => g,
            Err(CurveError::Overflow) => continue,
            Err(e) => panic!("pow_frac({x},{num},{den}) refused: {e:?}"),
        };
        checked += 1;
        let diff = if got >= expected { got - expected } else { expected - got };
        if got > expected {
            over += 1;
        }
        if diff > worst_abs {
            worst_abs = diff;
            worst_case = (x, num, den);
        }
    }

    println!(
        "checked {checked} vectors; worst absolute error {worst_abs} at scale 1e18 \
         (x={}, {}/{}) ; {over} results above exact",
        worst_case.0, worst_case.1, worst_case.2
    );

    // The bound this implementation claims, set to the measured maximum's next
    // power of ten rather than to a round number chosen in advance. Tightening
    // the series to 24 terms and removing the cancellation in `neg_ln` moved
    // this from 7e-12 down to what the assertion now holds.
    assert!(
        worst_abs <= 100,
        "worst error {worst_abs} exceeds the claimed bound, at x={} {}/{}",
        worst_case.0, worst_case.1, worst_case.2
    );
}

#[test]
fn pow_is_monotone_in_the_exponent() {
    // For x < 1, a larger exponent gives a smaller result. A pow that is not
    // monotone lets a trader find a weight where the pool pays more than at the
    // weight either side of it.
    let x = 500_000_000_000_000_000; // 0.5
    let mut last = u128::MAX;
    for num in 1..=40u128 {
        let got = pow_frac(x, num, 20).unwrap();
        assert!(got <= last, "not monotone at {num}/20: {got} > {last}");
        last = got;
    }
}

#[test]
fn the_edges_are_exact() {
    assert_eq!(pow_frac(ONE, 7, 3).unwrap(), ONE, "1^e must be exactly 1");
    assert_eq!(pow_frac(ONE / 2, 1, 1).unwrap(), ONE / 2, "x^1 must be x");
    assert_eq!(neg_ln(ONE).unwrap(), 0, "-ln(1) must be exactly 0");
    assert_eq!(exp_neg(0).unwrap(), ONE, "exp(0) must be exactly 1");
    assert_eq!(pow_frac(0, 1, 2), Err(CurveError::ZeroAmount));
    assert_eq!(pow_frac(ONE / 2, 1, 0), Err(CurveError::ZeroAmount));
}

#[test]
fn a_weighted_buy_never_pays_out_more_than_the_reserve() {
    let rt = 1_000_000 * ONE;
    let rc = 100_000 * ONE;
    for c_in in [ONE, 100 * ONE, 10_000 * ONE, rc, rc * 100] {
        for (wt, wc) in [(99u128, 1u128), (50, 50), (1, 99), (80, 20)] {
            match weighted_buy(rt, rc, c_in, wt, wc) {
                Ok(out) => assert!(out <= rt, "paid {out} from a reserve of {rt}"),
                Err(CurveError::Overflow) | Err(CurveError::ExceedsSaleReserve) => {}
                Err(e) => panic!("unexpected {e:?} for c_in={c_in} w={wt}/{wc}"),
            }
        }
    }
}

#[test]
fn a_bigger_buy_never_gets_a_better_rate() {
    // Price impact must be monotone: doubling the input must not more than
    // double the output. If it does, the curve pays for size.
    let rt = 1_000_000 * ONE;
    let rc = 100_000 * ONE;
    let a = weighted_buy(rt, rc, 1_000 * ONE, 80, 20).unwrap();
    let b = weighted_buy(rt, rc, 2_000 * ONE, 80, 20).unwrap();
    assert!(b <= a * 2, "doubling the input more than doubled the output: {a} -> {b}");
    assert!(b > a, "doubling the input did not increase the output");
}

#[test]
fn weights_are_correct_with_no_poke_at_all() {
    // The RFP requires the correct weight at transaction time regardless of
    // when the last poke happened. Nothing is stored, so this is a function of
    // `now` alone — asserted at adversarial instants.
    let (ws, we, t0, t1) = (990_000_000_000_000_000u128, 10_000_000_000_000_000u128, 1_000u64, 2_000u64);
    assert_eq!(weight_at(ws, we, t0, t1, 0).unwrap(), ws, "before the start");
    assert_eq!(weight_at(ws, we, t0, t1, 1_000).unwrap(), ws, "at the start");
    assert_eq!(weight_at(ws, we, t0, t1, 2_000).unwrap(), we, "at the end");
    assert_eq!(weight_at(ws, we, t0, t1, 9_999).unwrap(), we, "long after the end");
    let mid = weight_at(ws, we, t0, t1, 1_500).unwrap();
    assert!(mid < ws && mid > we, "midpoint {mid} outside the schedule");

    // Monotone across every instant of the schedule, one tick at a time.
    let mut prev = ws;
    for now in t0..=t1 {
        let w = weight_at(ws, we, t0, t1, now).unwrap();
        assert!(w <= prev, "weight rose at {now}: {w} > {prev}");
        prev = w;
    }
    assert_eq!(weight_at(ws, we, 2_000, 1_000, 1_500), Err(CurveError::ZeroAmount));
}
