// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fee properties, stated as the proposals state them.

use antumbra::fees::*;
use antumbra::{Curve, CurveError};

const ONE: u128 = 1_000_000_000_000_000_000;

#[test]
fn a_zero_rate_takes_nothing_at_any_size() {
    let cfg = FeeConfig::zero(CAP_PER_SWAP).unwrap();
    for amount in [1u128, 7, ONE, u128::MAX / 2, u128::MAX] {
        assert_eq!(cfg.fee_on(amount).unwrap(), 0, "at {amount}");
    }
    let (fee, eff) = buy_fee(&cfg, 1_000 * ONE).unwrap();
    assert_eq!(fee, 0);
    assert_eq!(eff, 1_000 * ONE, "a zero rate must not perturb the input");
}

#[test]
fn the_fee_always_rounds_against_the_payer() {
    // 1 unit at 0.01% is 0.0001 units. Rounding down gives the trader a free
    // trade; rounding up gives the pool the residue, which is the rule.
    let cfg = FeeConfig::new(100, CAP_PER_SWAP).unwrap(); // 0.01%
    assert_eq!(cfg.fee_on(1).unwrap(), 1, "a dust trade still pays");
    assert_eq!(cfg.fee_on(9_999).unwrap(), 1);
    assert_eq!(
        cfg.fee_on(10_000).unwrap(),
        1,
        "exactly one unit, no rounding"
    );
    assert_eq!(cfg.fee_on(10_001).unwrap(), 2, "one unit over rounds up");
}

#[test]
fn a_rate_above_the_cap_is_refused_rather_than_clamped() {
    // Clamping would hide a misconfiguration that somebody should be shown.
    assert_eq!(
        FeeConfig::new(CAP_PER_SWAP + 1, CAP_PER_SWAP),
        Err(CurveError::ExceedsRealCollateral)
    );
    assert!(FeeConfig::new(CAP_PER_SWAP, CAP_PER_SWAP).is_ok());
    assert!(FeeConfig::new(CAP_AT_CLOSE, CAP_AT_CLOSE).is_ok());
    // And a cap above 100% is not a cap.
    assert!(FeeConfig::new(0, RATE_ONE + 1).is_err());
}

#[test]
fn the_switch_can_move_the_rate_but_never_the_ceiling() {
    let mut cfg = FeeConfig::zero(CAP_PER_SWAP).unwrap();
    assert_eq!(cfg.rate(), 0);
    cfg.set_rate(5_000).unwrap(); // 0.5%
    assert_eq!(cfg.rate(), 5_000);
    // The whole point of compiling the cap in: governance cannot exceed it.
    assert_eq!(
        cfg.set_rate(CAP_PER_SWAP + 1),
        Err(CurveError::ExceedsRealCollateral)
    );
    assert_eq!(
        cfg.rate(),
        5_000,
        "a refused change must not partially apply"
    );
    assert_eq!(cfg.cap(), CAP_PER_SWAP, "the cap is not writable");
}

#[test]
fn a_buy_fee_is_taken_before_pricing_and_a_sell_fee_after() {
    let cfg = FeeConfig::new(10_000, CAP_PER_SWAP).unwrap(); // 1%
    let (fee, effective) = buy_fee(&cfg, 1_000).unwrap();
    assert_eq!(fee, 10);
    assert_eq!(effective, 990, "the curve must price 990, not 1000");

    let (fee, paid) = sell_fee(&cfg, 1_000).unwrap();
    assert_eq!(fee, 10);
    assert_eq!(paid, 990, "the seller receives the raw output less the fee");
}

#[test]
fn taking_the_buy_fee_before_pricing_gives_the_pool_less_not_more() {
    // The ordering matters economically, so it is asserted rather than
    // commented: pricing the full input would credit the curve with collateral
    // the treasury takes, inflating it by the fee on every single trade.
    let cfg = FeeConfig::new(10_000, CAP_PER_SWAP).unwrap();
    let c_in = 1_000 * ONE;

    let mut correct = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
    let (_, effective) = buy_fee(&cfg, c_in).unwrap();
    let out_correct = correct.buy(effective, 0).unwrap();

    let mut wrong = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
    let out_wrong = wrong.buy(c_in, 0).unwrap();

    assert!(
        out_correct < out_wrong,
        "fee-before-pricing must buy fewer tokens than fee-after"
    );
    assert!(
        correct.real_collateral < wrong.real_collateral,
        "the curve must not be credited with collateral the treasury took"
    );
}

#[test]
fn a_trade_consumed_entirely_by_fee_is_refused() {
    // Only reachable at a 100% rate, which the caps forbid — so this asserts
    // the guard rather than a live path, and it is here so that a future cap
    // change cannot quietly create a trade that buys nothing.
    let cfg = FeeConfig::new(RATE_ONE, RATE_ONE).unwrap();
    assert_eq!(buy_fee(&cfg, 100), Err(CurveError::ZeroAmount));
    assert_eq!(cfg.fee_on(100).unwrap(), 100);
}

#[test]
fn a_fee_can_never_exceed_the_amount_it_is_taken_from() {
    for rate in [1u128, 100, 10_000, 50_000, RATE_ONE] {
        let cfg = FeeConfig::new(rate, RATE_ONE).unwrap();
        for amount in [1u128, 2, 999, ONE, u128::MAX / RATE_ONE] {
            let fee = cfg.fee_on(amount).unwrap();
            assert!(fee <= amount, "rate {rate}, amount {amount}, fee {fee}");
        }
    }
}

