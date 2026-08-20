fn main() {
    use antumbra::weighted::{weight_at, ONE};
    let (w_start, w_end) = (990_000_000_000_000_000u128, 10_000_000_000_000_000u128);
    let (t_start, t_end, now) = (1_000u64, 11_000u64, 3_500u64);
    let w_token = weight_at(w_start, w_end, t_start, t_end, now).unwrap();
    let w_coll = ONE - w_token;
    let rt = 1_000_000 * ONE;
    let rc = 100_000 * ONE;
    let c_in = 5_000 * ONE;
    let out = antumbra::binfixed::weighted_buy(rt, rc, c_in, w_token, w_coll).unwrap();
    println!("w_token={w_token}");
    println!("w_coll={w_coll}");
    println!("tokens_out={out}");
    println!("reserve_token_after={}", rt - out);
    println!("reserve_collateral_after={}", rc + c_in);
}
