//! Host tests of plan and apply, without a zkVM. The executor tests run the
//! same logic inside the committed binary through the LEE state machine.

use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

const ME: AccountId = AccountId::new([7; 32]);
const CREATOR: AccountId = AccountId::new([1; 32]);
const BEN: AccountId = AccountId::new([2; 32]);
const REFUND: AccountId = AccountId::new([3; 32]);
const SID: [u8; 32] = [9; 32];

fn refusal<T>(f: impl FnOnce() -> T) -> String {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(_) => panic!("expected a refusal"),
        Err(e) => e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default(),
    }
}

fn terms(kind: u8) -> Terms {
    Terms {
        kind,
        start: 1_000,
        cliff: if kind == 0 { 2_000 } else { 1_000 },
        end: 11_000,
        total: 600,
        tranches: if kind == 2 { 4 } else { 0 },
        cancelable: true,
        transferable: true,
        cancel_authority: AccountId::default(),
        milestone_authority: AccountId::default(),
        refund_to: REFUND,
    }
}

fn state(kind: u8) -> VestingSchedule {
    new_state(CREATOR, holding_account(&ME, &SID), BEN, &Asset::Native, &terms(kind))
}

fn input(accounts: Vec<AccountMeta>, ix: &Instruction) -> PlanInput {
    PlanInput {
        self_account_id: ME,
        caller_account_id: None,
        accounts,
        instruction_data: borsh::to_vec(ix).unwrap(),
    }
}

fn claim_ix(amount: u128, at: u64) -> Instruction {
    Instruction::Claim {
        schedule_id: SID,
        batch_id: None,
        asset: Asset::Native,
        amount,
        at,
    }
}

fn claim_rows() -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(schedule_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
        AccountMeta::native_balance(BEN, false),
        AccountMeta::new(BEN, true, ME),
    ]
}

#[test]
fn seeds_follow_spels_rule() {
    assert_eq!(schedule_seed(&SID), PdaSeed::new(SID));
    let mut both = SID.to_vec();
    both.extend_from_slice(&seed_from_str("holding"));
    assert_eq!(holding_seed(&SID), PdaSeed::new(sha256(&[&both])));
    assert_ne!(batch_schedule_id(&SID, 0), batch_schedule_id(&SID, 1));
}

