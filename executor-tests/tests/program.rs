//! What the committed v0.3 binary accepts and refuses, run through the LEE
//! state machine. Each refusal is asserted by its code, and every refused
//! transaction is checked to have moved nothing.

use antumbra_executor_tests::{code, Chain, Key, VESTING};
use antumbra_vesting_core::{
    batch_schedule_id, event_selector, holding_account, rows, Asset, Instruction, Terms, CANCEL_WINDOW_MS,
};
use lee::AccountId;

const T0: u64 = 1_700_000_000_000;
const MIN: u64 = 60_000;
const REFUND: AccountId = AccountId::new([0xAA; 32]);

struct World {
    chain: Chain,
    creator: Key,
    ben: Key,
    other: Key,
    auth: Key,
    refund: AccountId,
    def: AccountId,
}

fn world() -> World {
    let creator = Key::new(1);
    let ben = Key::new(2);
    let other = Key::new(3);
    let auth = Key::new(4);
    let refund = REFUND;
    let def = AccountId::new([0xDE; 32]);
    let chain = Chain::new(
        &[(creator.id, 1_000_000), (ben.id, 10), (other.id, 10), (auth.id, 10)],
        &[(creator.id, def, 1_000_000)],
    );
    World { chain, creator, ben, other, auth, refund, def }
}

fn linear(total: u128, refund: AccountId) -> Terms {
    Terms {
        kind: 1,
        start: T0,
        cliff: T0,
        end: T0 + 30 * MIN,
        total,
        tranches: 0,
        cancelable: true,
        transferable: true,
        cancel_authority: AccountId::default(),
        milestone_authority: AccountId::default(),
        refund_to: refund,
    }
}

fn create(w: &mut World, sid: [u8; 32], asset: Asset, terms: Terms) -> Result<(), String> {
    let ix = Instruction::CreateSchedule { schedule_id: sid, beneficiary: w.ben.id, asset, terms };
    let r = rows::create(&VESTING, &sid, &asset, w.creator.id);
    let signer = Key::new(1);
    w.chain.send(&ix, r, &[&signer])
}

fn claim(w: &mut World, sid: [u8; 32], asset: Asset, dest: AccountId, who: &Key, amount: u128, at: u64) -> Result<(), String> {
    let ix = Instruction::Claim { schedule_id: sid, batch_id: None, asset, amount, at };
    let r = rows::claim(&VESTING, &sid, None, &asset, dest, who.id);
    w.chain.send(&ix, r, &[who])
}

fn cancel(w: &mut World, sid: [u8; 32], asset: Asset, who: &Key, at: u64, refund: u128) -> Result<(), String> {
    let ix = Instruction::Cancel { schedule_id: sid, batch_id: None, asset, at, refund };
    let r = rows::cancel(&VESTING, &sid, None, &asset, w.refund, who.id);
    w.chain.send(&ix, r, &[who])
}

#[track_caller]
fn refused(r: Result<(), String>, want: u32) {
    let err = r.expect_err("expected a refusal");
    assert_eq!(code(&err), Some(want), "wrong refusal: {err}");
}

