//! What the committed v0.3 antumbra_curve binary accepts and refuses, run
//! through the LEE state machine with the real native and token programs. Each
//! refusal is asserted by its code, and every refused transaction is checked
//! to have moved nothing. The on-chain values are compared with
//! `antumbra::Curve` run on the host, as the v0.2.4 evidence was.

use antumbra::weighted::ONE;
use antumbra_curve_core::{event_selector, holding_account, rows, sale_account, Asset, Instruction, Sale};
use antumbra_executor_tests::{code, Chain, Key, CURVE, GAS_CAP};
use borsh::BorshDeserialize;
use lee::AccountId;

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
    w.chain.send_to(CURVE, ix, r, who, GAS_CAP)
}

fn create(w: &mut World, sid: [u8; 32], collateral: Asset, vt: u128, vc: u128, reserve: u128, seed: u128, fee_rate: u128) -> Result<(), String> {
    let creator = Key::new(1);
    let ix = Instruction::CreateSale { sale_id: sid, collateral, vt, vc, sale_reserve: reserve, seed_reserve: seed, fee_treasury: FEES, fee_rate };
    send(w, &ix, rows::create(&CURVE, &sid, creator.id), &[&creator])
}

fn buy(w: &mut World, sid: [u8; 32], collateral: Asset, c_in: u128, min: u128) -> Result<(), String> {
    let buyer = Key::new(2);
    let ix = Instruction::ExecuteBuy { sale_id: sid, collateral, collateral_in: c_in, min_tokens_out: min };
    send(w, &ix, rows::buy(&CURVE, &sid, &collateral, buyer.id), &[&buyer])
}

fn withdraw(w: &mut World, sid: [u8; 32], collateral: Asset, who: &Key, dest: AccountId, amount: u128) -> Result<(), String> {
    let ix = Instruction::Withdraw { sale_id: sid, collateral, amount };
    send(w, &ix, rows::withdraw(&CURVE, &sid, &collateral, dest, who.id), &[who])
}

fn collect(w: &mut World, sid: [u8; 32], collateral: Asset, to: AccountId, amount: u128) -> Result<(), String> {
    // Permissionless: no signer at all.
    let ix = Instruction::CollectFees { sale_id: sid, collateral, amount };
    send(w, &ix, rows::collect_fees(&CURVE, &sid, &collateral, to), &[])
}

fn sale(w: &World, sid: &[u8; 32]) -> Sale {
    Sale::try_from_slice(&w.chain.shard(sale_account(&CURVE, sid), CURVE)).expect("a sale")
}

#[track_caller]
fn refused(r: Result<(), String>, want: u32) {
    let err = r.expect_err("expected a refusal");
    assert_eq!(code(&err), Some(want), "wrong refusal: {err}");
}

#[test]
fn a_paid_buy_debits_the_buyer_credits_the_holding_and_matches_the_host() {
    // The v0.2.4 paid run: Vt 1e6, Vc 1e3, reserve 8e5; a buyer of 4 buys 2.
    let mut w = world(4);
    let sid = [1; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 200_000, 0).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("SaleCreated"));
    let hold = holding_account(&CURVE, &sid);
    buy(&mut w, sid, Asset::Native, 2, 0).unwrap();
    assert_eq!(w.chain.events[0].event.selector, event_selector("BuyPaid"));
    assert_eq!((w.chain.native(Key::new(2).id), w.chain.native(hold)), (2, 2));
    let s = sale(&w, &sid);
    let mut host = antumbra::Curve::new(1_000_000, 1_000, 800_000).unwrap();
    let out = host.buy(2, 0).unwrap();
    assert_eq!(out, 1_996, "the residue stays with the pool");
    assert_eq!((s.vt, s.vc, s.sale_reserve, s.real_collateral), (host.vt, host.vc, host.sale_reserve, host.real_collateral));
    assert_eq!((s.vt, s.vc, s.sale_reserve, s.real_collateral, s.seed_reserve), (998_004, 1_002, 798_004, 2, 200_000));
}

