//! The committed antumbra_vesting binary, executed as the sequencer executes it.
//! Each test names the RFP-017 requirement it is evidence for.
//!
//!     cargo test --release --manifest-path executor-tests/Cargo.toml

use antumbra_executor_tests::*;
use lee_core::program::ProgramId;

const T0: u64 = 1_800_000_000_000; // ms
const MIN: u64 = 60_000;

fn refused(r: Result<Run, String>, want: u32) {
    match r {
        Ok(_) => panic!("accepted; expected refusal {want}"),
        Err(e) => assert_eq!(code(&e), Some(want), "wrong refusal: {e}"),
    }
}

// ---------------------------------------------------------------- time (F1)

#[test]
fn f1_claim_reads_the_clock_and_pays_exactly_the_linear_amount() {
    let mut w = World::new("linear");
    w.with(w.linear(T0, T0 + 30 * MIN, 600), 600);
    let r = w.claim(T0 + 7 * MIN + 333, native(DEST, 0), BENEFICIARY).unwrap();
    let want = 600u128 * u128::from(7 * MIN + 333) / u128::from(30 * MIN);
    assert_eq!(r.post(&w.holding).balance, 600 - want);
    assert_eq!(r.post(&native(DEST, 0)).balance, want);
    let s = r.schedule(&w.schedule);
    assert_eq!((s.claimed, s.last_seen), (want, T0 + 7 * MIN + 333));
}

#[test]
fn f1_a_clock_the_caller_owns_is_refused() {
    let mut w = World::new("fakeclock");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    // Right address, wrong owner: an account some other program wrote.
    let mut fake = clock(T0 + 10 * MIN);
    fake.account.program_owner = AUTH_TRANSFER;
    let r = run(&w.elf, &w.pid, &Ix::Claim { schedule_id: w.id }, vec![
        w.schedule.clone(), w.holding.clone(), native(DEST, 0),
        acc(BENEFICIARY, AUTH_TRANSFER, 0, vec![], true), fake,
    ]);
    refused(r, 7014);
}

#[test]
fn f1_an_account_the_clock_program_owns_but_is_not_the_clock_is_refused() {
    let mut w = World::new("otherclock");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    let mut other = clock(T0 + 10 * MIN);
    other.account_id = lee_core::account::AccountId::new(*b"/LEZ/ClockProgramAccount/0000010");
    let r = run(&w.elf, &w.pid, &Ix::Claim { schedule_id: w.id }, vec![
        w.schedule.clone(), w.holding.clone(), native(DEST, 0),
        acc(BENEFICIARY, AUTH_TRANSFER, 0, vec![], true), other,
    ]);
    refused(r, 7014);
}

#[test]
fn f1a_the_cliff_unlocks_its_lump_and_nothing_before_it() {
    let mut w = World::new("cliff");
    let mut s = w.linear(T0, T0 + 4 * 365 * MIN, 1_000_000);
    s.kind = 0;
    s.cliff = T0 + 365 * MIN;
    w.with(s, 1_000_000);
    refused(w.claim(T0 + 365 * MIN - 1, native(DEST, 0), BENEFICIARY), 7003);
    let r = w.claim(T0 + 365 * MIN, native(DEST, 0), BENEFICIARY).unwrap();
    assert_eq!(r.post(&native(DEST, 0)).balance, 250_000);
}

#[test]
fn f1c_milestones_release_one_tranche_per_signal_and_u7_nothing_before() {
    let mut w = World::new("ms");
    let mut s = w.linear(0, 1, 3);
    s.kind = 2;
    s.tranches = 3;
    w.with(s.clone(), 3);
    refused(w.claim(T0, native(DEST, 0), BENEFICIARY), 7003);
    s.signalled = 0b011;
    w.with(s, 3);
    let r = w.claim(T0, native(DEST, 0), BENEFICIARY).unwrap();
    assert_eq!(r.post(&native(DEST, 0)).balance, 2);
}

// ---------------------------------------------------------------- claim (F2, R1, P1)

#[test]
fn f2_only_the_named_beneficiary_can_claim() {
    let mut w = World::new("who");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    refused(w.claim(T0 + MIN, native(DEST, 0), STRANGER), 7004);
}

#[test]
fn f2_the_claim_pays_the_destination_the_beneficiary_names() {
    // The destination is not the signing key and need not be known to the
    // creator; on chain it is a shielded account on the private path.
    let mut w = World::new("dest");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    let r = w.claim(T0 + MIN, native(STRANGER, 5), BENEFICIARY).unwrap();
    assert_eq!(r.post(&native(STRANGER, 5)).balance, 15);
}

