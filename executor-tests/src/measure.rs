//! Whole-transaction cycle cost of each vesting instruction on LEZ v0.3, and
//! the largest batch that fits the 10,000,000 execution-gas cap.
//!
//! Cycles are what the state machine meters: the plan, every apply and every
//! chained call, summed. Gas is cycles one for one, so the "share" column is
//! the share of the most any single transaction may declare.
//!
//! `cargo run --release --bin measure`. CI diffs the table against CYCLES.md.

use antumbra_executor_tests::{Chain, Key, GAS_CAP, VESTING};
use antumbra_vesting_core::{batch_schedule_id, rows, Asset, Instruction, Terms};
use lee::AccountId;

const T0: u64 = 1_700_000_000_000;

fn terms(kind: u8, refund: AccountId) -> Terms {
    Terms {
        kind,
        start: T0,
        cliff: if kind == 0 { T0 + 60_000 } else { T0 },
        end: T0 + 1_800_000,
        total: 600,
        tranches: if kind == 2 { 4 } else { 0 },
        cancelable: true,
        transferable: true,
        cancel_authority: AccountId::default(),
        milestone_authority: AccountId::default(),
        refund_to: refund,
    }
}

fn row(label: &str, cycles: u64) {
    let share = cycles as f64 * 100.0 / GAS_CAP as f64;
    println!("| {label} | {cycles} | {share:.2}% |");
}

/// A fresh chain with a creator rich enough for any batch.
fn chain(def: AccountId, creator: &Key) -> Chain {
    Chain::new(&[(creator.id, 1 << 100)], &[(creator.id, def, 1 << 100)])
}

fn batch_fits(n: u32) -> Result<u64, String> {
    let creator = Key::new(1);
    let def = AccountId::new([0xDE; 32]);
    let mut c = chain(def, &creator);
    let bid = [0xBB; 32];
    let ix = Instruction::CreateScheduleBatch {
        batch_id: bid,
        beneficiaries: (0..n).map(|i| AccountId::new([(i % 251) as u8 + 1; 32])).collect(),
        asset: Asset::Native,
        terms: terms(1, AccountId::new([0xAA; 32])),
    };
    c.send(&ix, rows::batch(&VESTING, &bid, n, &Asset::Native, creator.id), &[&creator])?;
    Ok(c.last_cycles)
}

fn main() {
    let creator = Key::new(1);
    let ben = Key::new(2);
    let refund = AccountId::new([0xAA; 32]);
    let def = AccountId::new([0xDE; 32]);
    let token = Asset::Token { definition_id: def };
    let mut c = chain(def, &creator);

    println!("| op | cycles | share of the 10M gas cap |");
    println!("|---|---:|---:|");

    let create = |c: &mut Chain, sid: [u8; 32], asset: Asset, kind: u8| {
        let ix = Instruction::CreateSchedule { schedule_id: sid, beneficiary: ben.id, asset, terms: terms(kind, refund) };
        c.send(&ix, rows::create(&VESTING, &sid, &asset, creator.id), &[&creator]).unwrap();
        c.last_cycles
    };
    row("create schedule (native, funded)", create(&mut c, [1; 32], Asset::Native, 1));
    row("create schedule (token, funded)", create(&mut c, [2; 32], token, 1));
    create(&mut c, [3; 32], Asset::Native, 2);

    c.now = T0 + 120_000;
    let at = c.now;
    let claim = Instruction::Claim { schedule_id: [1; 32], batch_id: None, asset: Asset::Native, amount: 40, at };
    c.send(&claim, rows::claim(&VESTING, &[1; 32], None, &Asset::Native, AccountId::new([0xB0; 32]), ben.id), &[&ben]).unwrap();
    row("claim (native)", c.last_cycles);
    let claim = Instruction::Claim { schedule_id: [2; 32], batch_id: None, asset: token, amount: 40, at };
    c.send(&claim, rows::claim(&VESTING, &[2; 32], None, &token, AccountId::new([0xB1; 32]), ben.id), &[&ben]).unwrap();
    row("claim (token)", c.last_cycles);
    let cancel = Instruction::Cancel { schedule_id: [1; 32], batch_id: None, asset: Asset::Native, at, refund: 560 };
    c.send(&cancel, rows::cancel(&VESTING, &[1; 32], None, &Asset::Native, refund, creator.id), &[&creator]).unwrap();
    row("cancel (native)", c.last_cycles);
    let cancel = Instruction::Cancel { schedule_id: [2; 32], batch_id: None, asset: token, at, refund: 560 };
    c.send(&cancel, rows::cancel(&VESTING, &[2; 32], None, &token, refund, creator.id), &[&creator]).unwrap();
    row("cancel (token)", c.last_cycles);
    let sig = Instruction::SignalMilestone { schedule_id: [3; 32], index: 0 };
    c.send(&sig, rows::handle(&VESTING, &[3; 32], creator.id), &[&creator]).unwrap();
    row("signal milestone", c.last_cycles);
    let tr = Instruction::TransferBeneficiary { schedule_id: [3; 32], new_beneficiary: refund };
    c.send(&tr, rows::handle(&VESTING, &[3; 32], ben.id), &[&ben]).unwrap();
    row("transfer beneficiary", c.last_cycles);
    let nc = Instruction::MakeNonCancelable { schedule_id: [3; 32] };
    c.send(&nc, rows::handle(&VESTING, &[3; 32], creator.id), &[&creator]).unwrap();
    row("make non-cancelable", c.last_cycles);
    row("create a batch of 8 schedules", batch_fits(8).unwrap());

    // The ceiling: the largest n whose creation fits the cap, by bisection.
    let (mut lo, mut hi) = (1u32, 512u32);
    assert!(batch_fits(lo).is_ok());
    assert!(batch_fits(hi).is_err(), "512 schedules fit the cap; raise the bound");
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if batch_fits(mid).is_ok() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let at_max = batch_fits(lo).unwrap();
    let over = batch_fits(hi).unwrap_err();
    println!();
    println!("batch ceiling: {lo} schedules fit ({at_max} cycles), {hi} do not ({})", over.lines().next().unwrap_or(""));
    // Sanity: a batch id derives distinct schedules.
    assert_ne!(batch_schedule_id(&[0xBB; 32], 0), batch_schedule_id(&[0xBB; 32], 1));
}
