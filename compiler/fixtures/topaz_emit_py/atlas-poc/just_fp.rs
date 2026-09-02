// B3 (Latin + CJK) — full fixed-point justification penalty REFERENCE.
// ATLAS compute_justification_line_break_penalty (vnext_paragraph.rs:861-943) + helpers, in milli-points.
// I/O: line0 "<target> <minGap> <ratioM(=minLineWidthRatio*1000)> <maxAdjMpt> <maxGapMpt> <maxGapRatioR(=ratio*R)> <mode(0=latin,1=cjk)> <isLast>"
//      then "<advMpt>\t<text>"  ->  penalty B-units.
use std::io::Read;
const B: i64 = 1_000_000;
const R: i64 = 100_000_000;
const CAP: i64 = 25_000;
fn round_div(n: i64, d: i64) -> i64 { let q=n/d; let r=n%d; if r>=(d+1)/2 {q+1} else {q} }
fn is_word_space(t: &str) -> bool { t==" " || t=="\u{00A0}" }
fn is_u0020(t: &str) -> bool { t==" " }
fn is_cjk_just_char(c: char) -> bool { let cp=c as u32;
    matches!(cp, 0x1100..=0x11FF|0x3130..=0x318F|0x3400..=0x4DBF|0x4E00..=0x9FFF|0xAC00..=0xD7A3|0xF900..=0xFAFF) }
fn is_just_punct(c: char) -> bool { matches!(c,
    '.'|','|';'|':'|'!'|'?'|'"'|'\''|'('|')'|'['|']'|'{'|'}'|'。'|'、'|'，'|'．'|'「'|'」'|'『'|'』'|'《'|'》'|'〈'|'〉') }
fn is_single_cjk(t: &str) -> bool { let mut ch=t.chars(); match ch.next() { Some(c)=> ch.next().is_none() && is_cjk_just_char(c), None=>false } }
fn is_just_punct_unit(t: &str) -> bool { t.chars().any(is_just_punct) }
fn is_cjk_gap_cand(l: &str, r: &str) -> bool { is_single_cjk(l)&&is_single_cjk(r)&&!is_just_punct_unit(l)&&!is_just_punct_unit(r) }
fn is_kr_ws_gap_cand(l: &str, s: &str, r: &str) -> bool { is_single_cjk(l)&&is_word_space(s)&&is_single_cjk(r)&&!is_just_punct_unit(l)&&!is_just_punct_unit(r) }
fn cjk_gap_count(texts: &[String], start: usize, end: usize) -> i64 {
    let mut g = 0i64;
    let mut i = start; while i + 1 < end { if is_cjk_gap_cand(&texts[i], &texts[i+1]) { g += 1; } i += 1; }
    let mut i = start; while i + 2 < end {
        if is_kr_ws_gap_cand(&texts[i], &texts[i+1], &texts[i+2]) {
            if !is_cjk_gap_cand(&texts[i], &texts[i+1]) { g += 1; }
            if !is_cjk_gap_cand(&texts[i+1], &texts[i+2]) { g += 1; }
        }
        i += 1;
    }
    g
}
fn rep(adv: &[i64], texts: &[String], start: usize, end: usize) -> Option<(i64, i64)> {
    let mut sum=0i64; let mut cnt=0i64; let mut i=start;
    while i < end { if is_single_cjk(&texts[i]) { if adv[i] <= 1 { return None; } sum+=adv[i]; cnt+=1; } i+=1; }
    if cnt==0 { None } else { Some((sum,cnt)) }
}

fn just_penalty(adv: &[i64], texts: &[String], target: i64, min_gap: i64, ratio_m: i64,
                max_adj: i64, max_gap: i64, max_gap_ratio_r: i64, mode: i64, is_last: bool) -> i64 {
    if is_last { return 0; }
    let n = texts.len();
    let mut eff = n; while eff > 0 && is_word_space(&texts[eff-1]) { eff -= 1; }
    if eff == 0 { return CAP*B; }
    if target <= 0 { return CAP*B; }
    let original: i64 = adv[..eff].iter().sum();
    if original*1000 < target*ratio_m { return CAP*B; }
    let adjustment = target - original;
    if adjustment <= 1 { return 0; }
    if adjustment > max_adj { return CAP*B + round_div((adjustment-max_adj)*B, 1000); }
    if mode == 0 {
        let gap = texts[..eff].iter().filter(|t| is_u0020(t)).count() as i64;
        if gap < min_gap || gap == 0 { return CAP*B; }   // gap==0 guard (div-by-zero if minGap=0 seam-violated)
        let won = adjustment - gap*max_gap;
        if won > gap { return CAP*B + round_div(won*B, gap); }
    } else {
        let gap = cjk_gap_count(texts, 0, eff);
        if gap < min_gap || gap == 0 { return CAP*B; }   // gap==0 guard
        let Some((rep_sum, rep_count)) = rep(adv, texts, 0, eff) else { return CAP*B; };
        let won = adjustment - gap*max_gap;
        let ratio_r = round_div(adjustment * rep_count * R, gap * rep_sum);
        let ror = ratio_r - max_gap_ratio_r;
        if won > gap || ror > 100_000 {   // 100_000 = R/1000 (ratio tolerance)
            let wt = if won > 0 { round_div(won*B, gap) } else { 0 };
            let rt = if ror > 0 { ror * 100 } else { 0 };   // 10000*B/R = 100
            return CAP*B + wt + rt;
        }
    }
    0
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let mut lines = input.split('\n');
    let h: Vec<i64> = lines.next().unwrap().trim().split(' ').map(|x| x.parse().unwrap()).collect();
    let (target,min_gap,ratio_m,max_adj,max_gap,max_gap_ratio_r,mode,is_last) =
        (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]!=0);
    let mut adv=Vec::new(); let mut texts=Vec::new();
    for l in lines { let mut it=l.splitn(2,'\t'); adv.push(it.next().unwrap().parse::<i64>().unwrap()); texts.push(it.next().unwrap_or("").to_string()); }
    println!("{}", just_penalty(&adv,&texts,target,min_gap,ratio_m,max_adj,max_gap,max_gap_ratio_r,mode,is_last));
}