#[test]
fn r1_a_refused_claim_writes_nothing_and_a_retry_pays() {
    let mut w = World::new("retry");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    assert!(w.claim(T0 - 1, native(DEST, 0), BENEFICIARY).is_err()); // nothing vested
    let r = w.claim(T0 + MIN, native(DEST, 0), BENEFICIARY).unwrap();
    assert_eq!(r.post(&native(DEST, 0)).balance, 10);
    assert!(r.calls().is_empty(), "a native claim is one program, one transaction (P1)");
}

#[test]
fn a_claim_that_reads_an_older_clock_than_the_schedule_has_seen_is_refused() {
    let mut w = World::new("backwards");
    let mut s = w.linear(T0, T0 + 10 * MIN, 10);
    s.last_seen = T0 + 5 * MIN;
    s.claimed = 5;
    w.with(s, 5);
    refused(w.claim(T0 + 4 * MIN, native(DEST, 0), BENEFICIARY), 7005);
}

// ---------------------------------------------------------------- cancel (F3, R2)

#[test]
fn f3_cancel_returns_the_unvested_part_to_the_fixed_refund_account() {
    let mut w = World::new("cancel");
    w.with(w.linear(T0, T0 + 30 * MIN, 600), 600);
    let r = w.cancel(T0 + 10 * MIN, native(REFUND, 0), CREATOR).unwrap();
    assert_eq!(r.post(&native(REFUND, 0)).balance, 400);
    assert_eq!(r.post(&w.holding).balance, 200);
    assert_eq!(r.schedule(&w.schedule).cancelled_at, T0 + 10 * MIN);
}

#[test]
fn f3_the_vested_part_stays_claimable_after_cancellation_and_no_more() {
    let mut w = World::new("aftercancel");
    let mut s = w.linear(T0, T0 + 30 * MIN, 600);
    s.cancelled_at = T0 + 10 * MIN;
    w.with(s, 200);
    let r = w.claim(T0 + 29 * MIN, native(DEST, 0), BENEFICIARY).unwrap();
    assert_eq!(r.post(&native(DEST, 0)).balance, 200);
}

#[test]
fn f3_a_cancel_cannot_be_redirected_to_another_account() {
    let mut w = World::new("redirect");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    refused(w.cancel(T0, native(STRANGER, 0), CREATOR), 7017);
}

#[test]
fn f3_non_cancelable_refuses_cancel() {
    let mut w = World::new("noncancel");
    let mut s = w.linear(T0, T0 + MIN, 10);
    s.cancelable = 0;
    w.with(s, 10);
    refused(w.cancel(T0, native(REFUND, 0), CREATOR), 7010);
}

#[test]
fn f3_make_non_cancelable_is_one_way() {
    let mut w = World::new("oneway");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    let creator = acc(CREATOR, AUTH_TRANSFER, 0, vec![], true);
    let r = run(&w.elf, &w.pid, &Ix::MakeNonCancelable { schedule_id: w.id }, vec![w.schedule.clone(), creator.clone()]).unwrap();
    let s = r.schedule(&w.schedule);
    assert_eq!(s.cancelable, 0);
    w.with(s, 10);
    refused(run(&w.elf, &w.pid, &Ix::MakeNonCancelable { schedule_id: w.id }, vec![w.schedule.clone(), creator]), 7010);
}

#[test]
fn soft_a_nominated_cancel_authority_replaces_the_creator() {
    let mut w = World::new("cancelauth");
    let mut s = w.linear(T0, T0 + MIN, 10);
    s.cancel_authority = STRANGER;
    w.with(s, 10);
    refused(w.cancel(T0, native(REFUND, 0), CREATOR), 7015);
    assert!(w.cancel(T0, native(REFUND, 0), STRANGER).is_ok());
}

// ---------------------------------------------------------------- transfer (F4)

#[test]
fn f4_only_the_holder_moves_a_transferable_position() {
    let mut w = World::new("xfer");
    w.with(w.linear(T0, T0 + MIN, 10), 10);
    let ix = Ix::TransferBeneficiary { schedule_id: w.id, new_beneficiary: DEST };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]), 7004);
    let r = run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(BENEFICIARY, AUTH_TRANSFER, 0, vec![], true)]).unwrap();
    assert_eq!(r.schedule(&w.schedule).beneficiary, DEST);
}

#[test]
fn f4_a_non_transferable_position_cannot_move() {
    let mut w = World::new("noxfer");
    let mut s = w.linear(T0, T0 + MIN, 10);
    s.transferable = 0;
    w.with(s, 10);
    let ix = Ix::TransferBeneficiary { schedule_id: w.id, new_beneficiary: DEST };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(BENEFICIARY, AUTH_TRANSFER, 0, vec![], true)]), 7012);
}

