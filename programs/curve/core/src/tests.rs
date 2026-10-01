//! Host tests of plan and apply, without a zkVM. The executor tests run the
//! same logic inside the committed binary through the LEE state machine.

use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

const ME: AccountId = AccountId::new([7; 32]);
const CREATOR: AccountId = AccountId::new([1; 32]);
const BUYER: AccountId = AccountId::new([2; 32]);
const FEES: AccountId = AccountId::new([3; 32]);
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

fn input(accounts: Vec<AccountMeta>, ix: &Instruction) -> PlanInput {
    PlanInput {
        self_account_id: ME,
        caller_account_id: None,
        accounts,
        instruction_data: borsh::to_vec(ix).unwrap(),
    }
}

fn sale(vt: u128, vc: u128, reserve: u128, fee_rate: u128) -> Sale {
    new_sale(CREATOR, holding_account(&ME, &SID), &Asset::Native, vt, vc, reserve, 0, FEES, fee_rate)
}

fn buy(s: &Sale, c_in: u128, min: u128) -> Sale {
    let e = Effect::Buy { holding: holding_account(&ME, &SID), collateral: Asset::Native, collateral_in: c_in, min_tokens_out: min };
    Sale::try_from_slice(&apply(e, &store(s)).unwrap()).unwrap()
}

#[test]
fn seeds_follow_spels_rule() {
    assert_eq!(sale_seed(&SID), PdaSeed::new(SID));
    let mut both = SID.to_vec();
    both.extend_from_slice(&seed_from_str("holding"));
    assert_eq!(holding_seed(&SID), PdaSeed::new(sha256(&[&both])));
}

#[test]
fn a_creation_writes_the_curve_and_moves_nothing() {
    let ix = Instruction::CreateSale {
        sale_id: SID,
        collateral: Asset::Native,
        vt: 1_000_000,
        vc: 1_000,
        sale_reserve: 800_000,
        seed_reserve: 200_000,
        fee_treasury: FEES,
        fee_rate: 10_000,
    };
    let rows = vec![AccountMeta::new(sale_account(&ME, &SID), false, ME), AccountMeta::new(CREATOR, true, ME)];
    let out = plan(&input(rows, &ix), ix).output().clone();
    let Effect::Create(s) = borsh::from_slice(&out.effects[0].data).unwrap() else {
        panic!("not a creation")
    };
    assert_eq!((s.creator, s.treasury, s.seed_reserve, s.fee_rate), (CREATOR, holding_account(&ME, &SID), 200_000, 10_000));
    assert!(out.chained_calls.is_empty());
    assert_eq!(out.events[0].selector, event_selector("SaleCreated"));
}

#[test]
fn a_degenerate_curve_or_an_over_cap_fee_is_refused_at_creation() {
    assert!(refusal(|| sale(800_000, 1_000, 800_000, 0)).starts_with("E5001"));
    assert!(refusal(|| sale(1_000_000, 0, 800_000, 0)).starts_with("E5001"));
    assert!(refusal(|| sale(1_000_000, 1_000, 800_000, 10_001)).starts_with("E5010"));
}

#[test]
fn a_creation_over_an_existing_sale_is_refused() {
    let s = sale(1_000_000, 1_000, 800_000, 0);
    assert!(refusal(|| apply(Effect::Create(s.clone()), &store(&s))).starts_with("E5012"));
}

