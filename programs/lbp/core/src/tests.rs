//! Host tests of plan and apply, without a zkVM. The executor tests run the
//! same logic inside the committed binary through the LEE state machine.

use super::*;
use antumbra::weighted::ONE;
use std::panic::{catch_unwind, AssertUnwindSafe};

const ME: AccountId = AccountId::new([7; 32]);
const CREATOR: AccountId = AccountId::new([1; 32]);
const BUYER: AccountId = AccountId::new([2; 32]);
const FEES: AccountId = AccountId::new([3; 32]);
const PID: [u8; 32] = [9; 32];
const W99: u128 = 990_000_000_000_000_000;
const W01: u128 = 10_000_000_000_000_000;

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

/// The v0.2.4 pool: 99/1 shifting to 1/99 over 10,000 time units.
fn pool(rt: u128, rc: u128, fee_rate: u128) -> Pool {
    new_pool(CREATOR, holding_account(&ME, &PID), &Asset::Native, rt, rc, W99, W01, 1_000, 11_000, FEES, fee_rate)
}

fn buy_effect(now: u64, c_in: u128, min: u128) -> Effect {
    Effect::Buy { holding: holding_account(&ME, &PID), collateral: Asset::Native, now, collateral_in: c_in, min_tokens_out: min }
}

fn applied(e: Effect, p: &Pool) -> Pool {
    Pool::try_from_slice(&apply(e, &store(p)).unwrap()).unwrap()
}

#[test]
fn seeds_follow_spels_rule() {
    assert_eq!(pool_seed(&PID), PdaSeed::new(PID));
    let mut both = PID.to_vec();
    both.extend_from_slice(&seed_from_str("holding"));
    assert_eq!(holding_seed(&PID), PdaSeed::new(sha256(&[&both])));
}

#[test]
fn a_degenerate_pool_is_refused_at_creation() {
    assert!(refusal(|| new_pool(CREATOR, FEES, &Asset::Native, 1, 1, W99, W01, 5, 5, FEES, 0)).starts_with("E6001"));
    assert!(refusal(|| pool(0, 1_000, 0)).starts_with("E6001"));
    assert!(refusal(|| pool(1_000_000, 1_000, 50_001)).starts_with("E6001"));
    let p = pool(1_000_000, 1_000, 0);
    assert!(refusal(|| apply(Effect::Create(p.clone()), &store(&p))).starts_with("E6012"));
}

#[test]
fn a_buy_is_paid_and_priced_at_the_scheduled_weight_as_on_v024() {
    // The paying v0.2.4 run: buy 1 at t = 3500 of (1e6, 1e3), 342 tokens out.
    let after = applied(buy_effect(3_500, 1, 0), &pool(1_000_000, 1_000, 0));
    assert_eq!((after.reserve_token, after.reserve_collateral, after.raised, after.last_seen), (999_658, 1_001, 1, 3_500));
    // The 18-decimal run: 5e21 into (1e24, 1e23) at the same weight.
    let big = applied(buy_effect(3_500, 5_000 * ONE, 0), &pool(1_000_000 * ONE, 100_000 * ONE, 0));
    assert_eq!(1_000_000 * ONE - big.reserve_token, 16_561_317_272_817_989_000_000);
    assert_eq!(big.reserve_collateral, 105_000 * ONE);
}