#[test]
fn an_eighteen_decimal_sale_prices_without_forming_k() {
    // The v0.2.4 run at 1e24 / 1e21: k = 1e45 does not fit a u128.
    let mut w = world(1 << 100);
    let sid = [2; 32];
    create(&mut w, sid, Asset::Native, 1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE, 200_000 * ONE, 0).unwrap();
    buy(&mut w, sid, Asset::Native, 500 * ONE, 0).unwrap();
    let s = sale(&w, &sid);
    assert_eq!(s.vt, 666_666_666_666_666_666_666_667);
    assert_eq!(s.vc, 1_500 * ONE);
    assert_eq!(s.sale_reserve, 466_666_666_666_666_666_666_667);
    assert_eq!(s.real_collateral, 500 * ONE);
    assert_eq!(s.seed_reserve, 200_000 * ONE, "a buy never touches the seed reserve");
    assert_eq!(w.chain.native(holding_account(&CURVE, &sid)), 500 * ONE);
}

#[test]
fn slippage_is_refused_and_leaves_the_sale_byte_identical() {
    let mut w = world(10);
    let sid = [3; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 0, 0).unwrap();
    let before = w.chain.shard(sale_account(&CURVE, &sid), CURVE);
    refused(buy(&mut w, sid, Asset::Native, 2, 1_997), 5003);
    assert_eq!(w.chain.shard(sale_account(&CURVE, &sid), CURVE), before);
    assert_eq!(w.chain.native(Key::new(2).id), 10, "the payment went with the refusal");
}

#[test]
fn the_fee_accrues_and_is_swept_exactly_once() {
    let mut w = world(3);
    let sid = [4; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 0, 10_000).unwrap();
    let hold = holding_account(&CURVE, &sid);
    buy(&mut w, sid, Asset::Native, 2, 0).unwrap();
    // The v0.2.4 run: buyer 3 -> 1, holding 0 -> 2, real_collateral 1.
    assert_eq!((w.chain.native(Key::new(2).id), w.chain.native(hold)), (1, 2));
    let s = sale(&w, &sid);
    assert_eq!((s.real_collateral, s.fees_accrued), (1, 1));
    refused(collect(&mut w, sid, Asset::Native, FEES, 2), 5016);
    refused(collect(&mut w, sid, Asset::Native, Key::new(3).id, 1), 5009);
    collect(&mut w, sid, Asset::Native, FEES, 1).unwrap();
    assert_eq!((w.chain.native(hold), w.chain.native(FEES)), (1, 1));
    refused(collect(&mut w, sid, Asset::Native, FEES, 1), 5003);
    assert_eq!(w.chain.native(FEES), 1);
}

#[test]
fn the_creator_is_paid_once_and_only_after_the_close() {
    let mut w = world(5);
    let sid = [5; 32];
    let creator = Key::new(1);
    let dest = AccountId::new([0xC0; 32]);
    // The v0.2.4 close: Vt 1000, Vc 1, reserve 500; one buy of 1 takes all 500.
    create(&mut w, sid, Asset::Native, 1_000, 1, 500, 0, 0).unwrap();
    refused(withdraw(&mut w, sid, Asset::Native, &creator, dest, 1), 5004);
    buy(&mut w, sid, Asset::Native, 1, 0).unwrap();
    assert_eq!(sale(&w, &sid).sale_reserve, 0);
    refused(buy(&mut w, sid, Asset::Native, 1, 0), 5004);
    assert_eq!(w.chain.native(Key::new(2).id), 4, "a refused buy is not paid");
    refused(withdraw(&mut w, sid, Asset::Native, &Key::new(3), dest, 1), 5011);
    refused(withdraw(&mut w, sid, Asset::Native, &creator, dest, 2), 5016);
    withdraw(&mut w, sid, Asset::Native, &creator, dest, 1).unwrap();
    assert_eq!((w.chain.native(dest), w.chain.native(holding_account(&CURVE, &sid))), (1, 0));
    refused(withdraw(&mut w, sid, Asset::Native, &creator, dest, 1), 5003);
}

#[test]
fn a_withdrawal_leaves_the_accrued_fee_in_the_holding() {
    let mut w = world(5);
    let sid = [6; 32];
    let creator = Key::new(1);
    create(&mut w, sid, Asset::Native, 1_000, 1, 500, 0, 10_000).unwrap();
    buy(&mut w, sid, Asset::Native, 2, 0).unwrap(); // fee 1, the other 1 closes the sale
    withdraw(&mut w, sid, Asset::Native, &creator, creator.id, 1).unwrap();
    assert_eq!(w.chain.native(holding_account(&CURVE, &sid)), 1, "the protocol's fee is still there");
    collect(&mut w, sid, Asset::Native, FEES, 1).unwrap();
    assert_eq!(w.chain.native(holding_account(&CURVE, &sid)), 0);
}

