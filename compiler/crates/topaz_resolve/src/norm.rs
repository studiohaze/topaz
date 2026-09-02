//! Unicode normalization for the module collision keys (SPEC v5.2
//! §17, CDR-002 §5): NFD canonical equivalence and default case
//! folding, over the generated tables in the `unicode_norm` module.
//!
//! Scope note: this is exactly what the collision keys need — full
//! NFD (canonical decomposition, Hangul included, with canonical
//! reordering) and default case folding. NFC composition is not
//! needed: two names are canonically equivalent iff their NFDs are
//! equal.

use crate::unicode_norm::{CCC, DECOMP, FOLD};

const HANGUL_S_BASE: u32 = 0xAC00;
const HANGUL_L_BASE: u32 = 0x1100;
const HANGUL_V_BASE: u32 = 0x1161;
const HANGUL_T_BASE: u32 = 0x11A7;
const HANGUL_V_COUNT: u32 = 21;
const HANGUL_T_COUNT: u32 = 28;
const HANGUL_S_COUNT: u32 = 11172;

fn lookup<T: Copy>(table: &[(u32, T)], code: u32) -> Option<T> {
    table
        .binary_search_by_key(&code, |&(c, _)| c)
        .ok()
        .map(|i| table[i].1)
}

fn ccc(code: u32) -> u8 {
    lookup(CCC, code).unwrap_or(0)
}

/// Canonical decomposition (NFD) of `text`.
pub fn nfd(text: &str) -> String {
    let mut scalars: Vec<u32> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        decompose(ch as u32, &mut scalars);
    }
    // Canonical ordering: stable exchange of adjacent nonzero-CCC
    // pairs that are out of order.
    let mut i = 1;
    while i < scalars.len() {
        let (a, b) = (ccc(scalars[i - 1]), ccc(scalars[i]));
        if b != 0 && a > b {
            scalars.swap(i - 1, i);
            if i > 1 {
                i -= 1;
                continue;
            }
        }
        i += 1;
    }
    scalars
        .into_iter()
        .map(|c| char::from_u32(c).expect("valid scalar"))
        .collect()
}

fn decompose(code: u32, out: &mut Vec<u32>) {
    // Hangul syllables decompose algorithmically (UAX #15).
    if (HANGUL_S_BASE..HANGUL_S_BASE + HANGUL_S_COUNT).contains(&code) {
        let s = code - HANGUL_S_BASE;
        out.push(HANGUL_L_BASE + s / (HANGUL_V_COUNT * HANGUL_T_COUNT));
        out.push(HANGUL_V_BASE + (s % (HANGUL_V_COUNT * HANGUL_T_COUNT)) / HANGUL_T_COUNT);
        let t = s % HANGUL_T_COUNT;
        if t != 0 {
            out.push(HANGUL_T_BASE + t);
        }
        return;
    }
    match lookup(DECOMP, code) {
        Some(seq) => out.extend_from_slice(seq),
        None => out.push(code),
    }
}

/// Default case folding (statuses C+F) of `text`.
pub fn casefold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match lookup(FOLD, ch as u32) {
            Some(seq) => {
                for &c in seq {
                    out.push(char::from_u32(c).expect("valid scalar"));
                }
            }
            None => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfd_separates_canonical_equivalents() {
        // U+00E9 (é, NFC) and U+0065 U+0301 (e + combining acute, NFD).
        assert_eq!(nfd("caf\u{e9}"), "cafe\u{301}");
        assert_eq!(nfd("cafe\u{301}"), "cafe\u{301}");
    }

    #[test]
    fn nfd_decomposes_hangul() {
        // 한 = U+D55C -> U+1112 U+1161 U+11AB.
        assert_eq!(nfd("\u{d55c}"), "\u{1112}\u{1161}\u{11ab}");
    }

    #[test]
    fn casefold_is_default_full_folding() {
        assert_eq!(casefold("Strings"), "strings");
        // Full folding: ß -> ss, İ -> i + combining dot above.
        assert_eq!(casefold("stra\u{df}e"), "strasse");
        assert_eq!(casefold("\u{130}"), "i\u{307}");
    }

    #[test]
    fn canonical_reordering_sorts_combining_marks() {
        // U+0301 (ccc 230) after U+0323 (ccc 220) regardless of input
        // order.
        assert_eq!(nfd("q\u{301}\u{323}"), "q\u{323}\u{301}");
    }
}
