// SPDX-License-Identifier: MIT OR Apache-2.0
//! Recompute, on the host, exactly what the deployed programs were asked to do.
//!
//! `DEPLOYMENTS.md` claims the three programs agree with this crate to the unit.
//! That claim is only checkable if the host side is reproducible too, so this is
//! it: the same inputs the on-chain transactions carried, run through the same
//! functions, printing the values a reader can compare against the account state
//! they fetch themselves.
//!
//! ```text
//! cargo run --release --example onchain_reference
//! ```

use antumbra::vesting::Schedule;
use antumbra::weighted::{weight_at, ONE};
use antumbra::{binfixed, Curve};

fn main() {
    // --- bonding curve, PDA gweZAVxdgb2NmveTwf25hQVkueEZdxQEJyUpJgUE8N2 ---
    // create_sale, then three buys: 500 from the funded public account, then 250
    // from each of two single-use ephemeral accounts fed by a deshield.
    let mut c = Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
    for collateral in [500 * ONE, 250 * ONE, 250 * ONE] {
        c.buy(collateral, 0).unwrap();
    }
    println!("bonding curve after three buys");
    println!("  vt              = {}", c.vt);
    println!("  vc              = {}", c.vc);
    println!("  sale_reserve    = {}", c.sale_reserve);
    println!("  real_collateral = {}", c.real_collateral);
    println!("  seed_reserve is not touched by a buy, so the chain must still hold 200000 * ONE");

    // --- weighted pool, PDA 25ekuB2nQ84WLvoVjWejf63Z714X9vvjnb7Jz4R3Kkdg ---
    // 99/1 shifting to 1/99 over ten thousand seconds, bought at now = 3500.
    let (w_start, w_end) = (990_000_000_000_000_000u128, 10_000_000_000_000_000u128);
    let w_token = weight_at(w_start, w_end, 1_000, 11_000, 3_500).unwrap();
    let w_coll = ONE - w_token;
    let (rt, rc, c_in) = (1_000_000 * ONE, 100_000 * ONE, 5_000 * ONE);
    let out = binfixed::weighted_buy(rt, rc, c_in, w_token, w_coll).unwrap();
    println!("\nweighted pool at now = 3500");
    println!(
        "  weight (token)     = {w_token}   ({}%)",
        w_token / (ONE / 100)
    );
    println!("  tokens out         = {out}");
    println!("  reserve_token      = {}", rt - out);
    println!("  reserve_collateral = {}", rc + c_in);

    // --- vesting, PDA SqLgfYnsQ6STtHRvjNxj1MmXjM3EsUBbsCCvDCcpX9B ---
    // linear, 1e21 from t=1000 to t=8919, claimed once at t=4959.
    let mut s = Schedule::linear(1_000, 8_919, 1_000 * ONE).unwrap();
    let paid = s.claim(4_959).unwrap();
    println!("\nvesting schedule claimed at t = 4959");
    println!("  paid    = {paid}");
    println!("  claimed = {}", s.claimed);
}
