//! What the committed v0.3 antumbra_lbp binary accepts and refuses, run
//! through the LEE state machine with the real native and token programs and
//! the block timestamp checked against the windows the plan sets. The on-chain
//! values are compared with `antumbra::binfixed::weighted_buy` on the host, as
//! the v0.2.4 evidence was.

use antumbra::weighted::{weight_at, ONE};
use antumbra_executor_tests::{code, Chain, Key, GAS_CAP, LBP};
use antumbra_lbp_core::{event_selector, holding_account, pool_account, rows, Asset, Instruction, Pool, BUY_WINDOW_MS};
use borsh::BorshDeserialize;
use lee::AccountId;

const T0: u64 = 1_700_000_000_000;
/// The v0.2.4 schedule ran over 10,000 seconds; here in milliseconds.
const SPAN: u64 = 10_000_000;
const W99: u128 = 990_000_000_000_000_000;
const W01: u128 = 10_000_000_000_000_000;
const FEES: AccountId = AccountId::new([0xFE; 32]);

struct World {
    chain: Chain,
    def: AccountId,
}

fn world(buyer_native: u128) -> World {
    let def = AccountId::new([0xDE; 32]);
    let chain = Chain::new(
        &[(Key::new(1).id, 1_000), (Key::new(2).id, buyer_native), (Key::new(3).id, 10)],
        &[(Key::new(2).id, def, 1_000_000)],
    );
    World { chain, def }
}

fn send(w: &mut World, ix: &Instruction, r: Vec<lee::ProgramShardSelector>, who: &[&Key]) -> Result<(), String> {
    w.chain.send_to(LBP, ix, r, who, GAS_CAP)
}

fn create(w: &mut World, pid: [u8; 32], collateral: Asset, rt: u128, rc: u128, fee_rate: u128) -> Result<(), String> {
    let creator = Key::new(1);
    let ix = Instruction::CreatePool {
        pool_id: pid,
        collateral,
        reserve_token: rt,
        reserve_collateral: rc,
        w_start: W99,
        w_end: W01,
        t_start: T0,
        t_end: T0 + SPAN,
        fee_treasury: FEES,
        fee_rate,
    };
    send(w, &ix, rows::handle(&LBP, &pid, creator.id), &[&creator])
}

fn buy(w: &mut World, pid: [u8; 32], collateral: Asset, now: u64, c_in: u128, min: u128) -> Result<(), String> {
    let buyer = Key::new(2);
    let ix = Instruction::ExecuteBuy { pool_id: pid, collateral, now, collateral_in: c_in, min_tokens_out: min };
    send(w, &ix, rows::buy(&LBP, &pid, &collateral, buyer.id), &[&buyer])
}

fn withdraw(w: &mut World, pid: [u8; 32], collateral: Asset, who: &Key, now: u64, amount: u128, fee: u128) -> Result<(), String> {
    let ix = Instruction::Withdraw { pool_id: pid, collateral, now, amount, fee };
    let dest = AccountId::new([0xC0; 32]);
    send(w, &ix, rows::withdraw(&LBP, &pid, &collateral, dest, FEES, who.id), &[who])
}

fn pause(w: &mut World, pid: [u8; 32], who: &Key, paused: u8) -> Result<(), String> {
    let ix = Instruction::SetPaused { pool_id: pid, paused };
    send(w, &ix, rows::handle(&LBP, &pid, who.id), &[who])
}

fn pool(w: &World, pid: &[u8; 32]) -> Pool {
    Pool::try_from_slice(&w.chain.shard(pool_account(&LBP, pid), LBP)).expect("a pool")
}

#[track_caller]
fn refused(r: Result<(), String>, want: u32) {
    let err = r.expect_err("expected a refusal");
    assert_eq!(code(&err), Some(want), "wrong refusal: {err}");
}