#[test]
fn the_close_fee_leaves_the_creator_the_remainder_exactly() {
    let cfg = FeeConfig::new(50_000, CAP_AT_CLOSE).unwrap(); // 5%, the Fjord rate
    for raised in [1u128, 3, 999, 20 * ONE, 123_456_789 * ONE] {
        let (fee, paid) = close_fee(&cfg, raised).unwrap();
        assert_eq!(fee + paid, raised, "the split must be exact at {raised}");
        assert!(
            fee * 20 >= raised,
            "5% rounded up covers at least a twentieth"
        );
    }
}

#[test]
fn a_huge_amount_refuses_rather_than_wrapping() {
    // mul_div_ceil takes the product in 256 bits, so this is a real answer and
    // not an overflow — the assertion is that it stays a real answer.
    let cfg = FeeConfig::new(10_000, CAP_PER_SWAP).unwrap();
    let fee = cfg.fee_on(u128::MAX).unwrap();
    assert!(
        fee < u128::MAX / 50,
        "1% of the maximum is about a hundredth"
    );
    assert!(fee > u128::MAX / 200);
}

#[test]
fn a_creation_fee_is_charged_on_top_of_the_escrow_never_out_of_it() {
    let cfg = FeeConfig::new(10_000, CAP_AT_CREATION).unwrap(); // 1%
    let total = 1_000 * ONE;
    let (fee, debited) = creation_fee(&cfg, total).unwrap();
    assert_eq!(fee, 10 * ONE);
    assert_eq!(
        debited,
        total + fee,
        "the creator pays total + fee; the escrow must still hold the full total"
    );
    // Taking it out of the escrow would promise the beneficiary `total` while
    // holding less, and the shortfall would surface years later at the final
    // claim as an unexplained failure.
    assert!(debited > total);
}

#[test]
fn a_zero_creation_fee_debits_exactly_the_schedule_total() {
    let cfg = FeeConfig::zero(CAP_AT_CREATION).unwrap();
    for total in [1u128, 999, ONE, 123_456 * ONE] {
        let (fee, debited) = creation_fee(&cfg, total).unwrap();
        assert_eq!(fee, 0);
        assert_eq!(debited, total);
    }
    assert_eq!(creation_fee(&cfg, 0), Err(CurveError::ZeroAmount));
}

#[test]
fn a_raise_of_one_unit_pays_its_whole_self_in_fee_at_any_rate() {
    // Found on the public testnet, not by reading the code: a pool that raised
    // one unit at 5% paid the entire unit as fee and the creator received zero.
    //
    // The first explanation written down was wrong — it claimed the raise is
    // consumed below `1/rate`, which at 5% would be 20 units. It is not. The
    // creator receives nothing exactly when `ceil(a·r/ONE) == a`, which solves
    // to `a < ONE / (ONE − r)`. At any rate a sane protocol would charge that
    // is only `a = 1`.
    //
    // The arithmetic is right and rounding the other way would be worse: a
    // one-unit raise would pay no fee at all, and the same leniency at scale is
    // what makes a protocol insolvent.
    for rate in [1u128, 100, 10_000, 50_000] {
        let cfg = FeeConfig::new(rate, RATE_ONE).unwrap();
        let (fee, to_creator) = close_fee(&cfg, 1).unwrap();
        assert_eq!(fee, 1, "a one-unit raise pays one unit at rate {rate}");
        assert_eq!(
            to_creator, 0,
            "so the creator receives nothing at rate {rate}"
        );
    }
}

#[test]
fn the_threshold_is_one_over_one_minus_the_rate_not_one_over_the_rate() {
    // The exact boundary, asserted because the intuitive formula is wrong and
    // a UI that quotes the wrong one misinforms every creator who reads it.
    for rate in [50_000u128, 500_000, 900_000] {
        let cfg = FeeConfig::new(rate, RATE_ONE).unwrap();
        let last_consumed = RATE_ONE.div_ceil(RATE_ONE - rate) - 1;

        let (_, at) = close_fee(&cfg, last_consumed).unwrap();
        assert_eq!(
            at, 0,
            "at {last_consumed} the fee still takes everything (rate {rate})"
        );

        let (_, above) = close_fee(&cfg, last_consumed + 1).unwrap();
        assert!(
            above > 0,
            "at {} the creator must receive something (rate {rate})",
            last_consumed + 1
        );
    }
}

#[test]
fn the_effective_rate_is_worst_at_the_smallest_raises() {
    // The nominal rate is a ceiling on large raises and a floor on small ones.
    // This is the property a minimum-raise rule exists to bound, and it is the
    // honest way to state what the testnet run showed.
    let cfg = FeeConfig::new(50_000, RATE_ONE).unwrap(); // 5%
    let mut previous = u128::MAX;
    for raise in [1u128, 2, 5, 10, 100, 1_000, 1_000_000] {
        let fee = cfg.fee_on(raise).unwrap();
        // effective rate in millionths, rounded down
        let effective = fee * RATE_ONE / raise;
        assert!(
            effective <= previous,
            "effective rate must not rise with the raise: {raise} gave {effective}"
        );
        assert!(
            effective >= 50_000 - 1,
            "and never falls below the nominal rate"
        );
        previous = effective;
    }
    assert_eq!(
        cfg.fee_on(1).unwrap() * RATE_ONE,
        RATE_ONE,
        "at a raise of 1 it is 100%"
    );
}