#[test]
fn the_plan_pays_the_holding_inside_a_two_minute_window() {
    let ix = Instruction::ExecuteBuy { pool_id: PID, collateral: Asset::Native, now: 5_000, collateral_in: 7, min_tokens_out: 0 };
    let rows = vec![
        AccountMeta::new(pool_account(&ME, &PID), false, ME),
        AccountMeta::native_balance(BUYER, true),
        AccountMeta::native_balance(holding_account(&ME, &PID), false),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    assert_eq!(out.timestamp_validity_window.start(), Some(5_000));
    assert_eq!(out.timestamp_validity_window.end(), Some(5_000 + BUY_WINDOW_MS));
    let call = &out.chained_calls[0];
    assert_eq!((call.shard_selectors[0].account_id, call.shard_selectors[1].account_id), (BUYER, holding_account(&ME, &PID)));
    assert_eq!(
        borsh::from_slice::<native_token::Instruction>(&call.instruction_data).unwrap(),
        native_token::Instruction::Transfer { amount: 7 }
    );
    assert_eq!(out.events[0].selector, event_selector("BuyPaid"));
}

#[test]
fn slippage_pause_and_a_backward_time_are_refused() {
    let p = pool(1_000_000, 1_000, 0);
    assert!(refusal(|| apply(buy_effect(3_500, 1, 343), &store(&p))).starts_with("E6004"));
    let paused = Pool { paused: 1, ..p.clone() };
    assert!(refusal(|| apply(buy_effect(3_500, 1, 0), &store(&paused))).starts_with("E6005"));
    let later = applied(buy_effect(3_500, 1, 0), &p);
    assert!(refusal(|| apply(buy_effect(3_499, 1, 0), &store(&later))).starts_with("E6006"));
    let wrong = Effect::Buy { holding: FEES, collateral: Asset::Native, now: 3_500, collateral_in: 1, min_tokens_out: 0 };
    assert!(refusal(|| apply(wrong, &store(&p))).starts_with("E6010"));
}

#[test]
fn only_the_creator_pauses() {
    let p = pool(1_000_000, 1_000, 0);
    assert!(refusal(|| apply(Effect::SetPaused { creator: BUYER, paused: 1 }, &store(&p))).starts_with("E6011"));
    assert_eq!(applied(Effect::SetPaused { creator: CREATOR, paused: 1 }, &p).paused, 1);
}

#[test]
fn the_creator_withdraws_after_the_end_net_of_the_at_close_fee() {
    let p = applied(buy_effect(3_500, 100, 0), &pool(1_000_000, 1_000, 50_000));
    assert_eq!(withdrawal(&p), Ok((100, 5, 95)));
    let w = |who, now, amount, fee| Effect::Withdraw {
        creator: who,
        holding: holding_account(&ME, &PID),
        fee_treasury: FEES,
        collateral: Asset::Native,
        now,
        amount,
        fee,
    };
    assert!(refusal(|| apply(w(CREATOR, 10_999, 100, 5), &store(&p))).starts_with("E6017"));
    assert!(refusal(|| apply(w(BUYER, 11_000, 100, 5), &store(&p))).starts_with("E6011"));
    assert!(refusal(|| apply(w(CREATOR, 11_000, 100, 4), &store(&p))).starts_with("E6016"));
    assert!(refusal(|| apply(w(CREATOR, 11_000, 99, 5), &store(&p))).starts_with("E6016"));
    let done = applied(w(CREATOR, 11_000, 100, 5), &p);
    assert_eq!(done.withdrawn, 100);
    assert!(refusal(|| apply(w(CREATOR, 11_000, 100, 5), &store(&done))).starts_with("E6003"));
}

#[test]
fn a_one_unit_raise_is_all_fee() {
    // The v0.2.4 close: 1 raised at 5%, rounded up, leaves the creator nothing.
    let p = applied(buy_effect(3_500, 1, 0), &pool(1_000_000, 1_000, 50_000));
    assert_eq!(withdrawal(&p), Ok((1, 1, 0)));
}

#[test]
fn a_withdrawal_pays_creator_and_fee_treasury_out_of_the_holding_seed() {
    let ix = Instruction::Withdraw { pool_id: PID, collateral: Asset::Native, now: 11_000, amount: 100, fee: 5 };
    let rows = vec![
        AccountMeta::new(pool_account(&ME, &PID), false, ME),
        AccountMeta::native_balance(holding_account(&ME, &PID), false),
        AccountMeta::native_balance(CREATOR, false),
        AccountMeta::native_balance(FEES, false),
        AccountMeta::new(CREATOR, true, ME),
    ];
    let out = plan(&input(rows, &ix), ix).output().clone();
    assert_eq!(out.chained_calls.len(), 2);
    for (call, (to, amount)) in out.chained_calls.iter().zip([(CREATOR, 95), (FEES, 5)]) {
        assert_eq!(call.pda_seeds, vec![holding_seed(&PID)]);
        assert_eq!(call.shard_selectors[1].account_id, to);
        assert_eq!(
            borsh::from_slice::<native_token::Instruction>(&call.instruction_data).unwrap(),
            native_token::Instruction::Transfer { amount }
        );
    }
    assert_eq!(out.timestamp_validity_window.start(), Some(11_000));
    assert_eq!(out.timestamp_validity_window.end(), None);
}