#[test]
fn a_creation_writes_the_terms_and_funds_exactly_the_total() {
    let ix = Instruction::CreateSchedule {
        schedule_id: SID,
        beneficiary: BEN,
        asset: Asset::Native,
        terms: terms(1),
    };
    let rows = vec![
        AccountMeta::new(schedule_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
        AccountMeta::native_balance(CREATOR, true),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    assert_eq!(out.effects.len(), 1);
    let Effect::Create(s) = borsh::from_slice(&out.effects[0].data).unwrap() else {
        panic!("not a creation")
    };
    assert_eq!((s.total, s.beneficiary, s.creator, s.cancel_authority), (600, BEN, CREATOR, CREATOR));
    assert_eq!(out.chained_calls.len(), 1);
    let call = &out.chained_calls[0];
    assert_eq!(call.program_account_id, NATIVE_TOKEN_PROGRAM_ID);
    assert_eq!(
        borsh::from_slice::<native_token::Instruction>(&call.instruction_data).unwrap(),
        native_token::Instruction::Transfer { amount: 600 }
    );
    assert_eq!(out.events[0].selector, event_selector("ScheduleCreated"));
}

#[test]
fn a_creation_over_an_existing_schedule_is_refused() {
    let bytes = store(&state(1));
    assert!(refusal(|| apply(Effect::Create(state(1)), &bytes)).starts_with("E7019"));
}

#[test]
fn a_claim_is_bounded_by_what_has_vested_at_the_time_it_names() {
    let pre = store(&state(1));
    let at = 1_000 + 5_000; // half way: 300 vested
    let hold = holding_account(&ME, &SID);
    let claim = |amount| Effect::Claim { beneficiary: BEN, holding: hold, asset: Asset::Native, amount, at };
    let post = apply(claim(300), &pre).unwrap();
    assert_eq!(load(&post).claimed, 300);
    assert_eq!(load(&post).last_seen, at);
    assert!(refusal(|| apply(claim(301), &pre)).starts_with("E7003"));
    // After 300 are claimed, nothing more is owed at the same instant.
    assert!(refusal(|| apply(claim(1), &post)).starts_with("E7003"));
}

#[test]
fn a_claim_by_anyone_but_the_beneficiary_is_refused() {
    let pre = store(&state(1));
    let e = Effect::Claim {
        beneficiary: CREATOR,
        holding: holding_account(&ME, &SID),
        asset: Asset::Native,
        amount: 1,
        at: 11_000,
    };
    assert!(refusal(|| apply(e, &pre)).starts_with("E7004"));
}

#[test]
fn a_claim_plans_a_window_from_its_time_and_a_seeded_payout() {
    let ix = claim_ix(40, 5_000);
    let out = plan(&input(claim_rows(), &ix), ix).output().clone();
    assert_eq!(out.timestamp_validity_window.start(), Some(5_000));
    assert_eq!(out.timestamp_validity_window.end(), None);
    assert_eq!(out.chained_calls[0].pda_seeds, vec![holding_seed(&SID)]);
}

#[test]
fn an_unsigned_claim_is_refused_in_the_plan() {
    let mut rows = claim_rows();
    rows[3].is_authorized = false;
    let ix = claim_ix(40, 5_000);
    assert!(refusal(|| plan(&input(rows, &ix), ix)).starts_with("E7022"));
}

#[test]
fn a_claim_against_a_foreign_holding_is_refused_in_the_plan() {
    let mut rows = claim_rows();
    rows[1] = AccountMeta::native_balance(AccountId::new([5; 32]), false);
    let ix = claim_ix(40, 5_000);
    assert!(refusal(|| plan(&input(rows, &ix), ix)).starts_with("E7006"));
}

#[test]
fn a_cancellation_window_is_bounded_on_both_sides() {
    let ix = Instruction::Cancel {
        schedule_id: SID,
        batch_id: None,
        asset: Asset::Native,
        at: 5_000,
        refund: 360,
    };
    let rows = vec![
        AccountMeta::new(schedule_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
        AccountMeta::native_balance(REFUND, false),
        AccountMeta::new(CREATOR, true, ME),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    assert_eq!(out.timestamp_validity_window.start(), Some(5_000));
    assert_eq!(out.timestamp_validity_window.end(), Some(5_000 + CANCEL_WINDOW_MS));
}

#[test]
fn a_cancellation_must_refund_exactly_the_unvested_part() {
    let pre = store(&state(1));
    let hold = holding_account(&ME, &SID);
    let cancel = |refund, at| Effect::Cancel {
        authority: CREATOR,
        holding: hold,
        refund_to: REFUND,
        asset: Asset::Native,
        at,
        refund,
    };
    // At 6000 of [1000, 11000), 300 of 600 have vested.
    assert!(refusal(|| apply(cancel(301, 6_000), &pre)).starts_with("E7020"));
    assert!(refusal(|| apply(cancel(299, 6_000), &pre)).starts_with("E7020"));
    let post = load(&apply(cancel(300, 6_000), &pre).unwrap());
    assert_eq!(post.cancelled_at, 6_000);
    // Accrual is frozen: a claim much later still sees 300.
    assert_eq!(claimable(&post, 1 << 40), 300);
    assert!(refusal(|| apply(cancel(0, 20_000), &store(&post))).starts_with("E7011"));
}

#[test]
fn a_cancellation_cannot_reach_back_before_a_claim() {
    let mut s = state(1);
    s.claimed = 300;
    s.last_seen = 6_000;
    let e = Effect::Cancel {
        authority: CREATOR,
        holding: holding_account(&ME, &SID),
        refund_to: REFUND,
        asset: Asset::Native,
        at: 5_999,
        refund: unvested(&s, 5_999),
    };
    assert!(refusal(|| apply(e, &store(&s))).starts_with("E7005"));
}

#[test]
fn milestones_signal_once_and_only_by_their_authority() {
    let pre = store(&state(2));
    let sig = |authority, index| Effect::SignalMilestone { authority, index };
    let once = apply(sig(CREATOR, 1), &pre).unwrap();
    assert_eq!(vested(&load(&once), 0), 150);
    assert!(refusal(|| apply(sig(CREATOR, 1), &once)).starts_with("E7013"));
    assert!(refusal(|| apply(sig(BEN, 2), &once)).starts_with("E7015"));
    assert!(refusal(|| apply(sig(CREATOR, 4), &once)).starts_with("E7013"));
}

#[test]
fn transfer_is_the_holders_and_non_cancelable_is_one_way() {
    let pre = store(&state(1));
    let t = |beneficiary| Effect::TransferBeneficiary { beneficiary, new_beneficiary: REFUND };
    assert!(refusal(|| apply(t(CREATOR), &pre)).starts_with("E7004"));
    assert_eq!(load(&apply(t(BEN), &pre).unwrap()).beneficiary, REFUND);
    let mut fixed = state(1);
    fixed.transferable = 0;
    assert!(refusal(|| apply(t(BEN), &store(&fixed))).starts_with("E7012"));

    let nc = |creator| Effect::MakeNonCancelable { creator };
    assert!(refusal(|| apply(nc(BEN), &pre)).starts_with("E7009"));
    let once = apply(nc(CREATOR), &pre).unwrap();
    assert!(refusal(|| apply(nc(CREATOR), &once)).starts_with("E7010"));
}

#[test]
fn degenerate_terms_are_refused_at_creation() {
    let mut t = terms(0);
    t.cliff = t.end;
    assert!(refusal(|| new_state(CREATOR, REFUND, BEN, &Asset::Native, &t)).starts_with("E7001"));
    let mut m = terms(2);
    m.tranches = 65;
    assert!(refusal(|| new_state(CREATOR, REFUND, BEN, &Asset::Native, &m)).starts_with("E7013"));
}

#[test]
fn the_asset_a_claim_names_must_be_the_schedules() {
    let pre = store(&state(1));
    let e = Effect::Claim {
        beneficiary: BEN,
        holding: holding_account(&ME, &SID),
        asset: Asset::Token { definition_id: REFUND },
        amount: 1,
        at: 11_000,
    };
    assert!(refusal(|| apply(e, &pre)).starts_with("E7016"));
}

#[test]
fn event_names_are_distinct() {
    let mut sel: Vec<_> = EVENT_NAMES.iter().map(|n| event_selector(n)).collect();
    sel.sort();
    sel.dedup();
    assert_eq!(sel.len(), EVENT_NAMES.len());
}
