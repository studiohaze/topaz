//! Plain-text diagnostic rendering (CDR-001 §5): `file:line:col`
//! locations and caret underlines. 1-based lines; 1-based Unicode
//! scalar columns; caret alignment over wide glyphs is best-effort by
//! design until a future diagnostics CDR.

use std::fmt::Write as _;

use crate::diagnostic::{Diagnostic, Label};
use crate::source_map::SourceMap;

/// Renders one diagnostic to plain text.
///
/// Shape:
///
/// ```text
/// error[TPZ2001]: expected an expression
///  --> demo.tpz:2:9
///   |
/// 2 | let x =
///   |         ^ expected an expression
///   = note: assignments are statements in Topaz
/// ```
pub fn render(diag: &Diagnostic, map: &SourceMap) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}[{}]: {}",
        diag.severity.as_str(),
        diag.code,
        diag.message
    );

    // Gutter width: widest line number across every rendered label.
    let width = std::iter::once(&diag.primary)
        .chain(diag.secondary.iter())
        .map(|l| map.file(l.span.file).line_col(l.span.lo).line)
        .max()
        .map(|line| line.to_string().len())
        .unwrap_or(1);

    render_label(&mut out, map, &diag.primary, '^', width);
    for label in &diag.secondary {
        render_label(&mut out, map, label, '-', width);
    }
    for note in &diag.notes {
        let _ = writeln!(out, "{:width$} = note: {note}", "");
    }
    out
}

/// Render a diagnostic as a single-line JSON object — the machine-readable form
/// the data-oriented `Diagnostic` was designed for (CDR-001 §5: "JSON/LSP
/// later"). Field order is STABLE; positions are 1-based line/column resolved
/// through the source map, and each label exposes the start plus the exclusive
/// end. Zero-dependency: built by hand (no serde), so the output is fixed and
/// deterministic.
pub fn render_json(diag: &Diagnostic, map: &SourceMap) -> String {
    let mut s = String::from("{\"code\":");
    push_json_string(&mut s, diag.code.as_str());
    s.push_str(",\"severity\":");
    push_json_string(&mut s, diag.severity.as_str());
    s.push_str(",\"message\":");
    push_json_string(&mut s, &diag.message);
    s.push_str(",\"primary\":");
    push_label_json(&mut s, map, &diag.primary);
    s.push_str(",\"secondary\":[");
    for (i, l) in diag.secondary.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_label_json(&mut s, map, l);
    }
    s.push_str("],\"notes\":[");
    for (i, n) in diag.notes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_json_string(&mut s, n);
    }
    s.push_str("]}");
    s
}

/// One label as `{"file","line","col","endLine","endCol","lo","hi","message"}`.
/// `file` is the span's source name (multi-module diagnostics resolve to their
/// own file); `line`/`col` are 1-based with `endCol` the column AFTER the span's
/// last scalar (exclusive); `lo`/`hi` are the raw byte offsets (so an LSP adapter
/// can recompute UTF-16 columns without scalar/byte ambiguity).
fn push_label_json(s: &mut String, map: &SourceMap, label: &Label) {
    let file = map.file(label.span.file);
    let start = file.line_col(label.span.lo);
    let end = file.line_col(label.span.hi);
    s.push_str("{\"file\":");
    push_json_string(s, file.name());
    let _ = write!(
        s,
        ",\"line\":{},\"col\":{},\"endLine\":{},\"endCol\":{},\"lo\":{},\"hi\":{},\"message\":",
        start.line, start.col, end.line, end.col, label.span.lo, label.span.hi
    );
    push_json_string(s, &label.message);
    s.push('}');
}

