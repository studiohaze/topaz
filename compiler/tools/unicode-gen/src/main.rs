//! unicode-gen — generates `crates/topaz_lexer/src/unicode_tables.rs`
//! and `unicode_version.rs` from the vendored, pinned UCD data under
//! `tools/unicode-gen/data/` (CDR-001 §3). Dev-time only: v0.1 builds
//! never download Unicode data.
//!
//! Tables emitted (sorted, merged, inclusive scalar ranges):
//!
//! - `LETTER` — general categories Lu, Ll, Lt, Lm, Lo
//!   (SPEC §1 `UnicodeLetter`)
//! - `NUMBER` — general categories Nd, Nl, No
//!   (SPEC §1 `UnicodeNumber`)
//! - `EMOJI`  — UCD `Emoji` property **minus U+0000..U+007F**
//!   (SPEC §1 `Emoji`); the ASCII overlap (`#`, `*`, `0`-`9`) is
//!   excluded because those scalars are literal/operator grammar,
//!   never identifier starts (CDR-001 §3).
//!
//! Additionally emitted for the resolver (CDR-002 §5):
//!
//! - `crates/topaz_resolve/src/unicode_norm/` — `FOLD` (default case
//!   folding, statuses C+F, scalar → sequence), `DECOMP` (fully
//!   expanded canonical decompositions, scalar → NFD sequence;
//!   Hangul stays algorithmic at runtime), and `CCC` (nonzero
//!   canonical combining classes), from `CaseFolding.txt` and
//!   `UnicodeData.txt`.
//! - `crates/topaz_self_frontend/topaz/unicode_tables.tpz` — the same
//!   normalization data as flattened key/offset/value arrays consumed
//!   by the self-hosted resolver.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p unicode-gen
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

const UNICODE_VERSION: &str = "16.0.0";
const LETTER_CATS: [&str; 5] = ["Lu", "Ll", "Lt", "Lm", "Lo"];
const NUMBER_CATS: [&str; 3] = ["Nd", "Nl", "No"];

fn main() -> ExitCode {
    let root = workspace_root();
    let data = root.join("tools/unicode-gen/data");
    let out_dir = root.join("crates/topaz_lexer/src");

    let gc = std::fs::read_to_string(data.join("DerivedGeneralCategory.txt"))
        .expect("read DerivedGeneralCategory.txt (vendored under tools/unicode-gen/data)");
    let emoji = std::fs::read_to_string(data.join("emoji-data.txt"))
        .expect("read emoji-data.txt (vendored under tools/unicode-gen/data)");

    let letters = merge(collect(&gc, |prop| LETTER_CATS.contains(&prop)));
    let numbers = merge(collect(&gc, |prop| NUMBER_CATS.contains(&prop)));
    let emoji_ranges = merge(
        collect(&emoji, |prop| prop == "Emoji")
            .into_iter()
            .filter_map(|(lo, hi)| {
                // Drop the ASCII overlap entirely.
                if hi < 0x80 {
                    None
                } else {
                    Some((lo.max(0x80), hi))
                }
            })
            .collect(),
    );

    let folding = std::fs::read_to_string(data.join("CaseFolding.txt"))
        .expect("read CaseFolding.txt (vendored under tools/unicode-gen/data)");
    let unicode_data = std::fs::read_to_string(data.join("UnicodeData.txt"))
        .expect("read UnicodeData.txt (vendored under tools/unicode-gen/data)");
    let norm_data = parse_norm_data(&folding, &unicode_data);

    let tables = render_tables(&letters, &numbers, &emoji_ranges);
    let version = render_version();
    std::fs::write(out_dir.join("unicode_tables.rs"), tables).expect("write unicode_tables.rs");
    std::fs::write(out_dir.join("unicode_version.rs"), version).expect("write unicode_version.rs");
    let topaz_tables = render_topaz_tables(&letters, &numbers, &emoji_ranges, &norm_data);
    std::fs::write(
        root.join("crates/topaz_self_frontend/topaz/unicode_tables.tpz"),
        topaz_tables,
    )
    .expect("write Topaz front-end Unicode tables");

    let resolve_out = root.join("crates/topaz_resolve/src");
    let norm_out = resolve_out.join("unicode_norm");
    std::fs::create_dir_all(&norm_out).expect("create unicode_norm output directory");
    std::fs::write(norm_out.join("mod.rs"), render_norm_module())
        .expect("write unicode_norm/mod.rs");
    std::fs::write(norm_out.join("fold.rs"), render_fold_table(&norm_data))
        .expect("write unicode_norm/fold.rs");
    std::fs::write(
        norm_out.join("decomp.rs"),
        render_decomposition_table(&norm_data),
    )
    .expect("write unicode_norm/decomp.rs");
    std::fs::write(
        norm_out.join("ccc.rs"),
        render_combining_class_table(&norm_data),
    )
    .expect("write unicode_norm/ccc.rs");
    let legacy_norm = resolve_out.join("unicode_norm.rs");
    if legacy_norm.exists() {
        std::fs::remove_file(legacy_norm).expect("remove legacy unicode_norm.rs");
    }

    println!(
        "unicode-gen: UCD {UNICODE_VERSION} -> {} letter, {} number, {} emoji ranges",
        letters.len(),
        numbers.len(),
        emoji_ranges.len()
    );
    ExitCode::SUCCESS
}

