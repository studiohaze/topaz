// B2 — fixed-point line-break DP REFERENCE (Rust). Combines the verbatim P0 classifier +
// the B1 fixed-point badness + the ATLAS DP structure (vnext_paragraph.rs:573-731), reformulated
// to integer milli-points. Scope: drop-cap OFF (single bucket), first_line=target, justification=None.
// I/O: stdin = "<targetMpt>\n<advanceMpt>\t<text>\n..." -> "<start> <endExcl> <lineBadnessB>\n...TOTAL <b>".
use std::io::Read;
const R: i64 = 100_000_000;
const B: i64 = 1_000_000;
const MAX_DP_PREDECESSORS: usize = 80;

// ── P0 classifier (verbatim ATLAS predicates) ──
#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakKind { KoreanCluster, LatinSpace, LatinHyphen, ClosingPunctuation, HardFallback, ForcedEnd }
impl BreakKind {
    fn as_str(self) -> &'static str { match self {
        Self::KoreanCluster=>"korean_cluster", Self::LatinSpace=>"latin_space", Self::LatinHyphen=>"latin_hyphen",
        Self::ClosingPunctuation=>"closing_punctuation", Self::HardFallback=>"hard_fallback", Self::ForcedEnd=>"forced_end" } }
}
fn is_korean_break_character(c: char) -> bool { let cp=c as u32;
    matches!(cp,0x1100..=0x11FF)||matches!(cp,0x3130..=0x318F)||matches!(cp,0xAC00..=0xD7AF)||matches!(cp,0x3400..=0x4DBF)
    ||matches!(cp,0x4E00..=0x9FFF)||matches!(cp,0xF900..=0xFAFF)||matches!(c,'，'|'、'|'。'|'；'|'：') }
fn is_korean_break_allowed(cur: &str, next: Option<&str>) -> bool {
    cur.chars().all(is_korean_break_character) && next.map_or(true,|t|!is_latin_space(t))
    && next.map_or(true,|t|!t.chars().any(is_line_start_forbidden)) && !cur.chars().any(is_line_end_forbidden) }
fn is_closing_punctuation_break_allowed(cur: &str, prev: Option<&str>, next: Option<&str>) -> bool {
    cur.chars().all(is_line_start_forbidden) && next.map_or(true,|t|!is_latin_space(t)) && !is_numeric_separator_between_digits(cur,prev,next) }
fn is_numeric_separator_between_digits(cur: &str, prev: Option<&str>, next: Option<&str>) -> bool {
    matches!(cur,","|"."|":") && prev.is_some_and(is_ascii_digit) && next.is_some_and(is_ascii_digit) }
fn is_ascii_digit(t: &str) -> bool { t.len()==1 && t.as_bytes()[0].is_ascii_digit() }
fn is_line_start_forbidden(c: char) -> bool { matches!(c,')'|']'|'}'|'」'|'』'|'》'|'〉'|','|'.'|';'|':'|'!'|'?'|'、'|'。') }
fn is_line_end_forbidden(c: char) -> bool { matches!(c,'('|'['|'{'|'「'|'『'|'《'|'〈') }
fn is_latin_space(t: &str) -> bool { t==" "||t=="\u{00A0}" }
fn is_breakable_latin_space(t: &str) -> bool { t==" " }
fn is_no_break_whitespace(t: &str) -> bool { t=="\u{00A0}"||t=="\u{202F}" }
fn is_latin_hyphen(t: &str) -> bool { matches!(t,"-"|"‐"|"‑"|"–") }

fn build_break_opportunities(units: &[String]) -> Vec<(usize, BreakKind)> {
    let mut o = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        let unit_end = i+1;
        let prev = i.checked_sub(1).and_then(|p|units.get(p)).map(|p|p.as_str());
        let next = units.get(unit_end).map(|n|n.as_str());
        if unit_end==units.len() { o.push((unit_end,BreakKind::ForcedEnd)); continue; }
        if is_breakable_latin_space(unit) { o.push((unit_end,BreakKind::LatinSpace)); continue; }
        if is_latin_hyphen(unit) { o.push((unit_end,BreakKind::LatinHyphen)); continue; }
        if is_closing_punctuation_break_allowed(unit,prev,next) { o.push((unit_end,BreakKind::ClosingPunctuation)); continue; }
        if is_korean_break_allowed(unit,next) { o.push((unit_end,BreakKind::KoreanCluster)); continue; }
        if next.is_some_and(is_latin_space)||is_no_break_whitespace(unit)||next.is_some_and(|t|t.chars().any(is_line_start_forbidden)) { continue; }
        o.push((unit_end,BreakKind::HardFallback));
    }
    o
}

