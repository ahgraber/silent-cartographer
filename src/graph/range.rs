//! Range normalization: mapping a semantic occurrence's range, expressed in a document's declared
//! position encoding, onto tree-sitter byte offsets.
//!
//! A SCIP range counts characters per line in one of three encodings (UTF-8 bytes, UTF-16 code
//! units, UTF-32 code points). The syntax tree indexes bytes. Normalizing through the declared
//! encoding — rather than assuming one — is what keeps the join from silently misaligning on
//! non-ASCII source, which the join's text-equality guard then makes loud.

use crate::semantic::model::{PositionEncoding, SourceRange};

/// A half-open byte span `[start, end)` into a source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteSpan {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl ByteSpan {
    /// Whether this span fully contains `other`.
    pub fn contains(&self, other: &ByteSpan) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// Precomputed byte offset of the start of each line in a document, for O(1) line lookup.
pub struct LineIndex {
    line_starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    /// Build a line index over source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (idx, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            line_starts,
            len: source.len(),
        }
    }

    /// The byte offset at which `line` (zero-based) starts, or `None` if the line is out of range.
    fn line_start(&self, line: u32) -> Option<usize> {
        self.line_starts.get(line as usize).copied()
    }

    /// The byte offset just past the end of `line` (its terminating newline or end of file).
    fn line_end(&self, line: u32) -> Option<usize> {
        let next = line as usize + 1;
        if next < self.line_starts.len() {
            // The newline terminating this line sits one byte before the next line's start.
            Some(self.line_starts[next].saturating_sub(1))
        } else if (line as usize) < self.line_starts.len() {
            Some(self.len)
        } else {
            None
        }
    }
}

/// Convert a `(line, character)` position in `encoding` into a byte offset within `source`.
///
/// Returns `None` if the position falls outside the document — a coordinate the semantic and syntax
/// oracles cannot reconcile, which the join records as unaligned rather than misattributing.
pub fn position_to_byte(
    source: &str,
    index: &LineIndex,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> Option<usize> {
    let line_start = index.line_start(line)?;
    let line_end = index.line_end(line)?;
    let line_text = &source[line_start..line_end];

    if character == 0 {
        return Some(line_start);
    }

    let mut units = 0u32;
    for (byte_offset, ch) in line_text.char_indices() {
        if units >= character {
            return Some(line_start + byte_offset);
        }
        units += match encoding {
            PositionEncoding::Utf8 => ch.len_utf8() as u32,
            PositionEncoding::Utf16 => ch.len_utf16() as u32,
            PositionEncoding::Utf32 => 1,
        };
    }
    // The character offset reaches or exceeds the line's content; clamp to the content end.
    Some(line_start + line_text.len())
}

/// Normalize a source range into a byte span within `source`.
pub fn range_to_span(
    source: &str,
    index: &LineIndex,
    range: SourceRange,
    encoding: PositionEncoding,
) -> Option<ByteSpan> {
    let start = position_to_byte(source, index, range.start_line, range.start_char, encoding)?;
    let end = position_to_byte(source, index, range.end_line, range.end_char, encoding)?;
    if end < start {
        return None;
    }
    Some(ByteSpan { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_ascii_maps_char_to_byte() {
        let source = "let x = 1;\nlet y = 2;\n";
        let index = LineIndex::new(source);
        // "y" on line 1 is at char 4 (UTF-8 = byte 4 within the line).
        let span = range_to_span(source, &index, SourceRange::new(1, 4, 1, 5), PositionEncoding::Utf8).unwrap();
        assert_eq!(&source[span.start..span.end], "y");
    }

    #[test]
    fn utf16_encoding_accounts_for_wide_chars() {
        // A 4-byte emoji is 2 UTF-16 code units but 4 UTF-8 bytes; the identifier after it must map
        // to the right byte offset under each encoding.
        let source = "let \u{1F600}x = 1;\n";
        let index = LineIndex::new(source);
        // Under UTF-16, "x" sits at char offset 4 + 2 = 6.
        let span16 = range_to_span(source, &index, SourceRange::new(0, 6, 0, 7), PositionEncoding::Utf16).unwrap();
        assert_eq!(&source[span16.start..span16.end], "x");
        // Under UTF-8, the same "x" sits at char (byte) offset 4 + 4 = 8.
        let span8 = range_to_span(source, &index, SourceRange::new(0, 8, 0, 9), PositionEncoding::Utf8).unwrap();
        assert_eq!(&source[span8.start..span8.end], "x");
    }

    #[test]
    fn out_of_range_line_is_none() {
        let source = "one line\n";
        let index = LineIndex::new(source);
        assert!(range_to_span(source, &index, SourceRange::new(9, 0, 9, 1), PositionEncoding::Utf8).is_none());
    }
}