/// Locates the workspace root from the executable's manifest dir or
/// the current directory (the tool is always run via `cargo run`).
fn workspace_root() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("run via cargo");
    Path::new(&manifest)
        .ancestors()
        .nth(2)
        .expect("tools/unicode-gen sits two levels under the root")
        .to_path_buf()
}

/// Parses UCD range lines: `0041..005A ; Lu # ...` or `00AA ; Lo # ...`.
fn collect(text: &str, want: impl Fn(&str) -> bool) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split(';').map(str::trim);
        let (Some(range), Some(prop)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !want(prop) {
            continue;
        }
        let (lo, hi) = match range.split_once("..") {
            Some((lo, hi)) => (parse_hex(lo), parse_hex(hi)),
            None => {
                let cp = parse_hex(range);
                (cp, cp)
            }
        };
        out.push((lo, hi));
    }
    out
}

fn parse_hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim(), 16).expect("UCD hex code point")
}

/// Sorts and merges adjacent/overlapping ranges.
fn merge(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (lo, hi) in ranges {
        match out.last_mut() {
            Some((_, prev_hi)) if lo <= *prev_hi + 1 => *prev_hi = (*prev_hi).max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

fn render_tables(letters: &[(u32, u32)], numbers: &[(u32, u32)], emoji: &[(u32, u32)]) -> String {
    let mut out = String::new();
    out.push_str(
        "//! GENERATED by `cargo run -p unicode-gen` — do not edit.\n\
         //!\n\
         //! Source: pinned UCD data vendored under `tools/unicode-gen/data/`\n\
         //! (see `unicode_version.rs`). Tables are sorted inclusive scalar\n\
         //! ranges; membership tests binary-search them.\n\n",
    );
    for (name, ranges, doc) in [
        (
            "LETTER",
            letters,
            "General categories Lu, Ll, Lt, Lm, Lo (SPEC \u{a7}1 UnicodeLetter).",
        ),
        (
            "NUMBER",
            numbers,
            "General categories Nd, Nl, No (SPEC \u{a7}1 UnicodeNumber).",
        ),
        (
            "EMOJI",
            emoji,
            "UCD `Emoji` property minus U+0000..U+007F (SPEC \u{a7}1 Emoji).",
        ),
    ] {
        let _ = writeln!(out, "/// {doc}");
        let _ = writeln!(out, "pub(crate) static {name}: &[(u32, u32)] = &[");
        for chunk in ranges.chunks(4) {
            let row: Vec<String> = chunk
                .iter()
                .map(|(lo, hi)| format!("(0x{lo:05X}, 0x{hi:05X})"))
                .collect();
            let _ = writeln!(out, "    {},", row.join(", "));
        }
        let _ = writeln!(out, "];\n");
    }
    out.push_str(
        "/// True when `ch` falls inside one of the table's inclusive ranges.\n\
         pub(crate) fn contains(table: &[(u32, u32)], ch: char) -> bool {\n\
         \x20   let cp = ch as u32;\n\
         \x20   table\n\
         \x20       .binary_search_by(|&(lo, hi)| {\n\
         \x20           if cp < lo {\n\
         \x20               std::cmp::Ordering::Greater\n\
         \x20           } else if cp > hi {\n\
         \x20               std::cmp::Ordering::Less\n\
         \x20           } else {\n\
         \x20               std::cmp::Ordering::Equal\n\
         \x20           }\n\
         \x20       })\n\
         \x20       .is_ok()\n\
         }\n",
    );
    out
}

fn render_version() -> String {
    format!(
        "//! GENERATED by `cargo run -p unicode-gen` — do not edit.\n\n\
         /// Pinned UCD version the identifier tables were generated from.\n\
         pub const UNICODE_VERSION: &str = \"{UNICODE_VERSION}\";\n"
    )
}

fn render_topaz_tables(
    letters: &[(u32, u32)],
    numbers: &[(u32, u32)],
    emoji: &[(u32, u32)],
    norm: &NormData,
) -> String {
    let mut out = format!(
        "// GENERATED by `cargo run -p unicode-gen` — do not edit.\n\
         // Pinned UCD {UNICODE_VERSION} data is vendored under tools/unicode-gen/data/.\n\n",
    );
    out.push_str(
        "export record CodePointRange {\n\
         \x20 first: int,\n\
         \x20 last: int,\n\
         }\n\n",
    );
    for (name, ranges) in [
        ("LETTER_RANGES", letters),
        ("NUMBER_RANGES", numbers),
        ("EMOJI_RANGES", emoji),
    ] {
        let _ = writeln!(out, "export let {name}: Array<CodePointRange> = [");
        for &(lo, hi) in ranges {
            let _ = writeln!(out, "  CodePointRange {{ first: {lo}, last: {hi} }},");
        }
        out.push_str("]\n\n");
    }
    for (prefix, mappings) in [
        ("CASE_FOLD", &norm.fold),
        ("CANONICAL_DECOMPOSITION", &norm.expanded),
    ] {
        let mut keys = Vec::with_capacity(mappings.len());
        let mut offsets = Vec::with_capacity(mappings.len() + 1);
        let mut values = Vec::new();
        offsets.push(0);
        for (code_point, mapping) in mappings {
            keys.push(*code_point);
            values.extend_from_slice(mapping);
            offsets.push(values.len() as u32);
        }
        render_topaz_int_array(&mut out, &format!("{prefix}_KEYS"), &keys);
        render_topaz_int_array(&mut out, &format!("{prefix}_OFFSETS"), &offsets);
        render_topaz_int_array(&mut out, &format!("{prefix}_VALUES"), &values);
    }
    let combining_keys = norm.ccc.keys().copied().collect::<Vec<_>>();
    let combining_values = norm
        .ccc
        .values()
        .map(|value| u32::from(*value))
        .collect::<Vec<_>>();
    render_topaz_int_array(&mut out, "CANONICAL_COMBINING_CLASS_KEYS", &combining_keys);
    render_topaz_int_array(
        &mut out,
        "CANONICAL_COMBINING_CLASS_VALUES",
        &combining_values,
    );
    out.pop();
    out
}

fn render_topaz_int_array(out: &mut String, name: &str, values: &[u32]) {
    let _ = writeln!(out, "export let {name}: Array<int> = [");
    for chunk in values.chunks(12) {
        let values = chunk
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "  {values},");
    }
    out.push_str("]\n\n");
}

/// Parses `CaseFolding.txt` (statuses C and F) and `UnicodeData.txt`
/// (canonical decompositions, fully expanded; nonzero combining
/// classes) into the resolver's normalization tables.
struct NormData {
    fold: BTreeMap<u32, Vec<u32>>,
    expanded: BTreeMap<u32, Vec<u32>>,
    ccc: BTreeMap<u32, u8>,
}

fn parse_norm_data(folding: &str, unicode_data: &str) -> NormData {
    // Default case folding: statuses C (common) and F (full).
    let mut fold: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for line in folding.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() < 3 || !(fields[1] == "C" || fields[1] == "F") {
            continue;
        }
        let code = u32::from_str_radix(fields[0], 16).expect("fold code");
        let mapping: Vec<u32> = fields[2]
            .split_whitespace()
            .map(|s| u32::from_str_radix(s, 16).expect("fold mapping"))
            .collect();
        fold.insert(code, mapping);
    }

    // Canonical decompositions (no <tag>) and combining classes.
    let mut decomp: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut ccc: BTreeMap<u32, u8> = BTreeMap::new();
    for line in unicode_data.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 6 {
            continue;
        }
        let code = u32::from_str_radix(fields[0], 16).expect("ud code");
        if let Ok(class) = fields[3].parse::<u8>()
            && class != 0
        {
            ccc.insert(code, class);
        }
        let mapping = fields[5].trim();
        if !mapping.is_empty() && !mapping.starts_with('<') {
            let scalars: Vec<u32> = mapping
                .split_whitespace()
                .map(|s| u32::from_str_radix(s, 16).expect("decomp scalar"))
                .collect();
            decomp.insert(code, scalars);
        }
    }
    // Fully expand recursive decompositions at generation time.
    fn expand(code: u32, decomp: &std::collections::BTreeMap<u32, Vec<u32>>, out: &mut Vec<u32>) {
        match decomp.get(&code) {
            Some(parts) => {
                for &part in parts {
                    expand(part, decomp, out);
                }
            }
            None => out.push(code),
        }
    }
    let expanded: BTreeMap<u32, Vec<u32>> = decomp
        .keys()
        .map(|&code| {
            let mut seq = Vec::new();
            expand(code, &decomp, &mut seq);
            (code, seq)
        })
        .collect();

    NormData {
        fold,
        expanded,
        ccc,
    }
}

