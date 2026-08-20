// Measures the zkVM cycle cost of every priced operation, inside the guest.
//
// The fixtures are compiled in rather than read from the host so the run is
// reproducible from the binary alone: the same guest ELF always measures the
// same work. Each op is measured as a `cycle_count()` delta with the
// measurement's own overhead subtracted, and `black_box` keeps the optimiser
// from hoisting a pure function out of the region being timed.

use core::hint::black_box;
use risc0_zkvm::guest::env;

use antumbra::weighted::{exp_neg, neg_ln, pow_frac, weight_at, weighted_buy, ONE};
use antumbra::binfixed;
use antumbra::fees::{buy_fee, close_fee, FeeConfig, CAP_AT_CLOSE, CAP_PER_SWAP};
use antumbra::vesting::Schedule;
use antumbra::Curve;

/// `x` at 1e18, then the weight ratio `num/den`. Spans the range an LBP
/// actually sweeps: 99/1 down to 1/99, and `x` from just under one to 1e-18.
const POW_CASES: [(u128, u128, u128); 12] = [
    (999_999_999_999_999_999, 1, 99),
    (999_747_746_159_161_249, 68, 1),
    (999_167_476_763_127_780, 74, 3),
    (900_000_000_000_000_000, 1, 1),
    (500_000_000_000_000_000, 1, 1),
    (500_000_000_000_000_000, 99, 1),
    (500_000_000_000_000_000, 1, 99),
    (250_000_000_000_000_000, 50, 50),
    (100_000_000_000_000_000, 20, 80),
    (1_000_000_000_000, 80, 20),
    (1_000_000, 1, 99),
    (1, 99, 1),
];

const BUY_CASES: [(u128, u128, u128, u128, u128); 6] = [
    // reserve_token, reserve_collateral, c_in, w_token, w_collateral
    (1_000_000 * ONE, 100_000 * ONE, ONE, 99, 1),
    (1_000_000 * ONE, 100_000 * ONE, 1_000 * ONE, 50, 50),
    (1_000_000 * ONE, 100_000 * ONE, 50_000 * ONE, 1, 99),
    (1_000_000 * ONE, 100_000 * ONE, ONE / 1_000_000, 80, 20),
    (500_000 * ONE, 250_000 * ONE, 10_000 * ONE, 60, 40),
    (10 * ONE, 10 * ONE, ONE, 1, 1),
];

/// One measured region. Returns the net cycles for `f`.
fn timed<T>(baseline: u64, mut f: impl FnMut() -> T) -> u64 {
    let a = env::cycle_count();
    black_box(f());
    let b = env::cycle_count();
    (b - a).saturating_sub(baseline)
}

fn stats(v: &[u64]) -> (u64, u64, u64) {
    let mut s = v.to_vec();
    s.sort_unstable();
    (s[s.len() / 2], s[0], s[s.len() - 1])
}

