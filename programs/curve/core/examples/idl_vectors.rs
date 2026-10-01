//! Borsh vectors for the IDL drift check (scripts/check-idl.py).
//!
//! One sample of every instruction variant, of the stored sale, and of every
//! event, with the values they were built from; plus the event selectors and the
//! refusal codes. The script decodes each vector with nothing but the IDL and
//! requires the same values back and every byte consumed, so a field added,
//! removed or reordered in the Rust types without the IDL fails the check.

use antumbra_curve_core::*;
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
    let sid = [0x21u8; 32];
    let big = (1u128 << 100) + 7;
    let ix = vec![
        v(
            "create_sale",
            &Instruction::CreateSale {
                sale_id: sid,
                collateral: tok,
                vt: big,
                vc: 2,
                sale_reserve: 3,
                seed_reserve: 4,
                fee_treasury: a(0x31),
                fee_rate: 10_000,
            },
            json!({"sale_id": hx(&sid), "collateral": asset_json(&tok), "vt": big.to_string(), "vc": "2",
                   "sale_reserve": "3", "seed_reserve": "4", "fee_treasury": aid(&a(0x31)), "fee_rate": "10000"}),
        ),
        v(
            "execute_buy",
            &Instruction::ExecuteBuy { sale_id: sid, collateral: Asset::Native, collateral_in: 5, min_tokens_out: 6 },
            json!({"sale_id": hx(&sid), "collateral": asset_json(&Asset::Native), "collateral_in": "5", "min_tokens_out": "6"}),
        ),
        v(
            "collect_fees",
            &Instruction::CollectFees { sale_id: sid, collateral: tok, amount: 7 },
            json!({"sale_id": hx(&sid), "collateral": asset_json(&tok), "amount": "7"}),
        ),
        v(
            "withdraw",
            &Instruction::Withdraw { sale_id: sid, collateral: Asset::Native, amount: 8 },
            json!({"sale_id": hx(&sid), "collateral": asset_json(&Asset::Native), "amount": "8"}),
        ),
    ];
    let s = Sale {
        vt: 1,
        vc: 2,
        sale_reserve: 3,
        real_collateral: 4,
        seed_reserve: 5,
        creator: a(0x41),
        treasury: a(0x42),
        fee_treasury: a(0x43),
        fee_rate: 6,
        fees_accrued: 7,
        withdrawn: 8,
        collateral: 1,
        collateral_definition: [0x44; 32],
    };
    let state = v(
        "Sale",
        &s,
        json!({"vt": "1", "vc": "2", "sale_reserve": "3", "real_collateral": "4", "seed_reserve": "5",
               "creator": aid(&a(0x41)), "treasury": aid(&a(0x42)), "fee_treasury": aid(&a(0x43)),
               "fee_rate": "6", "fees_accrued": "7", "withdrawn": "8", "collateral": 1,
               "collateral_definition": hx(&[0x44; 32])}),
    );
    let events = vec![
        v(
            "SaleCreated",
            &SaleCreated {
                sale_id: sid,
                creator: a(1),
                holding: a(2),
                collateral: tok,
                vt: 3,
                vc: 4,
                sale_reserve: 5,
                seed_reserve: 6,
                fee_rate: 7,
            },
            json!({"sale_id": hx(&sid), "creator": aid(&a(1)), "holding": aid(&a(2)), "collateral": asset_json(&tok),
                   "vt": "3", "vc": "4", "sale_reserve": "5", "seed_reserve": "6", "fee_rate": "7"}),
        ),
        v(
            "BuyPaid",
            &BuyPaid { sale_id: sid, buyer: a(3), collateral_in: 8, min_tokens_out: 9 },
            json!({"sale_id": hx(&sid), "buyer": aid(&a(3)), "collateral_in": "8", "min_tokens_out": "9"}),
        ),
        v(
            "FeesCollected",
            &FeesCollected { sale_id: sid, amount: 10, fee_treasury: a(4) },
            json!({"sale_id": hx(&sid), "amount": "10", "fee_treasury": aid(&a(4))}),
        ),
        v(
            "Withdrawn",
            &Withdrawn { sale_id: sid, amount: 11, destination: a(5) },
            json!({"sale_id": hx(&sid), "amount": "11", "destination": aid(&a(5))}),
        ),
    ];
    let selectors: Vec<Value> = EVENT_NAMES.iter().map(|n| json!({"name": n, "selector": hx(&event_selector(n))})).collect();
    let errs: Vec<Value> = errors::ALL.iter().map(|(c, n)| json!({"code": c, "name": n})).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "instructions": ix, "state": state, "events": events, "selectors": selectors, "errors": errs,
            "windows": {},
        }))
        .unwrap()
    );
}
