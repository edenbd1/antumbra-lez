fn main() {
    const ONE: u128 = 1_000_000_000_000_000_000;
    let mut c = antumbra::Curve::new(1_000_000 * ONE, 1_000 * ONE, 800_000 * ONE).unwrap();
    c.buy(500 * ONE, 0).unwrap();
    c.buy(250 * ONE, 0).unwrap();
    c.buy(250 * ONE, 0).unwrap();
    println!("vt={} vc={} sale_reserve={} real_collateral={}", c.vt, c.vc, c.sale_reserve, c.real_collateral);
}
