//! Parser-close string cooking. All positions refer to the original UTF-8 file.
use topaz_diag::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookedRun {
    pub lo: u32,
    pub hi: u32,
    pub source: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookedText {
    pub text: String,
    pub span: Span,
    /// Empty cooked ranges retain removed opening/indent bytes.
    pub runs: Vec<CookedRun>,
}

pub fn cooked_source_map(text: &CookedText) -> String {
    text.runs
        .iter()
        .map(|run| format!("{}:{}:{}:{}", run.lo, run.hi, run.source.lo, run.source.hi))
        .collect::<Vec<_>>()
        .join(";")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringLayout {
    pub removed: Vec<Span>,
    pub offending: Vec<Span>,
}

/// `line_starts` come from text mode, never from interpolation code.
pub fn string_layout(
    source: &str,
    opening: Span,
    closing: Span,
    line_starts: &[u32],
) -> StringLayout {
    let mut layout = StringLayout::default();
    let start = opening.hi as usize;
    let end = closing.lo as usize;
    let span = |lo: usize, hi: usize| Span::new(opening.file, lo as u32, hi as u32);
    let opening_break = if source[start..end].starts_with("\r\n") {
        2
    } else if source[start..end].starts_with('\n') {
        1
    } else {
        0
    };
    if opening_break != 0 {
        layout.removed.push(span(start, start + opening_break));
    }
    let close_line = source[..end].rfind('\n').map_or(0, |i| i + 1);
    let indent = &source[close_line..end];
    if !indent.bytes().all(|b| b == b' ' || b == b'\t') {
        return layout;
    }
    if !indent.is_empty() {
        layout.removed.push(span(close_line, end));
    }
    for &line in line_starts {
        let line = line as usize;
        if line >= close_line || line < start + opening_break {
            continue;
        }
        let hi = source[line..end].find('\n').map_or(end, |i| line + i);
        let raw = &source[line..hi];
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.bytes().all(|b| b == b' ' || b == b'\t') {
            continue;
        }
        if !raw.starts_with(indent) {
            layout.offending.push(span(line, line + raw.len()));
        } else if !indent.is_empty() {
            layout.removed.push(span(line, line + indent.len()));
        }
    }
    layout.removed.sort_by_key(|s| s.lo);
    layout
}

/// The lexer already owns escape diagnostics and layout validation. The parser
/// calls this once after closing the complete literal, preserving segment order.
pub fn cook_string_literal(
    source: &str,
    spans: &[Span],
    layout: &StringLayout,
    multiline: bool,
) -> Vec<CookedText> {
    spans
        .iter()
        .map(|&span| {
            let mut result = CookedText {
                text: String::new(),
                span,
                runs: Vec::new(),
            };
            let mut at = span.lo as usize;
            while at < span.hi as usize {
                let lo = result.text.len() as u32;
                if let Some(removed) = layout
                    .removed
                    .iter()
                    .find(|s| s.lo as usize <= at && at < s.hi as usize)
                {
                    let end = removed.hi.min(span.hi) as usize;
                    result.runs.push(CookedRun {
                        lo,
                        hi: lo,
                        source: Span::new(span.file, at as u32, end as u32),
                    });
                    at = end;
                    continue;
                }
                let start = at;
                let rest = &source[at..span.hi as usize];
                let ch = rest.chars().next().expect("text character");
                at += ch.len_utf8();
                if multiline && rest.starts_with("\r\n") {
                    at += 1;
                    result.text.push('\n');
                } else if ch == '\\' {
                    if let Some(escaped) = source[at..span.hi as usize].chars().next() {
                        at += escaped.len_utf8();
                        result.text.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            other => other,
                        });
                    }
                } else {
                    result.text.push(ch);
                }
                result.runs.push(CookedRun {
                    lo,
                    hi: result.text.len() as u32,
                    source: Span::new(span.file, start as u32, at as u32),
                });
            }
            result
        })
        .collect()
}