// ---------------------------------------------------------------- milestones (R4, soft)

#[test]
fn r4_signalling_a_milestone_twice_is_refused() {
    let mut w = World::new("twice");
    let mut s = w.linear(0, 1, 2);
    s.kind = 2;
    s.tranches = 2;
    s.signalled = 0b01;
    w.with(s, 2);
    let ix = Ix::SignalMilestone { schedule_id: w.id, index: 0 };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]), 7013);
}

#[test]
fn soft_a_nominated_milestone_authority_replaces_the_creator() {
    let mut w = World::new("msauth");
    let mut s = w.linear(0, 1, 2);
    s.kind = 2;
    s.tranches = 2;
    s.milestone_authority = STRANGER;
    w.with(s, 2);
    let ix = Ix::SignalMilestone { schedule_id: w.id, index: 0 };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]), 7015);
    let r = run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(STRANGER, AUTH_TRANSFER, 0, vec![], true)]).unwrap();
    assert_eq!(r.schedule(&w.schedule).signalled, 0b01);
}

#[test]
fn a_cancelled_milestone_schedule_refuses_further_signals() {
    let mut w = World::new("mscancel");
    let mut s = w.linear(0, 1, 2);
    s.kind = 2;
    s.tranches = 2;
    s.cancelled_at = T0;
    w.with(s, 0);
    let ix = Ix::SignalMilestone { schedule_id: w.id, index: 0 };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]), 7011);
}

// ---------------------------------------------------------------- tokens (F1)

#[test]
fn f1_a_token_claim_is_a_chained_transfer_under_our_pda_seed() {
    let mut w = World::new("token");
    let mut s = w.linear(T0, T0 + MIN, 100);
    s.asset = 1;
    s.token_definition = DEFINITION;
    w.with(s, 100);
    let dest = token_holding(DEST, DEFINITION, 0, false);
    let r = w.claim(T0 + MIN, dest, BENEFICIARY).unwrap();
    let calls = r.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program_id, TOKEN, "the pinned token program, not an owner field");
    assert_eq!(calls[0].pda_seeds.len(), 1, "our holding's seed authorises it to the callee");
    assert!(calls[0].pre_states[0].is_authorized);
    // Neither balance is written by us: the token program moves them.
    assert_eq!(r.post(&w.holding).data, w.holding.account.data);
}

#[test]
fn f1_a_token_claim_into_a_holding_of_another_token_is_refused() {
    let mut w = World::new("wrongtoken");
    let mut s = w.linear(T0, T0 + MIN, 100);
    s.asset = 1;
    s.token_definition = DEFINITION;
    w.with(s, 100);
    refused(w.claim(T0 + MIN, token_holding(DEST, [0x99; 32], 0, false), BENEFICIARY), 7016);
    refused(w.claim(T0 + MIN, native(DEST, 0), BENEFICIARY), 7016);
}

// ---------------------------------------------------------------- creation

#[test]
fn creation_refuses_a_refund_account_equal_to_the_cancel_authority() {
    let w = World::new("create");
    let ix = Ix::CreateSchedule {
        schedule_id: w.id, kind: 1, start: T0, cliff: T0, end: T0 + MIN, total: 10,
        beneficiary: BENEFICIARY, cancelable: 1, transferable: 0, tranches: 0,
        cancel_authority: Z, milestone_authority: Z, refund_to: CREATOR,
    };
    refused(run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), w.holding.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]), 7017);
}

#[test]
fn creation_writes_the_terms_and_defaults_the_authorities_to_the_creator() {
    let w = World::new("create2");
    let ix = Ix::CreateSchedule {
        schedule_id: w.id, kind: 1, start: T0, cliff: T0, end: T0 + MIN, total: 10,
        beneficiary: BENEFICIARY, cancelable: 1, transferable: 0, tranches: 0,
        cancel_authority: Z, milestone_authority: Z, refund_to: REFUND,
    };
    let r = run(&w.elf, &w.pid, &ix, vec![w.schedule.clone(), w.holding.clone(), acc(CREATOR, AUTH_TRANSFER, 0, vec![], true)]).unwrap();
    let s = r.schedule(&w.schedule);
    assert_eq!((s.cancel_authority, s.milestone_authority, s.refund_to), (CREATOR, CREATOR, REFUND));
    assert_eq!(s.escrow, *w.holding.account_id.value());
    let _: ProgramId = w.pid;
}