#[test]
fn a_linear_native_schedule_pays_exactly_what_has_vested() {
    let mut w = world();
    let sid = [1; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let hold = holding_account(&VESTING, &sid);
    assert_eq!(w.chain.native(hold), 600, "funded exactly at creation");
    assert_eq!(w.chain.native(w.creator.id), 1_000_000 - 600);
    assert_eq!(w.chain.events[0].event.selector, event_selector("ScheduleCreated"));

    // 2 minutes into 30: floor(600 * 2 / 30) = 40.
    w.chain.now = T0 + 2 * MIN;
    let ben = Key::new(2);
    let dest = AccountId::new([0xB0; 32]);
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 41, T0 + 2 * MIN), 7003);
    assert_eq!(w.chain.native(hold), 600, "the refused claim moved nothing");
    claim(&mut w, sid, Asset::Native, dest, &ben, 40, T0 + 2 * MIN).unwrap();
    assert_eq!((w.chain.native(dest), w.chain.native(hold)), (40, 560));
    assert_eq!(w.chain.schedule(&sid).claimed, 40);
    assert_eq!(w.chain.events[0].event.selector, event_selector("Claimed"));

    // A time the chain has not reached is outside the window.
    let e = claim(&mut w, sid, Asset::Native, dest, &ben, 1, T0 + 10 * MIN).unwrap_err();
    assert!(e.contains("validity window"), "{e}");

    // The beneficiary may claim into its own account: the signer row is a
    // handle on this program's shard, the destination its native shard.
    w.chain.now = T0 + 31 * MIN;
    claim(&mut w, sid, Asset::Native, ben.id, &ben, 560, T0 + 30 * MIN).unwrap();
    assert_eq!(w.chain.native(ben.id), 10 + 560);
    assert_eq!(w.chain.native(hold), 0);
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 1, T0 + 30 * MIN), 7003);
}

#[test]
fn only_the_beneficiary_claims() {
    let mut w = world();
    let sid = [2; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    w.chain.now = T0 + 31 * MIN;
    let creator = Key::new(1);
    refused(claim(&mut w, sid, Asset::Native, creator.id, &creator, 10, T0 + 30 * MIN), 7004);
    assert_eq!(w.chain.native(holding_account(&VESTING, &sid)), 600);
}

#[test]
fn nothing_vests_before_the_cliff_then_the_accrual_unlocks_at_once() {
    let mut w = world();
    let sid = [3; 32];
    let mut t = linear(1_000, REFUND);
    t.kind = 0;
    t.cliff = T0 + 10 * MIN;
    t.end = T0 + 40 * MIN;
    create(&mut w, sid, Asset::Native, t).unwrap();
    let ben = Key::new(2);
    let dest = AccountId::new([0xB1; 32]);
    w.chain.now = T0 + 10 * MIN;
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 1, T0 + 10 * MIN - 1), 7003);
    claim(&mut w, sid, Asset::Native, dest, &ben, 250, T0 + 10 * MIN).unwrap();
    assert_eq!(w.chain.native(dest), 250);
}

#[test]
fn a_duplicate_creation_is_refused_and_takes_no_second_funding() {
    let mut w = world();
    let sid = [4; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    refused(create(&mut w, sid, Asset::Native, linear(900, REFUND)), 7019);
    assert_eq!(w.chain.native(holding_account(&VESTING, &sid)), 600);
    assert_eq!(w.chain.schedule(&sid).total, 600);
}

#[test]
fn a_creation_the_creator_cannot_fund_writes_nothing() {
    let mut w = world();
    let sid = [5; 32];
    // The native transfer refuses the debit (the state machine reports the
    // native program's failure as an execution-rule violation), and the whole
    // transaction goes with it, the schedule write included.
    let e = create(&mut w, sid, Asset::Native, linear(2_000_000, REFUND)).unwrap_err();
    assert!(e.contains("execution rules"), "{e}");
    assert!(!w.chain.has_schedule(&sid));
    assert_eq!(w.chain.native(w.creator.id), 1_000_000);
}

#[test]
fn cancellation_returns_the_unvested_part_and_leaves_the_vested_claimable() {
    let mut w = world();
    let sid = [6; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let hold = holding_account(&VESTING, &sid);
    let creator = Key::new(1);
    let ben = Key::new(2);
    let at = T0 + 7 * MIN; // 140 of 600 vested
    w.chain.now = at + 1_000;
    refused(cancel(&mut w, sid, Asset::Native, &creator, at, 461), 7020);
    refused(cancel(&mut w, sid, Asset::Native, &creator, at, 459), 7020);
    refused(cancel(&mut w, sid, Asset::Native, &ben, at, 460), 7015);
    assert_eq!(w.chain.native(hold), 600);
    cancel(&mut w, sid, Asset::Native, &creator, at, 460).unwrap();
    assert_eq!(w.chain.native(w.refund), 460);
    assert_eq!(w.chain.native(hold), 140);
    assert_eq!(w.chain.schedule(&sid).cancelled_at, at);
    refused(cancel(&mut w, sid, Asset::Native, &creator, at + 1, 0), 7011);

    // Accrual is frozen: long after, the beneficiary gets exactly 140.
    w.chain.now = T0 + 60 * MIN;
    let dest = AccountId::new([0xB2; 32]);
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 141, T0 + 60 * MIN), 7003);
    claim(&mut w, sid, Asset::Native, dest, &ben, 140, T0 + 60 * MIN).unwrap();
    assert_eq!(w.chain.native(hold), 0);
}

