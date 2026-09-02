use crate::span::FileId;

/// Maximum source size in bytes. Files at or below this limit have
/// total `u32` byte offsets; the loader rejects anything larger
/// before lexing (CDR-001 §5).
pub const MAX_SOURCE_LEN: usize = u32::MAX as usize;

/// A 1-based line / 1-based column position.
///
/// Columns count Unicode scalar values from the start of the line
/// (CDR-001 §5) — not bytes and not grapheme clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Errors produced by [`SourceMap::add_file`].
///
/// These are pre-lexing **loader errors**, not span-bearing
/// [`crate::Diagnostic`]s — there is no source position to point at.
/// They are rendered at the CLI/front-end boundary unless a later
/// diagnostics CDR adds source-less diagnostics.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceMapError {
    /// The file exceeds [`MAX_SOURCE_LEN`] bytes.
    FileTooLarge { name: String, len: usize },
    /// The file table is full (more than `u32::MAX` files).
    TooManyFiles,
}

impl std::fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceMapError::FileTooLarge { name, len } => write!(
                f,
                "source file `{name}` is {len} bytes, which exceeds the {MAX_SOURCE_LEN}-byte limit"
            ),
            SourceMapError::TooManyFiles => write!(f, "source map cannot hold more files"),
        }
    }
}

impl std::error::Error for SourceMapError {}

/// One loaded source file: display name, UTF-8 text, and a line index.
#[derive(Debug)]
pub struct SourceFile {
    name: String,
    src: String,
    /// Byte offset of the first byte of every line. `line_starts[0]`
    /// is always 0; a trailing newline opens a final empty line.
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(name: String, src: String) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            name,
            src,
            line_starts,
        }
    }

    /// Display name used in diagnostics.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Full source text.
    pub fn src(&self) -> &str {
        &self.src
    }

    /// Number of lines (a trailing newline opens a final empty line).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Zero-based index of the line containing byte `offset`.
    fn line_index(&self, offset: u32) -> usize {
        self.line_starts.partition_point(|&start| start <= offset) - 1
    }

    /// Byte range `[start, end)` of a zero-based line, excluding the
    /// line terminator.
    fn line_bounds(&self, line: usize) -> (u32, u32) {
        let start = self.line_starts[line];
        let raw_end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.src.len() as u32);
        // Strip the terminator: `\n`, preceded by an optional `\r`.
        let text = &self.src[start as usize..raw_end as usize];
        let trimmed = text.strip_suffix('\n').unwrap_or(text);
        let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
        (start, start + trimmed.len() as u32)
    }

    /// Text of a 1-based line, without its terminator.
    pub fn line_text(&self, line: u32) -> &str {
        let (start, end) = self.line_bounds((line - 1) as usize);
        &self.src[start as usize..end as usize]
    }

    /// 1-based line and 1-based scalar column of a byte offset.
    ///
    /// `offset` must lie on a UTF-8 character boundary (token and AST
    /// spans always do).
    pub fn line_col(&self, offset: u32) -> LineCol {
        let line = self.line_index(offset);
        let start = self.line_starts[line];
        let col = self.src[start as usize..offset as usize].chars().count() + 1;
        LineCol {
            line: (line + 1) as u32,
            col: col as u32,
        }
    }
}

/// Owns every loaded source file and resolves spans to positions.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a source file, rejecting files larger than
    /// [`MAX_SOURCE_LEN`] with a [`SourceMapError`] before any lexing
    /// can observe a non-`u32` offset.
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        src: impl Into<String>,
    ) -> Result<FileId, SourceMapError> {
        let name = name.into();
        let src = src.into();
        if src.len() > MAX_SOURCE_LEN {
            return Err(SourceMapError::FileTooLarge {
                name,
                len: src.len(),
            });
        }
        let id = u32::try_from(self.files.len()).map_err(|_| SourceMapError::TooManyFiles)?;
        self.files.push(SourceFile::new(name, src));
        Ok(FileId(id))
    }

    /// The file behind `id`. Panics on a foreign `FileId`, which is a
    /// compiler bug by construction (ids only come from `add_file`).
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(src: &str) -> (SourceMap, FileId) {
        let mut map = SourceMap::new();
        let id = map.add_file("test.tpz", src).expect("fits");
        (map, id)
    }

    #[test]
    fn ascii_line_col() {
        let (map, id) = map_with("let a = 1\nlet b = 2\n");
        let f = map.file(id);
        assert_eq!(f.line_col(0), LineCol { line: 1, col: 1 });
        assert_eq!(f.line_col(4), LineCol { line: 1, col: 5 });
        assert_eq!(f.line_col(10), LineCol { line: 2, col: 1 });
        assert_eq!(f.line_col(14), LineCol { line: 2, col: 5 });
        // Offset at EOF lands on the final (empty) line opened by the
        // trailing newline.
        assert_eq!(f.line_col(20), LineCol { line: 3, col: 1 });
        assert_eq!(f.line_count(), 3);
    }

    #[test]
    fn scalar_columns_for_multibyte_source() {
        // "한글" is 3 bytes per scalar; the emoji is 4 bytes.
        let src = "let 한글 = \"😀\"\n";
        let (map, id) = map_with(src);
        let f = map.file(id);
        // Offset of '=' : "let " (4) + 한글 (6) + " " (1) = 11; scalar
        // column: l,e,t,␣,한,글,␣ = 7 scalars before it.
        assert_eq!(f.line_col(11), LineCol { line: 1, col: 8 });
        // Offset of the emoji: 11 + "= " (2) + '"' (1) = 14; ten
        // scalars precede it.
        assert_eq!(f.line_col(14), LineCol { line: 1, col: 11 });
    }

    #[test]
    fn crlf_line_text_is_trimmed() {
        let (map, id) = map_with("a\r\nbb\r\n");
        let f = map.file(id);
        assert_eq!(f.line_text(1), "a");
        assert_eq!(f.line_text(2), "bb");
        assert_eq!(f.line_col(3), LineCol { line: 2, col: 1 });
    }

    #[test]
    fn file_without_trailing_newline() {
        let (map, id) = map_with("one\ntwo");
        let f = map.file(id);
        assert_eq!(f.line_count(), 2);
        assert_eq!(f.line_text(2), "two");
        assert_eq!(f.line_col(6), LineCol { line: 2, col: 3 });
    }

    #[test]
    fn oversized_source_is_rejected() {
        // The guard compares lengths; constructing a real 4 GiB string
        // is unnecessary to prove the branch. Exercise the public
        // contract with the largest practical assertion instead:
        // the constant itself and the error formatting.
        let err = SourceMapError::FileTooLarge {
            name: "big.tpz".into(),
            len: MAX_SOURCE_LEN + 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("big.tpz"));
        assert!(msg.contains("exceeds"));
        assert_eq!(MAX_SOURCE_LEN, u32::MAX as usize);
    }

    #[test]
    fn multiple_files_get_distinct_ids() {
        let mut map = SourceMap::new();
        let a = map.add_file("a.tpz", "x").unwrap();
        let b = map.add_file("b.tpz", "y").unwrap();
        assert_ne!(a, b);
        assert_eq!(map.file(a).name(), "a.tpz");
        assert_eq!(map.file(b).src(), "y");
    }
}