#[test]
fn a_sale_id_is_used_once_and_a_bad_curve_is_refused_at_creation() {
    let mut w = world(5);
    let sid = [7; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 0, 0).unwrap();
    refused(create(&mut w, sid, Asset::Native, 2_000_000, 1_000, 800_000, 0, 0), 5012);
    assert_eq!(sale(&w, &sid).vt, 1_000_000);
    refused(create(&mut w, [8; 32], Asset::Native, 800_000, 1_000, 800_000, 0, 0), 5001);
    refused(create(&mut w, [8; 32], Asset::Native, 1_000_000, 1_000, 800_000, 0, 10_001), 5010);
    let impostor = Key::new(3);
    let ix = Instruction::CreateSale {
        sale_id: [9; 32],
        collateral: Asset::Native,
        vt: 1_000_000,
        vc: 1_000,
        sale_reserve: 800_000,
        seed_reserve: 0,
        fee_treasury: FEES,
        fee_rate: 0,
    };
    refused(send(&mut w, &ix, rows::create(&CURVE, &[9; 32], impostor.id), &[]), 5014);
}

#[test]
fn a_buyer_who_cannot_pay_moves_nothing() {
    let mut w = world(1);
    let sid = [10; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 0, 0).unwrap();
    let before = w.chain.shard(sale_account(&CURVE, &sid), CURVE);
    let e = buy(&mut w, sid, Asset::Native, 2, 0).unwrap_err();
    assert!(e.contains("execution rules"), "{e}");
    assert_eq!(w.chain.shard(sale_account(&CURVE, &sid), CURVE), before);
}

#[test]
fn a_buy_cannot_pay_another_account_or_skip_its_signature() {
    let mut w = world(10);
    let sid = [11; 32];
    create(&mut w, sid, Asset::Native, 1_000_000, 1_000, 800_000, 0, 0).unwrap();
    let buyer = Key::new(2);
    let ix = Instruction::ExecuteBuy { sale_id: sid, collateral: Asset::Native, collateral_in: 2, min_tokens_out: 0 };
    let mut r = rows::buy(&CURVE, &sid, &Asset::Native, buyer.id);
    // The proceeds pointed at an account of the buyer's choosing.
    r[2] = lee::ProgramShardSelector::new(Key::new(3).id, Asset::Native.shard_program());
    refused(send(&mut w, &ix, r, &[&buyer]), 5008);
    refused(send(&mut w, &ix, rows::buy(&CURVE, &sid, &Asset::Native, buyer.id), &[]), 5014);
    let tok = Asset::Token { definition_id: w.def };
    let ix2 = Instruction::ExecuteBuy { sale_id: sid, collateral: tok, collateral_in: 2, min_tokens_out: 0 };
    refused(send(&mut w, &ix2, rows::buy(&CURVE, &sid, &tok, buyer.id), &[&buyer]), 5015);
    assert_eq!(w.chain.native(buyer.id), 10);
}

#[test]
fn a_token_collateral_sale_takes_and_pays_the_token() {
    let mut w = world(10);
    let sid = [12; 32];
    let tok = Asset::Token { definition_id: w.def };
    let creator = Key::new(1);
    create(&mut w, sid, tok, 1_000, 1, 500, 0, 10_000).unwrap();
    let hold = holding_account(&CURVE, &sid);
    buy(&mut w, sid, tok, 2, 0).unwrap();
    assert_eq!((w.chain.token(Key::new(2).id), w.chain.token(hold)), (999_998, 2));
    assert_eq!(w.chain.native(Key::new(2).id), 10, "no native balance moved");
    let dest = AccountId::new([0xC1; 32]); // no holding yet: the token program creates it
    withdraw(&mut w, sid, tok, &creator, dest, 1).unwrap();
    collect(&mut w, sid, tok, FEES, 1).unwrap();
    assert_eq!((w.chain.token(dest), w.chain.token(FEES), w.chain.token(hold)), (1, 1, 0));
}