#[test]
fn a_cancellation_cannot_be_back_dated_past_its_window() {
    let mut w = world();
    let sid = [7; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let creator = Key::new(1);
    let at = T0 + 5 * MIN;
    w.chain.now = at + CANCEL_WINDOW_MS; // one millisecond too late
    let e = cancel(&mut w, sid, Asset::Native, &creator, at, 500).unwrap_err();
    assert!(e.contains("validity window"), "{e}");
    w.chain.now = at + CANCEL_WINDOW_MS - 1;
    cancel(&mut w, sid, Asset::Native, &creator, at, 500).unwrap();
}

#[test]
fn a_cancellation_cannot_name_a_time_before_the_last_claim() {
    let mut w = world();
    let sid = [8; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let ben = Key::new(2);
    let creator = Key::new(1);
    w.chain.now = T0 + 10 * MIN;
    claim(&mut w, sid, Asset::Native, AccountId::new([0xB3; 32]), &ben, 200, T0 + 10 * MIN).unwrap();
    refused(cancel(&mut w, sid, Asset::Native, &creator, T0 + 9 * MIN, 420), 7005);
    cancel(&mut w, sid, Asset::Native, &creator, T0 + 10 * MIN, 400).unwrap();
    assert_eq!(w.chain.native(holding_account(&VESTING, &sid)), 0);
}

#[test]
fn non_cancelable_is_one_way_and_the_creators_alone() {
    let mut w = world();
    let sid = [9; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let ben = Key::new(2);
    let creator = Key::new(1);
    let ix = Instruction::MakeNonCancelable { schedule_id: sid };
    refused(w.chain.send(&ix, rows::handle(&VESTING, &sid, ben.id), &[&ben]), 7009);
    w.chain.send(&ix, rows::handle(&VESTING, &sid, creator.id), &[&creator]).unwrap();
    refused(w.chain.send(&ix, rows::handle(&VESTING, &sid, creator.id), &[&creator]), 7010);
    w.chain.now = T0 + MIN;
    refused(cancel(&mut w, sid, Asset::Native, &creator, T0 + MIN, 580), 7010);
}

#[test]
fn a_nominated_cancel_authority_cancels_and_the_creator_cannot() {
    let mut w = world();
    let sid = [10; 32];
    let mut t = linear(600, REFUND);
    t.cancel_authority = w.auth.id;
    create(&mut w, sid, Asset::Native, t).unwrap();
    let creator = Key::new(1);
    let auth = Key::new(4);
    w.chain.now = T0 + 3 * MIN;
    refused(cancel(&mut w, sid, Asset::Native, &creator, T0 + 3 * MIN, 540), 7015);
    cancel(&mut w, sid, Asset::Native, &auth, T0 + 3 * MIN, 540).unwrap();
    assert_eq!(w.chain.native(w.refund), 540);
}

#[test]
fn a_token_schedule_escrows_claims_and_refunds_the_token() {
    let mut w = world();
    let sid = [11; 32];
    let asset = Asset::Token { definition_id: w.def };
    create(&mut w, sid, asset, linear(100_000, REFUND)).unwrap();
    let hold = holding_account(&VESTING, &sid);
    assert_eq!(w.chain.token(hold), 100_000);
    assert_eq!(w.chain.token(w.creator.id), 900_000);
    let ben = Key::new(2);
    let dest = AccountId::new([0xB4; 32]); // no holding yet: the token program creates it
    w.chain.now = T0 + 3 * MIN;
    claim(&mut w, sid, asset, dest, &ben, 10_000, T0 + 3 * MIN).unwrap();
    assert_eq!(w.chain.token(dest), 10_000);

    // Naming the wrong asset is refused, whichever way round.
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 1, T0 + 3 * MIN), 7016);
    let other = Asset::Token { definition_id: AccountId::new([0xEE; 32]) };
    refused(claim(&mut w, sid, other, dest, &ben, 1, T0 + 3 * MIN), 7016);

    let creator = Key::new(1);
    cancel(&mut w, sid, asset, &creator, T0 + 3 * MIN, 90_000).unwrap();
    assert_eq!(w.chain.token(w.refund), 90_000);
    assert_eq!(w.chain.token(hold), 0);
}

#[test]
fn milestones_unlock_in_equal_tranches_signalled_once_by_their_authority() {
    let mut w = world();
    let sid = [12; 32];
    let mut t = linear(1_000, REFUND);
    t.kind = 2;
    t.tranches = 4;
    t.milestone_authority = w.auth.id;
    create(&mut w, sid, Asset::Native, t).unwrap();
    let creator = Key::new(1);
    let auth = Key::new(4);
    let ben = Key::new(2);
    let sig = |i| Instruction::SignalMilestone { schedule_id: sid, index: i };
    refused(w.chain.send(&sig(0), rows::handle(&VESTING, &sid, creator.id), &[&creator]), 7015);
    w.chain.send(&sig(0), rows::handle(&VESTING, &sid, auth.id), &[&auth]).unwrap();
    refused(w.chain.send(&sig(0), rows::handle(&VESTING, &sid, auth.id), &[&auth]), 7013);
    refused(w.chain.send(&sig(4), rows::handle(&VESTING, &sid, auth.id), &[&auth]), 7013);
    w.chain.send(&sig(2), rows::handle(&VESTING, &sid, auth.id), &[&auth]).unwrap();
    let dest = AccountId::new([0xB5; 32]);
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 501, T0), 7003);
    claim(&mut w, sid, Asset::Native, dest, &ben, 500, T0).unwrap();
    assert_eq!(w.chain.native(dest), 500);
}

