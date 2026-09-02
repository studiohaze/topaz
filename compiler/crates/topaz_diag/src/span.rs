/// Identity of a source file inside a [`crate::SourceMap`].
///
/// Spans carry their `FileId` so a span is meaningful on its own in a
/// multi-file compilation (CDR-001 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A half-open byte range `[lo, hi)` into the source file `file`.
///
/// Offsets are byte offsets into the UTF-8 source text. The source
/// loader guarantees every file fits in `u32` (see
/// [`crate::SourceMap::add_file`]), so `u32` offsets are total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

impl Span {
    /// Creates a span. `lo <= hi` is a caller invariant (checked in
    /// debug builds).
    pub fn new(file: FileId, lo: u32, hi: u32) -> Self {
        debug_assert!(lo <= hi, "span lo {lo} must not exceed hi {hi}");
        Self { file, lo, hi }
    }

    /// Byte length of the span.
    pub fn len(&self) -> u32 {
        self.hi - self.lo
    }

    /// True when the span covers zero bytes (a caret position).
    pub fn is_empty(&self) -> bool {
        self.lo == self.hi
    }

    /// Smallest span covering both `self` and `other`.
    ///
    /// Both spans must reference the same file (checked in debug
    /// builds); merging across files has no meaning.
    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(
            self.file, other.file,
            "cannot merge spans from different files"
        );
        Span {
            file: self.file,
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_covers_both() {
        let f = FileId(0);
        let a = Span::new(f, 4, 9);
        let b = Span::new(f, 7, 20);
        assert_eq!(a.merge(b), Span::new(f, 4, 20));
        assert_eq!(b.merge(a), Span::new(f, 4, 20));
    }

    #[test]
    fn empty_span() {
        let s = Span::new(FileId(1), 5, 5);
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }
}
