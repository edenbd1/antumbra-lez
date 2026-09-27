//! Whole-instruction cost of each vesting operation (RFP-017 P2), measured by
//! executing the committed binary the way the sequencer does.
//!
//!     cargo run --release --manifest-path executor-tests/Cargo.toml --bin measure
use antumbra_executor_tests::*;

const T0: u64 = 1_800_000_000_000;
const MIN: u64 = 60_000;

fn main() {
    let mut rows: Vec<(&str, u64)> = Vec::new();
    let signer = |id| acc(id, AUTH_TRANSFER, 1_000, vec![], true);

    let w = World::new("m-create");
    let create = Ix::CreateSchedule {
        schedule_id: w.id, kind: 0, start: T0, cliff: T0 + MIN, end: T0 + 30 * MIN, total: 600,
        beneficiary: BENEFICIARY, cancelable: 1, transferable: 1, tranches: 0,
        cancel_authority: Z, milestone_authority: Z, refund_to: REFUND,
    };
    let r = run(&w.elf, &w.pid, &create, vec![w.schedule.clone(), w.holding.clone(), signer(CREATOR)]).unwrap();
    rows.push(("create schedule", r.cycles));

    let mut w = World::new("m-claim");
    w.with(w.linear(T0, T0 + 30 * MIN, 600), 600);
    rows.push(("claim (native)", w.claim(T0 + 10 * MIN, native(DEST, 0), BENEFICIARY).unwrap().cycles));
    rows.push(("cancel (native)", w.cancel(T0 + 10 * MIN, native(REFUND, 0), CREATOR).unwrap().cycles));

    let mut t = World::new("m-token");
    let mut s = t.linear(T0, T0 + 30 * MIN, 600);
    s.asset = 1;
    s.token_definition = DEFINITION;
    t.with(s, 600);
    rows.push(("claim (token, chained transfer)", t.claim(T0 + 10 * MIN, token_holding(DEST, DEFINITION, 0, false), BENEFICIARY).unwrap().cycles));

    let mut m = World::new("m-ms");
    let mut s = m.linear(0, 1, 2);
    s.kind = 2;
    s.tranches = 2;
    m.with(s, 2);
    let sig = Ix::SignalMilestone { schedule_id: m.id, index: 0 };
    rows.push(("signal milestone", run(&m.elf, &m.pid, &sig, vec![m.schedule.clone(), signer(CREATOR)]).unwrap().cycles));

    let mut x = World::new("m-xfer");
    x.with(x.linear(T0, T0 + MIN, 10), 10);
    let tb = Ix::TransferBeneficiary { schedule_id: x.id, new_beneficiary: DEST };
    rows.push(("transfer beneficiary", run(&x.elf, &x.pid, &tb, vec![x.schedule.clone(), signer(BENEFICIARY)]).unwrap().cycles));

    let b = World::new("m-batch");
    let n = 8u32;
    let batch = Ix::CreateScheduleBatch {
        batch_id: b.id, kind: 1, start: T0, cliff: T0, end: T0 + 30 * MIN, total_each: 60,
        beneficiaries: vec![BENEFICIARY; n as usize], cancelable: 1, transferable: 0, tranches: 0,
        cancel_authority: Z, milestone_authority: Z, refund_to: REFUND,
    };
    let mut pre = vec![b.holding.clone(), signer(CREATOR)];
    for i in 0..n {
        pre.push(acc(*pda(&b.pid, &[batch_schedule_id(&b.id, i)]).value(), lee_core::program::ProgramId::default(), 0, vec![], false));
    }
    rows.push(("create a batch of 8 schedules", run(&b.elf, &b.pid, &batch, pre).unwrap().cycles));

    println!("program ImageID word 0: {}", w.pid[0]);
    println!("| operation | cycles | share of the 32M public-execution cap |");
    println!("|---|---:|---:|");
    for (op, c) in rows {
        println!("| {op} | {c} | {:.3}% |", c as f64 / MAX_NUM_CYCLES_PUBLIC_EXECUTION as f64 * 100.0);
    }
}