fn main() {
    // Cost of the measurement itself, so every figure below is net.
    let baseline = {
        let a = env::cycle_count();
        black_box(0u128);
        let b = env::cycle_count();
        b - a
    };

    let mut out: Vec<(&str, u64, u64, u64, usize)> = Vec::new();

    let v: Vec<u64> = POW_CASES
        .iter()
        .map(|&(x, n, d)| timed(baseline, || pow_frac(black_box(x), black_box(n), black_box(d))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("pow_frac", m, lo, hi, v.len()));

    let v: Vec<u64> = POW_CASES
        .iter()
        .map(|&(x, n, d)| {
            timed(baseline, || {
                binfixed::pow_frac(black_box(x), black_box(n), black_box(d))
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("pow_frac_binary", m, lo, hi, v.len()));

    let v: Vec<u64> = BUY_CASES
        .iter()
        .map(|&(rt, rc, c, wt, wc)| {
            timed(baseline, || {
                binfixed::weighted_buy(
                    black_box(rt),
                    black_box(rc),
                    black_box(c),
                    black_box(wt),
                    black_box(wc),
                )
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("weighted_buy_binary", m, lo, hi, v.len()));

    let v: Vec<u64> = POW_CASES
        .iter()
        .map(|&(x, _, _)| timed(baseline, || neg_ln(black_box(x))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("neg_ln", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u128)
        .map(|i| timed(baseline, || exp_neg(black_box(i * ONE / 4))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("exp_neg", m, lo, hi, v.len()));

    let v: Vec<u64> = BUY_CASES
        .iter()
        .map(|&(rt, rc, c, wt, wc)| {
            timed(baseline, || {
                weighted_buy(
                    black_box(rt),
                    black_box(rc),
                    black_box(c),
                    black_box(wt),
                    black_box(wc),
                )
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("weighted_buy", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u64)
        .map(|i| {
            timed(baseline, || {
                weight_at(black_box(99), black_box(1), 0, 1_000, black_box(i * 83))
            })
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("weight_at", m, lo, hi, v.len()));

    // Constant-product side, for RFP-015.
    let v: Vec<u64> = (1..13u128)
        .map(|i| {
            let mut c = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
            timed(baseline, || c.buy(black_box(i * ONE), 0))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("curve_buy", m, lo, hi, v.len()));

    let v: Vec<u64> = (1..13u128)
        .map(|i| {
            let mut c = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
            c.buy(1_000 * ONE, 0).unwrap();
            timed(baseline, || c.sell(black_box(i * ONE), 0))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("curve_sell", m, lo, hi, v.len()));

    // Vesting, for RFP-017's per-operation cost table.
    let v: Vec<u64> = (0..12u64)
        .map(|i| {
            let s = Schedule::linear(1_000, 1_000 + 7_919, 1_000_000 * ONE).unwrap();
            timed(baseline, || s.vested_at(black_box(1_000 + i * 700)))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("vested_at_linear", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u64)
        .map(|i| {
            let s = Schedule::cliff_linear(0, 365, 365 + 1_095, 1_000_000 * ONE).unwrap();
            timed(baseline, || s.vested_at(black_box(i * 140)))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("vested_at_cliff", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u64)
        .map(|i| {
            let mut s = Schedule::linear(1_000, 1_000 + 7_919, 1_000_000 * ONE).unwrap();
            timed(baseline, || s.claim(black_box(1_000 + i * 700)))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("vesting_claim", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u64)
        .map(|i| {
            let mut s = Schedule::linear(1_000, 1_000 + 7_919, 1_000_000 * ONE).unwrap();
            timed(baseline, || s.cancel(black_box(1_000 + i * 700)))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("vesting_cancel", m, lo, hi, v.len()));

    let v: Vec<u64> = (0..12u32)
        .map(|i| {
            let mut s = Schedule::milestone(vec![ONE; 12]).unwrap();
            timed(baseline, || s.signal_milestone(black_box(i)))
        })
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("signal_milestone", m, lo, hi, v.len()));

    // Fees, for the per-operation tables both launchpad RFPs ask for.
    let cfg = FeeConfig::new(10_000, CAP_PER_SWAP).unwrap();
    let v: Vec<u64> = (1..13u128)
        .map(|i| timed(baseline, || buy_fee(&cfg, black_box(i * ONE))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("buy_fee", m, lo, hi, v.len()));

    let cfg2 = FeeConfig::new(50_000, CAP_AT_CLOSE).unwrap();
    let v: Vec<u64> = (1..13u128)
        .map(|i| timed(baseline, || close_fee(&cfg2, black_box(i * 1_000 * ONE))))
        .collect();
    let (m, lo, hi) = stats(&v);
    out.push(("close_fee", m, lo, hi, v.len()));

    // A whole buy, start to finish, as the program would run it.
    let total = {
        let a = env::cycle_count();
        let mut c = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
        black_box(c.buy(black_box(500 * ONE), 0)).ok();
        env::cycle_count() - a
    };

    env::commit(&(out, baseline, total));
}
