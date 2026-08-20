// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every assertion here corresponds to a claim made about the vesting math.
//! If a claim is not testable it should not be made.

use antumbra::vesting::{Kind, Schedule, MAX_MILESTONES};
use antumbra::CurveError;

/// A schedule whose span does not divide its total, so every linear step has a
/// residue and exactness is not free.
fn awkward() -> Schedule {
    Schedule::linear(1_000, 1_000 + 7_919, 1_000_000_000_000_000_000_007).unwrap()
}

#[test]
fn claims_over_a_fully_elapsed_schedule_sum_to_the_total_exactly() {
    // Claim at every tick. Rounding down at each step must not strand dust:
    // by the end the beneficiary has every unit, not total-minus-epsilon.
    for span in [3u64, 7, 100, 7_919] {
        for total in [
            1u128,
            2,
            999,
            1_000_000_000_000_000_000_007,
            u64::MAX as u128,
        ] {
            let mut s = Schedule::linear(1_000, 1_000 + span, total).unwrap();
            let mut paid: u128 = 0;
            for t in 1_000..=(1_000 + span) {
                if s.claimable_at(t) > 0 {
                    paid += s.claim(t).unwrap();
                }
            }
            assert_eq!(
                paid, total,
                "span {span}, total {total}: claims summed to {paid}"
            );
            assert!(s.is_fully_claimed());
        }
    }
}

#[test]
fn no_prefix_of_claims_ever_exceeds_what_is_vested() {
    let mut s = awkward();
    let mut paid: u128 = 0;
    for t in 900..(1_000 + 7_919 + 50) {
        if s.claimable_at(t) > 0 {
            paid += s.claim(t).unwrap();
        }
        assert!(
            paid <= s.vested_at(t),
            "at t={t} paid {paid} exceeds vested {}",
            s.vested_at(t)
        );
    }
}

#[test]
fn vesting_never_goes_backwards() {
    let s = awkward();
    let mut prev = 0u128;
    for t in 900..(1_000 + 7_919 + 50) {
        let v = s.vested_at(t);
        assert!(v >= prev, "vested fell at t={t}: {prev} -> {v}");
        prev = v;
    }
    assert_eq!(prev, s.total);
}

#[test]
fn a_cliff_pays_nothing_before_it_and_everything_by_the_end() {
    let s = Schedule::cliff_linear(0, 365, 365 + 1_095, 1_000_000).unwrap();
    for t in 0..365 {
        assert_eq!(s.vested_at(t), 0, "paid before the cliff at t={t}");
    }
    assert_eq!(s.vested_at(365), 0);
    assert!(s.vested_at(366) > 0);
    assert_eq!(s.vested_at(365 + 1_095), 1_000_000);
    assert_eq!(s.vested_at(u64::MAX), 1_000_000);
}

#[test]
fn the_three_parts_of_a_cancellation_always_sum_to_the_total() {
    // Swept across every cancellation instant, and across whether the
    // beneficiary had claimed beforehand — which is what makes the split
    // three-way rather than two.
    for claim_first_at in [None, Some(1_500u64), Some(5_000), Some(8_900)] {
        for cancel_at in 900..(1_000 + 7_919 + 20) {
            let mut s = awkward();
            let total = s.total;
            if let Some(c) = claim_first_at {
                if c < cancel_at && s.claimable_at(c) > 0 {
                    s.claim(c).unwrap();
                }
            }
            let split = s.cancel(cancel_at).unwrap();
            assert_eq!(
                split.sum(),
                total,
                "cancel at {cancel_at} after claim {claim_first_at:?}: {split:?}"
            );
        }
    }
}

#[test]
fn cancelling_freezes_accrual_rather_than_clawing_back_what_vested() {
    let mut s = awkward();
    let at = 5_000u64;
    let vested_then = s.vested_at(at);
    let split = s.cancel(at).unwrap();
    assert_eq!(split.to_beneficiary, vested_then);
    // Time keeps moving; the schedule does not.
    for t in at..(at + 10_000) {
        assert_eq!(s.vested_at(t), vested_then, "accrued after cancellation");
    }
    // And what had vested is still claimable afterwards, as the RFP requires.
    assert_eq!(s.claim(at + 10_000).unwrap(), vested_then);
}

#[test]
fn a_non_cancelable_schedule_refuses_and_the_conversion_is_one_way() {
    let mut s = awkward();
    s.make_non_cancelable().unwrap();
    assert_eq!(s.cancel(5_000), Err(CurveError::ZeroAmount));
    // Converting twice is refused, and there is no inverse anywhere in the API.
    assert!(s.make_non_cancelable().is_err());
}

