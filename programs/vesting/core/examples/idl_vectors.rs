//! Borsh vectors for the IDL drift check (scripts/check-idl.py).
//!
//! One sample of every instruction variant, of the stored schedule, and of every
//! event, with the values they were built from; plus the event selectors and the
//! refusal codes. The script decodes each vector with nothing but the IDL and
//! requires the same values back and every byte consumed, so a field added,
//! removed or reordered in the Rust types without the IDL fails the check.

use antumbra_vesting_core::*;
use borsh::BorshSerialize;
use lee_core::account::AccountId;
use serde_json::{json, Value};

fn a(b: u8) -> AccountId {
    AccountId::new([b; 32])
}

fn hx(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn aid(x: &AccountId) -> Value {
    json!(hx(x.value()))
}

fn asset_json(x: &Asset) -> Value {
    match x {
        Asset::Native => json!({"Native": {}}),
        Asset::Token { definition_id } => json!({"Token": {"definition_id": aid(definition_id)}}),
    }
}

fn terms_json(t: &Terms) -> Value {
    json!({
        "kind": t.kind, "start": t.start, "cliff": t.cliff, "end": t.end, "total": t.total.to_string(),
        "tranches": t.tranches, "cancelable": t.cancelable, "transferable": t.transferable,
        "cancel_authority": aid(&t.cancel_authority), "milestone_authority": aid(&t.milestone_authority),
        "refund_to": aid(&t.refund_to),
    })
}

fn v<T: BorshSerialize>(name: &str, x: &T, values: Value) -> Value {
    json!({"name": name, "hex": hx(&borsh::to_vec(x).unwrap()), "values": values})
}

fn main() {
    let terms = Terms {
        kind: 0,
        start: 1_700_000_000_001,
        cliff: 1_700_000_000_002,
        end: 1_700_000_000_003,
        total: (1u128 << 100) + 7,
        tranches: 9,
        cancelable: true,
        transferable: false,
        cancel_authority: a(0x11),
        milestone_authority: a(0x12),
        refund_to: a(0x13),
    };
    let tok = Asset::Token { definition_id: a(0x14) };
    let sid = [0x21u8; 32];
    let bid = [0x22u8; 32];
    let ix = vec![
        v(
            "create_schedule",
            &Instruction::CreateSchedule { schedule_id: sid, beneficiary: a(0x31), asset: tok, terms: terms.clone() },
            json!({"schedule_id": hx(&sid), "beneficiary": aid(&a(0x31)), "asset": asset_json(&tok), "terms": terms_json(&terms)}),
        ),
        v(
            "create_schedule_batch",
            &Instruction::CreateScheduleBatch {
                batch_id: bid,
                beneficiaries: vec![a(0x32), a(0x33)],
                asset: Asset::Native,
                terms: terms.clone(),
            },
            json!({"batch_id": hx(&bid), "beneficiaries": [aid(&a(0x32)), aid(&a(0x33))],
                   "asset": asset_json(&Asset::Native), "terms": terms_json(&terms)}),
        ),
        v(
            "claim",
            &Instruction::Claim { schedule_id: sid, batch_id: Some(bid), asset: tok, amount: 12345, at: 1_700_000_000_004 },
            json!({"schedule_id": hx(&sid), "batch_id": hx(&bid), "asset": asset_json(&tok), "amount": "12345", "at": 1_700_000_000_004u64}),
        ),
        v(
            "cancel",
            &Instruction::Cancel { schedule_id: sid, batch_id: None, asset: Asset::Native, at: 5, refund: 6 },
            json!({"schedule_id": hx(&sid), "batch_id": null, "asset": asset_json(&Asset::Native), "at": 5, "refund": "6"}),
        ),
        v(
            "make_non_cancelable",
            &Instruction::MakeNonCancelable { schedule_id: sid },
            json!({"schedule_id": hx(&sid)}),
        ),
        v(
            "transfer_beneficiary",
            &Instruction::TransferBeneficiary { schedule_id: sid, new_beneficiary: a(0x34) },
            json!({"schedule_id": hx(&sid), "new_beneficiary": aid(&a(0x34))}),
        ),
        v(
            "signal_milestone",
            &Instruction::SignalMilestone { schedule_id: sid, index: 63 },
            json!({"schedule_id": hx(&sid), "index": 63}),
        ),
    ];
    let s = VestingSchedule {
        kind: 1,
        start: 2,
        cliff: 3,
        end: 4,
        total: 5,
        claimed: 6,
        last_seen: 7,
        beneficiary: a(0x41),
        escrow: a(0x42),
        creator: a(0x43),
        cancelable: 1,
        transferable: 0,
        cancelled_at: 8,
        signalled: 9,
        tranches: 10,
        asset: 1,
        token_definition: [0x44; 32],
        refund_to: a(0x45),
        cancel_authority: a(0x46),
        milestone_authority: a(0x47),
    };
    let state = v(
        "VestingSchedule",
        &s,
        json!({"kind": 1, "start": 2, "cliff": 3, "end": 4, "total": "5", "claimed": "6", "last_seen": 7,
               "beneficiary": aid(&a(0x41)), "escrow": aid(&a(0x42)), "creator": aid(&a(0x43)),
               "cancelable": 1, "transferable": 0, "cancelled_at": 8, "signalled": 9, "tranches": 10,
               "asset": 1, "token_definition": hx(&[0x44; 32]), "refund_to": aid(&a(0x45)),
               "cancel_authority": aid(&a(0x46)), "milestone_authority": aid(&a(0x47))}),
    );
    let events = vec![
        v(
            "ScheduleCreated",
            &ScheduleCreated { schedule_id: sid, beneficiary: a(1), holding: a(2), asset: tok, kind: 2, total: 3 },
            json!({"schedule_id": hx(&sid), "beneficiary": aid(&a(1)), "holding": aid(&a(2)), "asset": asset_json(&tok), "kind": 2, "total": "3"}),
        ),
        v(
            "BatchCreated",
            &BatchCreated { batch_id: bid, holding: a(2), asset: Asset::Native, count: 4, total_each: 5 },
            json!({"batch_id": hx(&bid), "holding": aid(&a(2)), "asset": asset_json(&Asset::Native), "count": 4, "total_each": "5"}),
        ),
        v(
            "Claimed",
            &Claimed { schedule_id: sid, amount: 6, at: 7, destination: a(3) },
            json!({"schedule_id": hx(&sid), "amount": "6", "at": 7, "destination": aid(&a(3))}),
        ),
        v(
            "Cancelled",
            &Cancelled { schedule_id: sid, at: 8, refund: 9, refund_to: a(4) },
            json!({"schedule_id": hx(&sid), "at": 8, "refund": "9", "refund_to": aid(&a(4))}),
        ),
        v("MadeNonCancelable", &MadeNonCancelable { schedule_id: sid }, json!({"schedule_id": hx(&sid)})),
        v(
            "BeneficiaryTransferred",
            &BeneficiaryTransferred { schedule_id: sid, from: a(5), to: a(6) },
            json!({"schedule_id": hx(&sid), "from": aid(&a(5)), "to": aid(&a(6))}),
        ),
        v(
            "MilestoneSignalled",
            &MilestoneSignalled { schedule_id: sid, index: 3 },
            json!({"schedule_id": hx(&sid), "index": 3}),
        ),
    ];
    let selectors: Vec<Value> = EVENT_NAMES.iter().map(|n| json!({"name": n, "selector": hx(&event_selector(n))})).collect();
    let errs: Vec<Value> = errors::ALL.iter().map(|(c, n)| json!({"code": c, "name": n})).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "instructions": ix, "state": state, "events": events, "selectors": selectors, "errors": errs,
            "cancel_window_ms": CANCEL_WINDOW_MS,
        }))
        .unwrap()
    );
}
