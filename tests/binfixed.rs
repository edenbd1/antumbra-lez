// SPDX-License-Identifier: MIT OR Apache-2.0
//! The binary-scale kernel against the same oracle as the decimal one, so the
//! two are comparable rather than merely both "tested". Same vectors, same
//! bound, same rounding-direction question.

use antumbra_curve::binfixed;
use antumbra_curve::weighted;
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
fn the_binary_kernel_matches_python_decimal_at_least_as_well_as_the_decimal_one() {
    let v = vectors();
    assert!(v.len() >= 2500, "only {} vectors", v.len());

    let mut worst_bin: u128 = 0;
    let mut worst_dec: u128 = 0;
    let mut arg_bin = (0u128, 0u128, 0u128);
    let mut above_bin = 0usize;

    for &(x, num, den, want) in &v {
        let got_b = binfixed::pow_frac(x, num, den).unwrap();
        let got_d = weighted::pow_frac(x, num, den).unwrap();

        let e_b = got_b.abs_diff(want);
        let e_d = got_d.abs_diff(want);
        if e_b > worst_bin {
            worst_bin = e_b;
            arg_bin = (x, num, den);
        }
        worst_dec = worst_dec.max(e_d);
        if got_b > want {
            above_bin += 1;
        }
    }

    println!(
        "  binaire : pire erreur {worst_bin} (x={}, {}/{}) ; {above_bin} au-dessus de l'exact",
        arg_bin.0, arg_bin.1, arg_bin.2
    );
    println!("  décimal : pire erreur {worst_dec}");

    // The bound the decimal kernel already meets. Beating it is the point of
    // the rewrite; failing it would mean the rewrite bought speed with accuracy.
    assert!(
        worst_bin <= 100,
        "binary kernel worst error {worst_bin} exceeds the decimal kernel's own bound of 100"
    );
    assert!(
        worst_bin <= worst_dec,
        "the rewrite lost accuracy: binary {worst_bin} vs decimal {worst_dec}"
    );
}

#[test]
fn the_two_kernels_agree_with_each_other_across_the_whole_sweep() {
    // Not just "both near the oracle" — near each other, which catches a
    // systematic offset that a loose bound would hide.
    let mut worst: u128 = 0;
    for &(x, num, den, _) in &vectors() {
        let a = binfixed::pow_frac(x, num, den).unwrap();
        let b = weighted::pow_frac(x, num, den).unwrap();
        worst = worst.max(a.abs_diff(b));
    }
    println!("  écart maximal entre les deux noyaux : {worst}");
    assert!(worst <= 200, "kernels disagree by {worst}");
}

#[test]
fn the_edges_are_exact() {
    let one = 1_000_000_000_000_000_000u128;
    assert_eq!(binfixed::pow_frac(one, 3, 7).unwrap(), one, "1^e must be 1");
    assert_eq!(binfixed::pow_frac(0, 3, 7).unwrap(), 0, "0^e must be 0");
    // x^(n/n) is x exactly, which is the case an LBP hits whenever its
    // schedule crosses 50/50.
    for x in [one, one / 2, one / 3, 1, 999_999_999_999_999_999] {
        for n in [1u128, 7, 50, 99] {
            assert_eq!(binfixed::pow_frac(x, n, n).unwrap(), x, "x^(n/n) at x={x}");
        }
    }
}

#[test]
fn it_is_monotone_in_the_exponent() {
    // A larger exponent on a base below one gives a smaller result. If this
    // fails, some weight in the schedule pays better than its neighbours.
    for x in [
        999_999_999_999_999_999u128,
        900_000_000_000_000_000,
        500_000_000_000_000_000,
        1_000_000,
    ] {
        let mut prev = u128::MAX;
        for n in 1..=99u128 {
            let got = binfixed::pow_frac(x, n, 1).unwrap();
            assert!(got <= prev, "not monotone at x={x}, n={n}: {got} > {prev}");
            prev = got;
        }
    }
}

#[test]
fn degenerate_arguments_are_refused_by_name() {
    let one = 1_000_000_000_000_000_000u128;
    assert_eq!(binfixed::pow_frac(one, 1, 0), Err(CurveError::ZeroAmount));
    assert_eq!(binfixed::pow_frac(one, 0, 1), Err(CurveError::ZeroAmount));
    assert_eq!(
        binfixed::pow_frac(one + 1, 1, 2),
        Err(CurveError::QuantityAtOrAboveReserve)
    );
}

#[test]
fn the_logarithm_is_signed_across_the_reduction_window_without_losing_zero() {
    // ln(1) = 0 exactly, and the reduction window straddles 1, so this is the
    // case where a sign slip would show up as a non-zero answer.
    let one62 = 1i128 << 62;
    assert_eq!(binfixed::neg_ln_62(one62).unwrap(), 0);
    assert_eq!(binfixed::exp_neg_62(0).unwrap(), one62);
    // Round trip across the window edges.
    for x62 in [one62, one62 - 1, one62 / 2, one62 / 3, (one62 * 7) / 10, 1] {
        let nl = binfixed::neg_ln_62(x62).unwrap();
        assert!(nl >= 0, "-ln x went negative at x62={x62}");
        let back = binfixed::exp_neg_62(nl).unwrap();
        let d = (back - x62).unsigned_abs();
        assert!(d <= 4_000, "round trip drifted by {d} at x62={x62}");
    }
}