#[test]
fn signalling_a_milestone_twice_is_rejected_and_unlocks_nothing_extra() {
    let mut s = Schedule::milestone(vec![10, 20, 30, 40]).unwrap();
    assert_eq!(s.total, 100);
    assert_eq!(s.vested_at(0), 0);

    assert_eq!(s.signal_milestone(2).unwrap(), 30);
    assert_eq!(s.vested_at(0), 30);

    // The second signal is refused deterministically...
    let again = s.signal_milestone(2);
    assert_eq!(again, Err(CurveError::SlippageExceeded));
    // ...and, the part that matters, nothing was unlocked by the attempt.
    assert_eq!(s.vested_at(0), 30);

    assert_eq!(s.signal_milestone(0).unwrap(), 10);
    assert_eq!(s.vested_at(0), 40);
    assert_eq!(s.signal_milestone(1).unwrap(), 20);
    assert_eq!(s.signal_milestone(3).unwrap(), 40);
    assert_eq!(s.vested_at(0), 100);
    assert_eq!(s.claim(0).unwrap(), 100);
    assert!(s.is_fully_claimed());
}

#[test]
fn a_milestone_index_past_the_end_is_refused_rather_than_wrapping() {
    let mut s = Schedule::milestone(vec![1, 2, 3]).unwrap();
    assert_eq!(s.signal_milestone(3), Err(CurveError::ExceedsSaleReserve));
    assert_eq!(s.signal_milestone(63), Err(CurveError::ExceedsSaleReserve));
    // The shift `1u64 << index` would be undefined at 64; refused before that.
    assert_eq!(
        s.signal_milestone(MAX_MILESTONES),
        Err(CurveError::ExceedsSaleReserve)
    );
    assert_eq!(s.vested_at(0), 0);
}

#[test]
fn milestone_totals_are_checked_at_creation_not_trusted() {
    assert!(Schedule::milestone(vec![]).is_err());
    assert!(Schedule::milestone(vec![1, 0, 3]).is_err(), "zero tranche");
    assert!(
        Schedule::milestone(vec![u128::MAX, 1]).is_err(),
        "overflowing total must refuse, not wrap"
    );
    assert!(Schedule::milestone(vec![1; 65]).is_err(), "past the bitmap");
    assert!(Schedule::milestone(vec![1; 64]).is_ok());
}

#[test]
fn degenerate_schedules_are_refused_at_construction() {
    assert!(Schedule::linear(10, 10, 100).is_err(), "zero span");
    assert!(Schedule::linear(11, 10, 100).is_err(), "inverted");
    assert!(Schedule::linear(0, 10, 0).is_err(), "zero total");
    assert!(
        Schedule::cliff_linear(0, 10, 10, 100).is_err(),
        "cliff == end"
    );
    assert!(
        Schedule::cliff_linear(0, 11, 10, 100).is_err(),
        "cliff > end"
    );
    assert!(
        Schedule::cliff_linear(5, 4, 10, 100).is_err(),
        "cliff < start"
    );
    assert!(
        Schedule::cliff_linear(0, 0, 10, 100).is_ok(),
        "cliff at start"
    );
}

#[test]
fn claiming_nothing_is_an_error_rather_than_a_silent_no_op() {
    // U7's "nothing is claimable yet" has to be distinguishable from a claim
    // that paid zero, or the UI cannot tell the user when to come back.
    let mut s = Schedule::cliff_linear(0, 365, 1_000, 1_000).unwrap();
    assert_eq!(s.claim(0), Err(CurveError::ZeroAmount));
    assert_eq!(s.claim(364), Err(CurveError::ZeroAmount));
    assert!(s.claim(500).is_ok());
    // Immediately claiming again in the same instant is also nothing.
    assert_eq!(s.claim(500), Err(CurveError::ZeroAmount));
}

#[test]
fn rounding_down_means_the_residue_waits_in_escrow_rather_than_paying_early() {
    // 10 units over 3 ticks: 3, 3, then 4. Never 4, 4, 4.
    let mut s = Schedule::linear(0, 3, 10).unwrap();
    assert_eq!(s.vested_at(1), 3);
    assert_eq!(s.vested_at(2), 6);
    assert_eq!(s.vested_at(3), 10);
    let mut paid = 0;
    for t in 1..=3 {
        paid += s.claim(t).unwrap();
        assert!(paid <= s.vested_at(t));
    }
    assert_eq!(paid, 10);
}

#[test]
fn the_kind_decides_whether_time_matters_at_all() {
    let s = Schedule::milestone(vec![5, 5]).unwrap();
    assert_eq!(s.kind, Kind::Milestone);
    // A milestone schedule must not accrue with the clock, at any timestamp.
    for t in [0u64, 1, u64::MAX / 2, u64::MAX] {
        assert_eq!(s.vested_at(t), 0);
    }
}