#[test]
fn a_paid_buy_is_priced_at_the_scheduled_weight_and_matches_the_host() {
    // The v0.2.4 paid run: reserves 1e6 / 1e3, a schedule from t = 1000 to
    // 11000 bought at t = 3500, a quarter of the way through.
    let mut w = world(2);
    let pid = [1; 32];
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 0).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("PoolCreated"));
    let at = T0 + 2_500_000;
    w.chain.now = at;
    buy(&mut w, pid, Asset::Native, at, 1, 0).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("BuyPaid"));
    let hold = holding_account(&LBP, &pid);
    assert_eq!((w.chain.native(Key::new(2).id), w.chain.native(hold)), (1, 1));
    let wt = weight_at(W99, W01, T0, T0 + SPAN, at).unwrap();
    assert_eq!(wt, 745_000_000_000_000_000, "the weight is derived, 0.745");
    let out = antumbra::binfixed::weighted_buy(1_000_000, 1_000, 1, wt, ONE - wt).unwrap();
    assert_eq!(out, 342);
    let p = pool(&w, &pid);
    assert_eq!((p.reserve_token, p.reserve_collateral, p.raised, p.last_seen), (1_000_000 - out, 1_001, 1, at));
}

#[test]
fn the_fractional_power_runs_in_the_guest_at_eighteen_decimals() {
    // The v0.2.4 run: 5e21 into 1e24 / 1e23 at 0.745.
    let mut w = world(1 << 100);
    let pid = [2; 32];
    create(&mut w, pid, Asset::Native, 1_000_000 * ONE, 100_000 * ONE, 0).unwrap();
    let at = T0 + 2_500_000;
    w.chain.now = at + 1_000;
    buy(&mut w, pid, Asset::Native, at, 5_000 * ONE, 0).unwrap();
    let p = pool(&w, &pid);
    assert_eq!(1_000_000 * ONE - p.reserve_token, 16_561_317_272_817_989_000_000);
    assert_eq!(p.reserve_collateral, 105_000 * ONE);
    assert_eq!(w.chain.native(holding_account(&LBP, &pid)), 5_000 * ONE);
}

#[test]
fn a_buy_cannot_name_a_future_time_nor_one_past_its_window() {
    let mut w = world(10);
    let pid = [3; 32];
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 0).unwrap();
    w.chain.now = T0 + 3_500_000;
    let e = buy(&mut w, pid, Asset::Native, T0 + 9_000_000, 1, 0).unwrap_err();
    assert!(e.contains("validity window"), "{e}");
    let e = buy(&mut w, pid, Asset::Native, T0 + 3_500_000 - BUY_WINDOW_MS, 1, 0).unwrap_err();
    assert!(e.contains("validity window"), "{e}");
    buy(&mut w, pid, Asset::Native, T0 + 3_500_000 - BUY_WINDOW_MS + 1, 1, 0).unwrap();
    buy(&mut w, pid, Asset::Native, T0 + 3_500_000, 1, 0).unwrap();
    // And never earlier than a time the pool has already honoured, even inside the window.
    refused(buy(&mut w, pid, Asset::Native, T0 + 3_500_000 - 1, 1, 0), 6006);
}

#[test]
fn slippage_is_refused_and_leaves_the_pool_byte_identical() {
    let mut w = world(10);
    let pid = [4; 32];
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 0).unwrap();
    let at = T0 + 2_500_000;
    w.chain.now = at;
    let before = w.chain.shard(pool_account(&LBP, &pid), LBP);
    refused(buy(&mut w, pid, Asset::Native, at, 1, 343), 6004);
    assert_eq!(w.chain.shard(pool_account(&LBP, &pid), LBP), before);
    assert_eq!(w.chain.native(Key::new(2).id), 10);
}

#[test]
fn pausing_halts_buys_but_not_the_weight() {
    let mut w = world(10);
    let pid = [5; 32];
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 0).unwrap();
    refused(pause(&mut w, pid, &Key::new(3), 1), 6011);
    pause(&mut w, pid, &Key::new(1), 1).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("PauseSet"));
    w.chain.now = T0 + 5_000_000;
    refused(buy(&mut w, pid, Asset::Native, T0 + 5_000_000, 1, 0), 6005);
    pause(&mut w, pid, &Key::new(1), 0).unwrap();
    buy(&mut w, pid, Asset::Native, T0 + 5_000_000, 1, 0).unwrap();
    // 0.500 at t = 6000 (half way), as on v0.2.4: the pause did not stop the clock.
    let wt = weight_at(W99, W01, T0, T0 + SPAN, T0 + 5_000_000).unwrap();
    assert_eq!(wt, 500_000_000_000_000_000);
    let out = antumbra::binfixed::weighted_buy(1_000_000, 1_000, 1, wt, ONE - wt).unwrap();
    assert_eq!(pool(&w, &pid).reserve_token, 1_000_000 - out);
}

