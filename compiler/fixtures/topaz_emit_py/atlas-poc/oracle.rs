// FAITHFUL ORACLE: ATLAS break-opportunity classifier + kinsoku predicates copied
// VERBATIM from /root/atlas/core/typesetting/src/vnext_paragraph.rs (develop).
// Reads clusters from stdin (one per line, NO trailing newline added by the harness),
// prints "<unit_end> <kind>" per break opportunity. This is the ground-truth reference.

use std::io::Read;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreakKind {
    KoreanCluster,
    LatinSpace,
    LatinHyphen,
    ClosingPunctuation,
    HardFallback,
    ForcedEnd,
}
impl BreakKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::KoreanCluster => "korean_cluster",
            Self::LatinSpace => "latin_space",
            Self::LatinHyphen => "latin_hyphen",
            Self::ClosingPunctuation => "closing_punctuation",
            Self::HardFallback => "hard_fallback",
            Self::ForcedEnd => "forced_end",
        }
    }
}

// ── predicates: verbatim from vnext_paragraph.rs:1171-1255 ──
fn is_korean_break_character(character: char) -> bool {
    let code_point = character as u32;
    matches!(code_point, 0x1100..=0x11FF)
        || matches!(code_point, 0x3130..=0x318F)
        || matches!(code_point, 0xAC00..=0xD7AF)
        || matches!(code_point, 0x3400..=0x4DBF)
        || matches!(code_point, 0x4E00..=0x9FFF)
        || matches!(code_point, 0xF900..=0xFAFF)
        || matches!(character, '，' | '、' | '。' | '；' | '：')
}
fn is_korean_break_allowed(current: &str, next: Option<&str>) -> bool {
    current.chars().all(is_korean_break_character)
        && next.map_or(true, |text| !is_latin_space(text))
        && next.map_or(true, |text| !text.chars().any(is_line_start_forbidden))
        && !current.chars().any(is_line_end_forbidden)
}
fn is_closing_punctuation_break_allowed(current: &str, previous: Option<&str>, next: Option<&str>) -> bool {
    current.chars().all(is_line_start_forbidden)
        && next.map_or(true, |text| !is_latin_space(text))
        && !is_numeric_separator_between_digits(current, previous, next)
}
fn is_numeric_separator_between_digits(current: &str, previous: Option<&str>, next: Option<&str>) -> bool {
    matches!(current, "," | "." | ":")
        && previous.is_some_and(is_ascii_digit)
        && next.is_some_and(is_ascii_digit)
}
fn is_ascii_digit(text: &str) -> bool {
    text.len() == 1 && text.as_bytes()[0].is_ascii_digit()
}
fn is_line_start_forbidden(character: char) -> bool {
    matches!(character,
        ')' | ']' | '}' | '」' | '』' | '》' | '〉' | ',' | '.' | ';' | ':' | '!' | '?' | '、' | '。')
}
fn is_line_end_forbidden(character: char) -> bool {
    matches!(character, '(' | '[' | '{' | '「' | '『' | '《' | '〈')
}
fn is_latin_space(text: &str) -> bool {
    text == " " || text == "\u{00A0}"
}
fn is_breakable_latin_space(text: &str) -> bool {
    text == " "
}
fn is_no_break_whitespace(text: &str) -> bool {
    text == "\u{00A0}" || text == "\u{202F}"
}
fn is_latin_hyphen(text: &str) -> bool {
    matches!(text, "-" | "‐" | "‑" | "–")
}

// ── classifier: verbatim logic from vnext_paragraph.rs:412-478, over cluster strings ──
struct Opp { unit_end: usize, kind: BreakKind }
fn build_break_opportunities(units: &[String]) -> Vec<Opp> {
    let mut opportunities = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        let unit_end = index + 1;
        let previous_text = index.checked_sub(1).and_then(|p| units.get(p)).map(|p| p.as_str());
        let next_text = units.get(unit_end).map(|n| n.as_str());
        if unit_end == units.len() {
            opportunities.push(Opp { unit_end, kind: BreakKind::ForcedEnd });
            continue;
        }
        if is_breakable_latin_space(unit) {
            opportunities.push(Opp { unit_end, kind: BreakKind::LatinSpace });
            continue;
        }
        if is_latin_hyphen(unit) {
            opportunities.push(Opp { unit_end, kind: BreakKind::LatinHyphen });
            continue;
        }
        if is_closing_punctuation_break_allowed(unit, previous_text, next_text) {
            opportunities.push(Opp { unit_end, kind: BreakKind::ClosingPunctuation });
            continue;
        }
        if is_korean_break_allowed(unit, next_text) {
            opportunities.push(Opp { unit_end, kind: BreakKind::KoreanCluster });
            continue;
        }
        if next_text.is_some_and(is_latin_space)
            || is_no_break_whitespace(unit)
            || next_text.is_some_and(|text| text.chars().any(is_line_start_forbidden))
        {
            continue;
        }
        opportunities.push(Opp { unit_end, kind: BreakKind::HardFallback });
    }
    opportunities
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    // protocol: clusters are exactly the newline-split of stdin (harness sends no trailing newline)
    let units: Vec<String> = input.split('\n').map(|s| s.to_string()).collect();
    let mut out = String::new();
    for opp in build_break_opportunities(&units) {
        out.push_str(&format!("{} {}\n", opp.unit_end, opp.kind.as_str()));
    }
    print!("{}", out);
}