/// Append `raw` as a JSON string literal (RFC 8259 escaping; control chars
/// below U+0020 as lowercase `\uXXXX`).
fn push_json_string(s: &mut String, raw: &str) {
    s.push('"');
    for c in raw.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

fn render_label(out: &mut String, map: &SourceMap, label: &Label, mark: char, width: usize) {
    let file = map.file(label.span.file);
    let pos = file.line_col(label.span.lo);
    let line_text = file.line_text(pos.line);

    let _ = writeln!(
        out,
        "{:width$}--> {}:{}:{}",
        "",
        file.name(),
        pos.line,
        pos.col
    );
    let _ = writeln!(out, "{:width$} |", "");
    let _ = writeln!(out, "{:>width$} | {line_text}", pos.line);

    // Underline: from the label's start column to its end within this
    // line (multi-line spans underline to the end of the first line —
    // best-effort per CDR-001 §5). Always at least one mark.
    let pad = (pos.col - 1) as usize;
    let end = file.line_col(label.span.hi);
    let marks = if end.line == pos.line {
        ((end.col - pos.col) as usize).max(1)
    } else {
        line_text.chars().count().saturating_sub(pad).max(1)
    };
    let underline: String = std::iter::repeat_n(mark, marks).collect();
    let _ = write!(out, "{:width$} | {:pad$}{underline}", "", "");
    if label.message.is_empty() {
        out.push('\n');
    } else {
        let _ = writeln!(out, " {}", label.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::Code;
    use crate::diagnostic::{Diagnostic, Label};
    use crate::span::Span;

    #[test]
    fn renders_primary_label_with_caret_run() {
        let mut map = SourceMap::new();
        let id = map.add_file("demo.tpz", "let answer = 42\n").unwrap();
        // Span over `answer` (bytes 4..10).
        let diag = Diagnostic::error(
            Code::new("TPZ2001"),
            "something about this binding",
            Label::new(Span::new(id, 4, 10), "the binding"),
        );
        let expected = "\
error[TPZ2001]: something about this binding
 --> demo.tpz:1:5
  |
1 | let answer = 42
  |     ^^^^^^ the binding
";
        assert_eq!(render(&diag, &map), expected);
    }

    #[test]
    fn renders_json_object_with_stable_fields() {
        let mut map = SourceMap::new();
        let id = map.add_file("demo.tpz", "let answer = 42\n").unwrap();
        // The message exercises escaping (quote + newline); span over `answer`.
        let diag = Diagnostic::error(
            Code::new("TPZ2001"),
            "bad \"binding\"\nhere",
            Label::new(Span::new(id, 4, 10), "the binding"),
        );
        assert_eq!(
            render_json(&diag, &map),
            "{\"code\":\"TPZ2001\",\"severity\":\"error\",\"message\":\"bad \\\"binding\\\"\\nhere\",\
             \"primary\":{\"file\":\"demo.tpz\",\"line\":1,\"col\":5,\"endLine\":1,\"endCol\":11,\
             \"lo\":4,\"hi\":10,\"message\":\"the binding\"},\"secondary\":[],\"notes\":[]}"
        );
    }

    #[test]
    fn renders_secondary_label_and_note_with_scalar_columns() {
        let mut map = SourceMap::new();
        let id = map
            .add_file("유니코드.tpz", "let 한글 = 1\nlet x = 한글\n")
            .unwrap();
        // Line 1 is bytes 0..14 ("let 한글 = 1" + \n), so line 2
        // starts at byte 15; "let x = " is 8 bytes, so the `한글` use
        // is bytes 23..29. Secondary on the definition: bytes 4..10.
        let diag = Diagnostic::error(
            Code::new("TPZ2002"),
            "demo",
            Label::new(Span::new(id, 23, 29), "used here"),
        )
        .with_secondary(Span::new(id, 4, 10), "defined here")
        .with_note("columns count Unicode scalars");
        let expected = "\
error[TPZ2002]: demo
 --> 유니코드.tpz:2:9
  |
2 | let x = 한글
  |         ^^ used here
 --> 유니코드.tpz:1:5
  |
1 | let 한글 = 1
  |     -- defined here
  = note: columns count Unicode scalars
";
        assert_eq!(render(&diag, &map), expected);
    }

    #[test]
    fn empty_span_renders_single_caret() {
        let mut map = SourceMap::new();
        let id = map.add_file("eof.tpz", "let x =\n").unwrap();
        let diag = Diagnostic::error(
            Code::new("TPZ2003"),
            "expected an expression",
            Label::new(Span::new(id, 7, 7), ""),
        );
        let expected = "\
error[TPZ2003]: expected an expression
 --> eof.tpz:1:8
  |
1 | let x =
  |        ^
";
        assert_eq!(render(&diag, &map), expected);
    }

    #[test]
    fn multiline_span_underlines_to_end_of_first_line() {
        let mut map = SourceMap::new();
        let id = map.add_file("multi.tpz", "first line\nsecond\n").unwrap();
        // Span from byte 6 ("line") through the second line.
        let diag = Diagnostic::error(
            Code::new("TPZ2004"),
            "spans lines",
            Label::new(Span::new(id, 6, 17), "starts here"),
        );
        let expected = "\
error[TPZ2004]: spans lines
 --> multi.tpz:1:7
  |
1 | first line
  |       ^^^^ starts here
";
        assert_eq!(render(&diag, &map), expected);
    }

    #[test]
    fn gutter_width_follows_widest_line_number() {
        let mut map = SourceMap::new();
        let src = "a\n".repeat(12);
        let id = map.add_file("wide.tpz", src).unwrap();
        // Line 11 starts at byte 20.
        let diag = Diagnostic::error(
            Code::new("TPZ2005"),
            "demo",
            Label::new(Span::new(id, 20, 21), "here"),
        );
        let expected = "\
error[TPZ2005]: demo
  --> wide.tpz:11:1
   |
11 | a
   | ^ here
";
        assert_eq!(render(&diag, &map), expected);
    }
}
