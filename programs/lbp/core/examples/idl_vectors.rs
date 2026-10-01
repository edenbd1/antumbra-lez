//! Borsh vectors for the IDL drift check (scripts/check-idl.py).
//!
//! One sample of every instruction variant, of the stored pool, and of every
//! event, with the values they were built from; plus the event selectors, the
//! refusal codes and the timestamp windows. The script decodes each vector with
//! nothing but the IDL and requires the same values back and every byte
//! consumed, so a field added, removed or reordered in the Rust types without
//! the IDL fails the check.

use antumbra_lbp_core::*;
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

fn v<T: BorshSerialize>(name: &str, x: &T, values: Value) -> Value {
    json!({"name": name, "hex": hx(&borsh::to_vec(x).unwrap()), "values": values})
}

fn main() {
    let tok = Asset::Token { definition_id: a(0x14) };
    let pid = [0x21u8; 32];
    let big = (1u128 << 100) + 7;
    let ix = vec![
        v(
            "create_pool",
            &Instruction::CreatePool {
                pool_id: pid,
                collateral: tok,
                reserve_token: big,
                reserve_collateral: 2,
                w_start: 3,
                w_end: 4,
                t_start: 1_700_000_000_001,
                t_end: 1_700_000_000_002,
                fee_treasury: a(0x31),
                fee_rate: 50_000,
            },
            json!({"pool_id": hx(&pid), "collateral": asset_json(&tok), "reserve_token": big.to_string(),
                   "reserve_collateral": "2", "w_start": "3", "w_end": "4", "t_start": 1_700_000_000_001u64,
                   "t_end": 1_700_000_000_002u64, "fee_treasury": aid(&a(0x31)), "fee_rate": "50000"}),
        ),
        v(
            "execute_buy",
            &Instruction::ExecuteBuy { pool_id: pid, collateral: Asset::Native, now: 5, collateral_in: 6, min_tokens_out: 7 },
            json!({"pool_id": hx(&pid), "collateral": asset_json(&Asset::Native), "now": 5, "collateral_in": "6", "min_tokens_out": "7"}),
        ),
        v(
            "withdraw",
            &Instruction::Withdraw { pool_id: pid, collateral: tok, now: 8, amount: 9, fee: 10 },
            json!({"pool_id": hx(&pid), "collateral": asset_json(&tok), "now": 8, "amount": "9", "fee": "10"}),
        ),
        v(
            "set_paused",
            &Instruction::SetPaused { pool_id: pid, paused: 1 },
            json!({"pool_id": hx(&pid), "paused": 1}),
        ),
    ];
    let p = Pool {
        reserve_token: 1,
        reserve_collateral: 2,
        w_start: 3,
        w_end: 4,
        t_start: 5,
        t_end: 6,
        last_seen: 7,
        paused: 1,
        creator: a(0x41),
        treasury: a(0x42),
        fee_treasury: a(0x43),
        fee_rate: 8,
        raised: 9,
        withdrawn: 10,
        collateral: 1,
        collateral_definition: [0x44; 32],
    };
    let state = v(
        "Pool",
        &p,
        json!({"reserve_token": "1", "reserve_collateral": "2", "w_start": "3", "w_end": "4", "t_start": 5,
               "t_end": 6, "last_seen": 7, "paused": 1, "creator": aid(&a(0x41)), "treasury": aid(&a(0x42)),
               "fee_treasury": aid(&a(0x43)), "fee_rate": "8", "raised": "9", "withdrawn": "10",
               "collateral": 1, "collateral_definition": hx(&[0x44; 32])}),
    );
    let events = vec![
        v(
            "PoolCreated",
            &PoolCreated {
                pool_id: pid,
                creator: a(1),
                holding: a(2),
                collateral: tok,
                reserve_token: 3,
                reserve_collateral: 4,
                w_start: 5,
                w_end: 6,
                t_start: 7,
                t_end: 8,
                fee_rate: 9,
            },
            json!({"pool_id": hx(&pid), "creator": aid(&a(1)), "holding": aid(&a(2)), "collateral": asset_json(&tok),
                   "reserve_token": "3", "reserve_collateral": "4", "w_start": "5", "w_end": "6", "t_start": 7,
                   "t_end": 8, "fee_rate": "9"}),
        ),
        v(
            "BuyPaid",
            &BuyPaid { pool_id: pid, buyer: a(3), now: 10, collateral_in: 11, min_tokens_out: 12 },
            json!({"pool_id": hx(&pid), "buyer": aid(&a(3)), "now": 10, "collateral_in": "11", "min_tokens_out": "12"}),
        ),
        v(
            "Withdrawn",
            &Withdrawn { pool_id: pid, to_creator: 13, fee: 14, destination: a(4) },
            json!({"pool_id": hx(&pid), "to_creator": "13", "fee": "14", "destination": aid(&a(4))}),
        ),
        v("PauseSet", &PauseSet { pool_id: pid, paused: 0 }, json!({"pool_id": hx(&pid), "paused": 0})),
    ];
    let selectors: Vec<Value> = EVENT_NAMES.iter().map(|n| json!({"name": n, "selector": hx(&event_selector(n))})).collect();
    let errs: Vec<Value> = errors::ALL.iter().map(|(c, n)| json!({"code": c, "name": n})).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "instructions": ix, "state": state, "events": events, "selectors": selectors, "errors": errs,
            "windows": {
                "execute_buy": format!("[now, now + {BUY_WINDOW_MS})"),
                "withdraw": "[now, inf)",
            },
        }))
        .unwrap()
    );
}