#[test]
fn a_position_moves_only_by_its_holder_and_only_if_transferable() {
    let mut w = world();
    let sid = [13; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    let creator = Key::new(1);
    let ben = Key::new(2);
    let other = Key::new(3);
    let tr = |to| Instruction::TransferBeneficiary { schedule_id: sid, new_beneficiary: to };
    refused(w.chain.send(&tr(creator.id), rows::handle(&VESTING, &sid, creator.id), &[&creator]), 7004);
    w.chain.send(&tr(other.id), rows::handle(&VESTING, &sid, ben.id), &[&ben]).unwrap();
    w.chain.now = T0 + 30 * MIN;
    let dest = AccountId::new([0xB6; 32]);
    refused(claim(&mut w, sid, Asset::Native, dest, &ben, 1, T0 + 30 * MIN), 7004);
    claim(&mut w, sid, Asset::Native, dest, &other, 600, T0 + 30 * MIN).unwrap();

    let fixed = [14; 32];
    let mut t = linear(600, REFUND);
    t.transferable = false;
    create(&mut w, fixed, Asset::Native, t).unwrap();
    let tr2 = Instruction::TransferBeneficiary { schedule_id: fixed, new_beneficiary: other.id };
    refused(w.chain.send(&tr2, rows::handle(&VESTING, &fixed, ben.id), &[&ben]), 7012);
}

#[test]
fn a_batch_is_funded_once_and_each_schedule_moves_only_its_share() {
    let mut w = world();
    let bid = [15; 32];
    let n = 8u32;
    let bens: Vec<Key> = (0..n).map(|i| Key::new(40 + i as u8)).collect();
    let ix = Instruction::CreateScheduleBatch {
        batch_id: bid,
        beneficiaries: bens.iter().map(|k| k.id).collect(),
        asset: Asset::Native,
        terms: linear(600, REFUND),
    };
    let creator = Key::new(1);
    w.chain.send(&ix, rows::batch(&VESTING, &bid, n, &Asset::Native, creator.id), &[&creator]).unwrap();
    let hold = holding_account(&VESTING, &bid);
    assert_eq!(w.chain.native(hold), 600 * u128::from(n));

    let s0 = batch_schedule_id(&bid, 0);
    let s1 = batch_schedule_id(&bid, 1);
    w.chain.now = T0 + 15 * MIN;
    let claim0 = Instruction::Claim { schedule_id: s0, batch_id: Some(bid), asset: Asset::Native, amount: 300, at: T0 + 15 * MIN };
    let dest = AccountId::new([0xB7; 32]);
    // Schedule 0's beneficiary only.
    refused(
        w.chain.send(&claim0, rows::claim(&VESTING, &s0, Some(&bid), &Asset::Native, dest, bens[1].id), &[&bens[1]]),
        7004,
    );
    w.chain.send(&claim0, rows::claim(&VESTING, &s0, Some(&bid), &Asset::Native, dest, bens[0].id), &[&bens[0]]).unwrap();
    let cancel1 = Instruction::Cancel { schedule_id: s1, batch_id: Some(bid), asset: Asset::Native, at: T0 + 15 * MIN, refund: 300 };
    w.chain.send(&cancel1, rows::cancel(&VESTING, &s1, Some(&bid), &Asset::Native, w.refund, creator.id), &[&creator]).unwrap();
    assert_eq!(w.chain.native(dest), 300);
    assert_eq!(w.chain.native(w.refund), 300);
    assert_eq!(w.chain.native(hold), 600 * u128::from(n) - 600);
    assert_eq!(w.chain.schedule(&batch_schedule_id(&bid, 7)).claimed, 0);
    // A batch schedule cannot be pointed at a different escrow.
    let wrong = Instruction::Claim { schedule_id: s0, batch_id: None, asset: Asset::Native, amount: 1, at: T0 + 15 * MIN };
    refused(
        w.chain.send(&wrong, rows::claim(&VESTING, &s0, None, &Asset::Native, dest, bens[0].id), &[&bens[0]]),
        7006,
    );
}

#[test]
fn a_batch_row_out_of_order_is_refused() {
    let mut w = world();
    let bid = [16; 32];
    let creator = Key::new(1);
    let ix = Instruction::CreateScheduleBatch {
        batch_id: bid,
        beneficiaries: vec![w.ben.id, w.other.id],
        asset: Asset::Native,
        terms: linear(600, REFUND),
    };
    let mut r = rows::batch(&VESTING, &bid, 2, &Asset::Native, creator.id);
    r.swap(2, 3);
    refused(w.chain.send(&ix, r, &[&creator]), 7018);
}

#[test]
fn an_unsigned_claim_is_refused() {
    let mut w = world();
    let sid = [17; 32];
    create(&mut w, sid, Asset::Native, linear(600, REFUND)).unwrap();
    w.chain.now = T0 + 30 * MIN;
    let ix = Instruction::Claim { schedule_id: sid, batch_id: None, asset: Asset::Native, amount: 600, at: T0 + 30 * MIN };
    let r = rows::claim(&VESTING, &sid, None, &Asset::Native, w.other.id, w.ben.id);
    refused(w.chain.send(&ix, r, &[]), 7022);
}
