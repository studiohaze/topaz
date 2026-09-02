// B1 — fixed-point badness REFERENCE (the reformulation per ATLAS-POC-P1-DESIGN.md §합).
// This is the NEW reference (not verbatim float ATLAS): B1 proves Topaz ≡ this fixed-point Rust.
// I/O: stdin lines "width|target|char|spaces|kind|isLast(0/1)" -> badness B-units per line.
use std::io::Read;
const R: i64 = 100_000_000; // ratio scale 1e8
const B: i64 = 1_000_000;   // badness scale 1e6

fn round_div(n: i64, d: i64) -> i64 { // round-half-up, non-negative only; avoids n+d/2 overflow
    let q = n / d;
    let r = n % d;
    if r >= (d + 1) / 2 { q + 1 } else { q }
}
fn abs_i(x: i64) -> i64 { if x < 0 { -x } else { x } }

fn badness(width: i64, target: i64, char_count: i64, spaces: i64, kind: &str, is_last: bool) -> i64 {
    let slack = target - width;
    let abs_slack = abs_i(slack);
    let raw_r = if abs_slack >= 4 * target { 4 * R } else { abs_slack * R / target };
    let adjcap_centi = if slack >= 0 { 100 + spaces * 45 } else { 100 + spaces * 25 };
    let sr_r = round_div(raw_r * 100, adjcap_centi);
    let sq = sr_r * sr_r;
    let q = round_div(sq, R);
    let mut base_b = round_div(q * (1000 * B), R);
    if is_last {
        base_b = round_div(base_b * 2, 5); // ×0.4 exact
        if width * 100 < target * 52 && char_count > 0 { base_b += 500 * B; }
    }
    if slack < 0 {
        base_b += 20_000 * B + round_div(20_000 * B * sr_r, R); // overflow penalty, split
    }
    if kind == "latin_hyphen" { base_b += 65 * B; }
    if kind == "hard_fallback" { base_b += 50_000 * B; }
    if char_count <= 2 { base_b += 275 * B; }
    base_b += 10 * B; // line penalty
    base_b
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let mut out = String::new();
    for line in input.split('\n') {
        let f: Vec<&str> = line.split('|').collect();
        let w: i64 = f[0].parse().unwrap();
        let t: i64 = f[1].parse().unwrap();
        let c: i64 = f[2].parse().unwrap();
        let sp: i64 = f[3].parse().unwrap();
        let kind = f[4];
        let is_last = f[5] == "1";
        out.push_str(&format!("{}\n", badness(w, t, c, sp, kind, is_last)));
    }
    print!("{}", out);
}
