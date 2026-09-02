// B3a — fixed-point LatinWordSpace justification penalty (ATLAS compute_justification_line_break_penalty,
// Latin branch, vnext_paragraph.rs:857-944). Reference. I/O:
//   line0: "<target> <minGap> <minLineWidthRatioM(=ratio*1000)> <maxAdjMpt> <maxGapMpt> <isLast(0/1)>"
//   then:  "<advanceMpt>\t<text>"  ->  penalty B-units (one int)
use std::io::Read;
const B: i64 = 1_000_000;
const CAP: i64 = 25_000; // JUSTIFICATION_CAP_EXCEEDED_LINE_PENALTY
fn round_div(n: i64, d: i64) -> i64 { let q=n/d; let r=n%d; if r>=(d+1)/2 {q+1} else {q} }
fn is_word_space(t: &str) -> bool { t==" " || t=="\u{00A0}" }
fn is_u0020(t: &str) -> bool { t==" " }

fn just_latin(adv: &[i64], texts: &[String], target: i64, min_gap: i64,
              min_lw_ratio_m: i64, max_adj: i64, max_gap: i64, is_last: bool) -> i64 {
    if is_last { return 0; }
    let n = texts.len();
    let mut eff = n;
    while eff > 0 && is_word_space(&texts[eff-1]) { eff -= 1; }   // effective_end (trim trailing word-spaces)
    if eff == 0 { return CAP*B; }
    if target <= 0 { return CAP*B; }
    let original: i64 = adv[..eff].iter().sum();
    if original*1000 < target*min_lw_ratio_m { return CAP*B; }    // original/target < min_line_width_ratio
    let adjustment = target - original;
    if adjustment <= 1 { return 0; }                              // <= TOL (1 mpt = 0.001pt)
    if adjustment > max_adj { return CAP*B + round_div((adjustment-max_adj)*B, 1000); }
    let gap_count: i64 = texts[..eff].iter().filter(|t| is_u0020(t)).count() as i64;
    if gap_count < min_gap { return CAP*B; }
    let width_over_num = adjustment - gap_count*max_gap;          // gapCount*(adj_per_gap - max_gap), mpt
    if width_over_num > gap_count {                              // width_overage_pt > 0.001pt
        return CAP*B + round_div(width_over_num*B, gap_count);
    }
    0
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let mut lines = input.split('\n');
    let hdr: Vec<i64> = lines.next().unwrap().trim().split(' ').map(|x| x.parse().unwrap()).collect();
    let (target, min_gap, ratio_m, max_adj, max_gap, is_last) =
        (hdr[0], hdr[1], hdr[2], hdr[3], hdr[4], hdr[5] != 0);
    let mut adv = Vec::new(); let mut texts = Vec::new();
    for l in lines {
        let mut it = l.splitn(2, '\t');
        adv.push(it.next().unwrap().parse::<i64>().unwrap());
        texts.push(it.next().unwrap_or("").to_string());
    }
    println!("{}", just_latin(&adv, &texts, target, min_gap, ratio_m, max_adj, max_gap, is_last));
}