#[test]
fn the_creator_withdraws_after_the_end_net_of_the_at_close_fee() {
    let mut w = world(200);
    let pid = [6; 32];
    let creator = Key::new(1);
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 50_000).unwrap();
    w.chain.now = T0 + 3_500_000;
    buy(&mut w, pid, Asset::Native, T0 + 3_500_000, 100, 0).unwrap();
    let hold = holding_account(&LBP, &pid);
    assert_eq!(w.chain.native(hold), 100);
    // Before t_end, even at the current block time.
    refused(withdraw(&mut w, pid, Asset::Native, &creator, T0 + 3_500_000, 100, 5), 6017);
    // A withdrawal cannot name a time the chain has not reached.
    let e = withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 100, 5).unwrap_err();
    assert!(e.contains("validity window"), "{e}");
    w.chain.now = T0 + SPAN + 1_000;
    refused(withdraw(&mut w, pid, Asset::Native, &Key::new(3), T0 + SPAN, 100, 5), 6011);
    refused(withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 100, 4), 6016);
    refused(withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 101, 5), 6016);
    assert_eq!(w.chain.native(hold), 100);
    withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 100, 5).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("Withdrawn"));
    let dest = AccountId::new([0xC0; 32]);
    assert_eq!((w.chain.native(dest), w.chain.native(FEES), w.chain.native(hold)), (95, 5, 0));
    refused(withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 100, 5), 6003);
}

#[test]
fn a_one_unit_raise_is_all_fee_as_on_v024() {
    let mut w = world(10);
    let pid = [7; 32];
    let creator = Key::new(1);
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 50_000).unwrap();
    w.chain.now = T0 + 3_500_000;
    buy(&mut w, pid, Asset::Native, T0 + 3_500_000, 1, 0).unwrap();
    w.chain.now = T0 + SPAN;
    withdraw(&mut w, pid, Asset::Native, &creator, T0 + SPAN, 1, 1).unwrap();
    assert_eq!((w.chain.native(AccountId::new([0xC0; 32])), w.chain.native(FEES)), (0, 1));
}

#[test]
fn a_pool_id_is_used_once_and_a_bad_pool_is_refused_at_creation() {
    let mut w = world(10);
    let pid = [8; 32];
    create(&mut w, pid, Asset::Native, 1_000_000, 1_000, 0).unwrap();
    refused(create(&mut w, pid, Asset::Native, 5, 5, 0), 6012);
    assert_eq!(pool(&w, &pid).reserve_token, 1_000_000);
    refused(create(&mut w, [9; 32], Asset::Native, 0, 1_000, 0), 6001);
    refused(create(&mut w, [9; 32], Asset::Native, 1_000_000, 1_000, 50_001), 6001);
}

#[test]
fn a_token_collateral_pool_takes_and_pays_the_token() {
    let mut w = world(10);
    let pid = [10; 32];
    let tok = Asset::Token { definition_id: w.def };
    let creator = Key::new(1);
    create(&mut w, pid, tok, 1_000_000, 1_000, 50_000).unwrap();
    w.chain.now = T0 + 3_500_000;
    buy(&mut w, pid, tok, T0 + 3_500_000, 1_000, 0).unwrap();
    let hold = holding_account(&LBP, &pid);
    assert_eq!((w.chain.token(Key::new(2).id), w.chain.token(hold)), (999_000, 1_000));
    refused(buy(&mut w, pid, Asset::Native, T0 + 3_500_000, 1, 0), 6015);
    w.chain.now = T0 + SPAN;
    withdraw(&mut w, pid, tok, &creator, T0 + SPAN, 1_000, 50).unwrap();
    assert_eq!((w.chain.token(AccountId::new([0xC0; 32])), w.chain.token(FEES), w.chain.token(hold)), (950, 50, 0));
}