#[test]
fn a_buy_pays_the_holding_and_advances_the_curve_as_the_host_does() {
    let ix = Instruction::ExecuteBuy { sale_id: SID, collateral: Asset::Native, collateral_in: 2, min_tokens_out: 0 };
    let rows = vec![
        AccountMeta::new(sale_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(BUYER, true),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    let call = &out.chained_calls[0];
    assert_eq!(call.program_account_id, NATIVE_TOKEN_PROGRAM_ID);
    assert_eq!(call.shard_selectors[0].account_id, BUYER);
    assert_eq!(call.shard_selectors[1].account_id, holding_account(&ME, &SID));
    assert_eq!(
        borsh::from_slice::<native_token::Instruction>(&call.instruction_data).unwrap(),
        native_token::Instruction::Transfer { amount: 2 }
    );
    // The v0.2.4 on-chain buy: Curve::buy(2, 0) on (1e6, 1e3, 8e5).
    let after = buy(&sale(1_000_000, 1_000, 800_000, 0), 2, 0);
    assert_eq!((after.vt, after.vc, after.sale_reserve, after.real_collateral), (998_004, 1_002, 798_004, 2));
}

#[test]
fn the_fee_comes_off_before_pricing_and_accrues() {
    let after = buy(&sale(1_000_000, 1_000, 800_000, 10_000), 2, 0);
    assert_eq!((after.fees_accrued, after.real_collateral), (1, 1));
    let mut host = antumbra::Curve::new(1_000_000, 1_000, 800_000).unwrap();
    host.buy(1, 0).unwrap();
    assert_eq!(after.vt, host.vt);
}

#[test]
fn an_unsigned_buy_is_refused_in_the_plan() {
    let ix = Instruction::ExecuteBuy { sale_id: SID, collateral: Asset::Native, collateral_in: 2, min_tokens_out: 0 };
    let rows = vec![
        AccountMeta::new(sale_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(BUYER, false),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
    ];
    assert!(refusal(|| plan(&input(rows, &ix), ix.clone())).starts_with("E5014"));
}

#[test]
fn slippage_a_closed_sale_and_a_wrong_holding_are_refused() {
    let s = sale(1_000_000, 1_000, 800_000, 0);
    let h = holding_account(&ME, &SID);
    let e = |min| Effect::Buy { holding: h, collateral: Asset::Native, collateral_in: 2, min_tokens_out: min };
    assert!(refusal(|| apply(e(1_997), &store(&s))).starts_with("E5003"));
    let closed = Sale { sale_reserve: 0, ..s.clone() };
    assert!(refusal(|| apply(e(0), &store(&closed))).starts_with("E5004"));
    let wrong = Effect::Buy { holding: FEES, collateral: Asset::Native, collateral_in: 2, min_tokens_out: 0 };
    assert!(refusal(|| apply(wrong, &store(&s))).starts_with("E5008"));
    let tok = Effect::Buy { holding: h, collateral: Asset::Token { definition_id: FEES }, collateral_in: 2, min_tokens_out: 0 };
    assert!(refusal(|| apply(tok, &store(&s))).starts_with("E5015"));
    assert!(refusal(|| apply(e(0), &[])).starts_with("E5002"));
}

#[test]
fn fees_are_swept_exactly_once_to_the_fixed_treasury() {
    let s = buy(&sale(1_000_000, 1_000, 800_000, 10_000), 2, 0);
    let h = holding_account(&ME, &SID);
    let e = |to, amount| Effect::CollectFees { holding: h, fee_treasury: to, collateral: Asset::Native, amount };
    assert!(refusal(|| apply(e(BUYER, 1), &store(&s))).starts_with("E5009"));
    assert!(refusal(|| apply(e(FEES, 2), &store(&s))).starts_with("E5016"));
    let swept = Sale::try_from_slice(&apply(e(FEES, 1), &store(&s)).unwrap()).unwrap();
    assert_eq!(swept.fees_accrued, 0);
    assert!(refusal(|| apply(e(FEES, 1), &store(&swept))).starts_with("E5003"));
}

#[test]
fn the_creator_withdraws_the_raise_once_and_only_after_the_close() {
    let s = sale(1_000, 1, 500, 0);
    let h = holding_account(&ME, &SID);
    let e = |who, amount| Effect::Withdraw { creator: who, holding: h, collateral: Asset::Native, amount };
    assert!(refusal(|| apply(e(CREATOR, 1), &store(&s))).starts_with("E5004"));
    // The v0.2.4 close: one buy of 1 takes exactly the 500 on sale.
    let closed = buy(&s, 1, 0);
    assert_eq!((closed.sale_reserve, closed.real_collateral, withdrawable(&closed)), (0, 1, 1));
    assert!(refusal(|| apply(e(BUYER, 1), &store(&closed))).starts_with("E5011"));
    assert!(refusal(|| apply(e(CREATOR, 2), &store(&closed))).starts_with("E5016"));
    let paid = Sale::try_from_slice(&apply(e(CREATOR, 1), &store(&closed)).unwrap()).unwrap();
    assert_eq!(paid.withdrawn, 1);
    assert!(refusal(|| apply(e(CREATOR, 1), &store(&paid))).starts_with("E5003"));
}

#[test]
fn a_withdrawal_leaves_the_accrued_fee_behind() {
    // 2 in, 1 of it fee (rounded up), and the 1 left buys exactly the 500 on sale.
    let closed = buy(&sale(1_000, 1, 500, 10_000), 2, 0);
    assert_eq!(closed.sale_reserve, 0, "sized to close in one buy");
    assert_eq!((closed.fees_accrued, withdrawable(&closed)), (1, 1));
}

#[test]
fn payouts_carry_the_holding_seed() {
    let ix = Instruction::Withdraw { sale_id: SID, collateral: Asset::Native, amount: 5 };
    let rows = vec![
        AccountMeta::new(sale_account(&ME, &SID), false, ME),
        AccountMeta::native_balance(holding_account(&ME, &SID), false),
        AccountMeta::native_balance(CREATOR, false),
        AccountMeta::new(CREATOR, true, ME),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    assert_eq!(out.chained_calls[0].pda_seeds, vec![holding_seed(&SID)]);
    assert_eq!(out.events[0].selector, event_selector("Withdrawn"));
}
