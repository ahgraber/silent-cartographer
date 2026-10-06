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

/// A position in source text: a line and a column, both numbered from 1.
///
/// Lines end at LF, CRLF, or a lone CR. A column counts Unicode scalar values from the start of its
/// line, so it matches an editor's column for every character in the Basic Multilingual Plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineColumn {
    /// The line, from 1.
    pub line: u32,
    /// The column, from 1.
    pub column: u32,
}

/// The line and column of a span's start and of its exclusive end, the position after its last
/// character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineColumnSpan {
    /// The position of the span's first character.
    pub start: LineColumn,
    /// The position after the span's last character.
    pub end: LineColumn,
}

/// Converts byte offsets in one document into [`LineColumn`] positions.
///
/// Independent of [`LineIndex`], which splits lines at LF only because SCIP and git do.
pub struct PositionIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> PositionIndex<'a> {
    /// Build a position index over source text.
    pub fn new(source: &'a str) -> Self {
        let bytes = source.as_bytes();
        let mut line_starts = vec![0usize];
        let mut idx = 0;
        while idx < bytes.len() {
            match bytes[idx] {
                b'\n' => line_starts.push(idx + 1),
                b'\r' => {
                    if bytes.get(idx + 1) == Some(&b'\n') {
                        idx += 1;
                    }
                    line_starts.push(idx + 1);
                }
                _ => {}
            }
            idx += 1;
        }
        Self { source, line_starts }
    }

    /// The position of `offset`, or `None` when it is past the end of the text or inside a
    /// multi-byte character.
    ///
    /// An offset between the CR and the LF of a CRLF has no position of its own and takes the CR's.
    pub fn position(&self, offset: usize) -> Option<LineColumn> {
        if !self.source.is_char_boundary(offset) {
            return None;
        }
        let bytes = self.source.as_bytes();
        let offset = if offset > 0 && bytes[offset - 1] == b'\r' && bytes.get(offset) == Some(&b'\n') {
            offset - 1
        } else {
            offset
        };
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = self.source[self.line_starts[line]..offset].chars().count();
        Some(LineColumn {
            line: line as u32 + 1,
            column: column as u32 + 1,
        })
    }

    /// The positions of a byte span's start and end, or `None` when either has no position.
    pub fn span(&self, start: usize, end: usize) -> Option<LineColumnSpan> {
        Some(LineColumnSpan {
            start: self.position(start)?,
            end: self.position(end)?,
        })
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

    fn at(index: &PositionIndex, offset: usize) -> (u32, u32) {
        let position = index.position(offset).unwrap();
        (position.line, position.column)
    }

    #[test]
    fn position_in_an_ascii_line() {
        let index = PositionIndex::new("let x = 1;\nlet y = 2;\n");
        assert_eq!(at(&index, 15), (2, 5));
    }

    #[test]
    fn column_counts_a_multi_byte_character_once() {
        let source = "abc\u{e9} x";
        let index = PositionIndex::new(source);
        assert_eq!(at(&index, source.find('x').unwrap()), (1, 6));
    }

    #[test]
    fn column_counts_a_character_outside_the_basic_multilingual_plane_once() {
        let source = "\u{1F600} x";
        let index = PositionIndex::new(source);
        assert_eq!(at(&index, source.find('x').unwrap()), (1, 3));
    }

    #[test]
    fn each_line_ending_style_starts_a_new_line() {
        for ending in ["\n", "\r\n", "\r"] {
            let source = ["a", "b", "c", "d", "target"].join(ending);
            let index = PositionIndex::new(&source);
            assert_eq!(at(&index, source.find("target").unwrap()), (5, 1), "ending {ending:?}");
            assert_eq!(
                at(&index, source.find("target").unwrap() + 2),
                (5, 3),
                "ending {ending:?}"
            );
        }
    }

    #[test]
    fn offset_between_cr_and_lf_takes_the_position_of_the_cr() {
        let source = "ab\r\ncd";
        let index = PositionIndex::new(source);
        assert_eq!(at(&index, 2), (1, 3));
        assert_eq!(at(&index, 3), (1, 3));
        assert_eq!(at(&index, 4), (2, 1));
    }

    #[test]
    fn offsets_at_the_start_and_end_of_the_text() {
        let source = "ab\ncd\n";
        let index = PositionIndex::new(source);
        assert_eq!(at(&index, 0), (1, 1));
        assert_eq!(at(&index, source.len()), (3, 1));
        assert!(index.position(source.len() + 1).is_none());
    }

    #[test]
    fn offset_inside_a_multi_byte_character_has_no_position() {
        assert!(PositionIndex::new("\u{e9}").position(1).is_none());
    }

    #[test]
    fn span_end_is_the_position_after_the_closing_brace() {
        let source = "a\nb\nfn target() {}\n";
        let index = PositionIndex::new(source);
        let start = source.find("fn").unwrap();
        let span = index.span(start, start + "fn target() {}".len()).unwrap();
        assert_eq!((span.start.line, span.start.column), (3, 1));
        assert_eq!((span.end.line, span.end.column), (3, 15));
    }

    #[test]
    fn scip_ranges_normalize_by_lf_lines_alone() {
        // A lone CR is not a line break to SCIP, so the whole text is line 0.
        let lone_cr = "a\rfn x";
        let span = range_to_span(
            lone_cr,
            &LineIndex::new(lone_cr),
            SourceRange::new(0, 5, 0, 6),
            PositionEncoding::Utf8,
        )
        .unwrap();
        assert_eq!((span.start, span.end), (5, 6));

        // A CRLF ends at its LF, so line 1 starts after the LF and keeps the CR as its last byte.
        let crlf = "a\r\nfn x\r\n";
        let index = LineIndex::new(crlf);
        let span = range_to_span(crlf, &index, SourceRange::new(1, 3, 1, 4), PositionEncoding::Utf8).unwrap();
        assert_eq!((span.start, span.end), (6, 7));
        let past_end = range_to_span(crlf, &index, SourceRange::new(1, 3, 1, 99), PositionEncoding::Utf8).unwrap();
        assert_eq!(past_end.end, 8);
    }
}