fn render_norm_header(out: &mut String) {
    let _ = writeln!(
        out,
        "// @generated by unicode-gen from CaseFolding.txt and UnicodeData.txt"
    );
    let _ = writeln!(
        out,
        "// (UCD {UNICODE_VERSION}, vendored under tools/unicode-gen/data). Do not edit."
    );
    let _ = writeln!(out);
}

fn render_norm_module() -> String {
    let mut out = String::new();
    render_norm_header(&mut out);
    out.push_str(
        "mod ccc;\n\
         mod decomp;\n\
         mod fold;\n\n\
         pub use ccc::CCC;\n\
         pub use decomp::DECOMP;\n\
         pub use fold::FOLD;\n",
    );
    out
}

fn render_fold_table(norm: &NormData) -> String {
    let mut out = String::new();
    render_norm_header(&mut out);
    let _ = writeln!(
        out,
        "/// Default case folding (statuses C+F): scalar -> folded sequence."
    );
    let _ = writeln!(out, "pub const FOLD: &[(u32, &[u32])] = &[");
    for (code, mapping) in &norm.fold {
        let seq: Vec<String> = mapping.iter().map(|c| format!("{c:#X}")).collect();
        let _ = writeln!(out, "    ({code:#X}, &[{}]),", seq.join(", "));
    }
    let _ = writeln!(out, "];");
    out
}

fn render_decomposition_table(norm: &NormData) -> String {
    let mut out = String::new();
    render_norm_header(&mut out);
    let _ = writeln!(
        out,
        "/// Fully expanded canonical decompositions (NFD sequences,"
    );
    let _ = writeln!(
        out,
        "/// pre-reordering). Hangul syllables decompose algorithmically."
    );
    let _ = writeln!(out, "pub const DECOMP: &[(u32, &[u32])] = &[");
    for (code, seq) in &norm.expanded {
        let parts: Vec<String> = seq.iter().map(|c| format!("{c:#X}")).collect();
        let _ = writeln!(out, "    ({code:#X}, &[{}]),", parts.join(", "));
    }
    let _ = writeln!(out, "];");
    out
}

fn render_combining_class_table(norm: &NormData) -> String {
    let mut out = String::new();
    render_norm_header(&mut out);
    let _ = writeln!(out, "/// Nonzero canonical combining classes.");
    let _ = writeln!(out, "pub const CCC: &[(u32, u8)] = &[");
    for (code, class) in &norm.ccc {
        let _ = writeln!(out, "    ({code:#X}, {class}),");
    }
    let _ = writeln!(out, "];");
    out
}