// ── B1 fixed-point badness (justification=None) ──
fn round_div(n: i64, d: i64) -> i64 { let q=n/d; let r=n%d; if r>=(d+1)/2 {q+1} else {q} }
fn abs_i(x: i64) -> i64 { if x<0 {-x} else {x} }
fn badness(width: i64, target: i64, char_count: i64, spaces: i64, kind: BreakKind, is_last: bool) -> i64 {
    let slack = target - width;
    let abs_slack = abs_i(slack);
    let raw_r = if abs_slack >= 4*target { 4*R } else { abs_slack*R/target };
    let adjcap_centi = if slack>=0 { 100+spaces*45 } else { 100+spaces*25 };
    let sr_r = round_div(raw_r*100, adjcap_centi);
    let sq = sr_r*sr_r;
    let q = round_div(sq, R);
    let mut base_b = round_div(q*(1000*B), R);
    if is_last { base_b = round_div(base_b*2,5); if width*100 < target*52 && char_count>0 { base_b += 500*B; } }
    if slack<0 { base_b += 20_000*B + round_div(20_000*B*sr_r, R); }
    if kind==BreakKind::LatinHyphen { base_b += 65*B; }
    if kind==BreakKind::HardFallback { base_b += 50_000*B; }
    if char_count<=2 { base_b += 275*B; }
    base_b += 10*B;
    base_b
}

// ── DP (single bucket; target constant; tol = 1 mpt) ──
struct St { present: bool, cost: i64, prev: usize, line_cost: i64 }
fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let mut lines = input.split('\n');
    let target: i64 = lines.next().unwrap().trim().parse().unwrap();
    if target <= 0 || target > 23_000_000_000 { print!("OUT_OF_BOUNDS\n"); return; } // seam cap (design §합)
    let mut texts: Vec<String> = Vec::new();
    let mut adv: Vec<i64> = Vec::new();
    for l in lines {
        let mut it = l.splitn(2, '\t');
        let a: i64 = it.next().unwrap().parse().unwrap();
        let t = it.next().unwrap_or("").to_string();
        adv.push(a); texts.push(t);
    }
    let n = texts.len();
    // prefix sums
    let mut wpre = vec![0i64; n+1];
    let mut cpre = vec![0i64; n+1];
    for i in 0..n { wpre[i+1]=wpre[i]+adv[i]; cpre[i+1]=cpre[i]+texts[i].chars().count() as i64; }
    // breakpoints: (unit_end, kind); index 0 is the sentinel start
    let mut bp: Vec<(usize, BreakKind)> = vec![(0, BreakKind::ForcedEnd)];
    bp.extend(build_break_opportunities(&texts));
    let mut states: Vec<St> = (0..bp.len()).map(|_| St{present:false,cost:0,prev:0,line_cost:0}).collect();
    states[0] = St{present:true,cost:0,prev:0,line_cost:0};
    for end_index in 1..bp.len() {
        let (end_unit, end_kind) = bp[end_index];
        let pred_start = end_index.saturating_sub(MAX_DP_PREDECESSORS);
        let is_last = end_unit == n;
        let mut best: Option<St> = None;
        for prev_index in pred_start..end_index {
            if !states[prev_index].present { continue; }
            let start = bp[prev_index].0;
            if start >= end_unit { continue; }
            let width = wpre[end_unit]-wpre[start];
            if width > target + 1 && end_unit > start+1 { continue; }
            let sp: i64 = (start..end_unit).filter(|&u| is_breakable_latin_space(&texts[u])).count() as i64;
            let line_cost = badness(width, target, cpre[end_unit]-cpre[start], sp, end_kind, is_last);
            let cand_cost = states[prev_index].cost + line_cost;
            if best.as_ref().map_or(true, |b| cand_cost < b.cost) {
                best = Some(St{present:true, cost:cand_cost, prev:prev_index, line_cost});
            }
        }
        if let Some(b)=best { states[end_index]=b; }
    }
    // back-walk
    let mut out = String::new();
    let last = bp.len()-1;
    if !states[last].present { print!("NO_PLAN\n"); return; }
    let mut ranges: Vec<(usize,usize,i64)> = Vec::new();
    let mut wi = last;
    while wi > 0 {
        let st = &states[wi];
        let start = bp[st.prev].0;
        let end = bp[wi].0;
        ranges.push((start, end, st.line_cost));
        wi = st.prev;
    }
    ranges.reverse();
    let mut total = 0i64;
    for (s,e,lc) in &ranges { out.push_str(&format!("{} {} {}\n", s, e, lc)); total += lc; }
    out.push_str(&format!("TOTAL {}\n", total));
    print!("{}", out);
}
