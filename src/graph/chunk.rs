//! The chunk splitter: dividing a passage into the bounded texts the embedding model receives.
//!
//! A chunk is the passage's bounded header followed by a contiguous run of its content, and the
//! chunk size bounds the whole text — header included — as measured by the model's own tokenizer.
//! Content is divided into units at the strongest boundary available and units are packed into
//! chunks up to the size; a unit that alone exceeds the room available is subdivided at the next
//! level down:
//!
//! 1. syntax nodes, read from the tree the build already parses (never by re-parsing);
//! 2. prose boundaries — paragraph, then sentence, then line — inside a unit the syntax oracle
//!    reports as one indivisible construct;
//! 3. a plain token window, as the terminal fallback for content offering no boundary at all.
//!
//! The chunks of one passage tile its content: every content byte lies in at least one chunk, and
//! with an overlap of none in exactly one. A non-zero overlap prepends whole trailing units of the
//! preceding chunk, up to the overlap budget; within a windowed run there are no units, so overlap
//! there is plain token overlap.

use super::embed;
use super::range::ByteSpan;
use super::syntax::SyntaxTree;

/// The recommended chunk size: above the ordinary passage (so roughly 96% of passages stay a
/// single chunk) and below the dilution ceiling, with headroom for the header and an operator's
/// overlap.
pub const DEFAULT_CHUNK_SIZE: usize = 512;

/// The recommended overlap between adjacent chunks, in model tokens.
///
/// Chosen by the boundary-sensitivity probe over the five dogfood workspaces: 64 lifted rank-1 on
/// boundary-spanning dense queries 70.5% → 77.3% at +2.6% vectors, consistently across every
/// workspace, and 128's further gain was half of 64's for more than double the carried tokens.
pub const DEFAULT_CHUNK_OVERLAP: usize = 64;

/// The smallest accepted chunk size.
///
/// At 8 the whole-chunk bound holds by arithmetic in every case the splitter can produce: the
/// bounded header is at most half the size (4 tokens), the joining newline is one, and a single
/// character — the terminal fallback's minimum emission — is at most two, so even the fallback
/// stays within the size. Smaller values could let a fallback emission exceed the bound.
pub const MIN_CHUNK_SIZE: usize = 8;

/// The chunk parameters a build runs under, recorded with the store as part of the semantic-index
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkParams {
    /// The bound on the whole embedded text of one chunk, header included, in model tokens.
    pub chunk_size: usize,
    /// The content carried from the tail of each chunk's predecessor, in model tokens.
    pub overlap: usize,
}

impl Default for ChunkParams {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            overlap: DEFAULT_CHUNK_OVERLAP,
        }
    }
}

/// A passage's header, carried on every chunk so a mid-passage chunk still says what symbol its
/// content belongs to.
#[derive(Debug, Clone, Copy)]
pub struct PassageHeader<'a> {
    /// The identity-bearing head: name words, kind, module-path words, and signature.
    pub identity: &'a str,
    /// The symbol's own documentation — the part a header budget may trim from its tail, because
    /// it also appears whole in the passage's first chunk and in the lexical index.
    pub documentation: Option<&'a str>,
}

/// One chunk: the text handed to the embedding model, and the content range it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The embedded text: the bounded header, a newline, and the content run (overlap included).
    pub text: String,
    /// The half-open byte range of the passage content this chunk covers, overlap included.
    pub content_range: (usize, usize),
}

/// One content unit awaiting packing. Windowed atoms come from the terminal token-window fallback
/// and carry their own overlap, so the packer treats each as a complete chunk.
struct Atom {
    range: (usize, usize),
    windowed: bool,
}

/// Split one passage into chunks.
///
/// `syntax` locates the content within its parsed document when the content is a contiguous
/// document span — a leaf symbol's body. Synthesized content (a container's interface tier) has no
/// location and divides on prose boundaries.
///
/// Content that fits beside the header returns exactly one chunk covering it whole. Empty content
/// returns one chunk of the header alone, so every passage carries at least one vector.
pub fn split_passage(
    header: &PassageHeader<'_>,
    content: &str,
    syntax: Option<(&SyntaxTree, ByteSpan)>,
    params: &ChunkParams,
) -> Vec<Chunk> {
    let header_text = bounded_header(header, params.chunk_size);
    if content.is_empty() {
        return vec![Chunk {
            text: header_text,
            content_range: (0, 0),
        }];
    }
    if fits(&header_text, content, params.chunk_size) {
        return vec![Chunk {
            text: chunk_text(&header_text, content),
            content_range: (0, content.len()),
        }];
    }
    let splitter = Splitter {
        content,
        syntax,
        header_text: &header_text,
        params,
    };
    let mut atoms = Vec::new();
    let descend = syntax.map(|(_, span)| span);
    splitter.atomize((0, content.len()), descend, &mut atoms);
    splitter.pack(&atoms)
}

/// The embedded text of one chunk: the header, a newline, and the content run — the same joining
/// the passage render uses.
fn chunk_text(header: &str, slice: &str) -> String {
    format!("{header}\n{slice}")
}

/// Whether a chunk of `header` plus `slice` is within the chunk size, measured by the model's own
/// tokenizer.
fn fits(header: &str, slice: &str, chunk_size: usize) -> bool {
    embed::count_tokens(&chunk_text(header, slice)) <= chunk_size
}

/// The header actually carried on each chunk, bounded to at most half the chunk size.
///
/// The documentation tail is trimmed first — it appears whole in the passage's first chunk and in
/// the lexical index. A head that alone exceeds the budget is itself tail-trimmed as the terminal
/// fallback: the hard size bound outranks header wholeness, and the head's tail (the signature's
/// parameter run) is its least identifying part.
fn bounded_header(header: &PassageHeader<'_>, chunk_size: usize) -> String {
    let budget = (chunk_size / 2).max(1);
    let full = match header.documentation {
        Some(docs) if !docs.is_empty() => format!("{}\n{}", header.identity, docs),
        _ => header.identity.to_string(),
    };
    if embed::count_tokens(&full) <= budget {
        return full;
    }
    if embed::count_tokens(header.identity) <= budget {
        // Trim the documentation tail to the room the head leaves.
        if let Some(docs) = header.documentation {
            let with_docs = |cut: usize| format!("{}\n{}", header.identity, &docs[..cut]);
            if let Some(cut) = largest_char_cut(docs, |cut| cut > 0 && embed::count_tokens(&with_docs(cut)) <= budget)
            {
                return with_docs(cut);
            }
        }
        return header.identity.to_string();
    }
    // Terminal fallback: the head alone exceeds the budget.
    let cut = largest_char_cut(header.identity, |cut| {
        cut > 0 && embed::count_tokens(&header.identity[..cut]) <= budget
    });
    match cut {
        Some(cut) => header.identity[..cut].to_string(),
        None => header.identity.chars().take(1).collect(),
    }
}

/// The smallest char-boundary cut of `s` satisfying `ok`, by binary search with a linear
/// step-forward (token counts are near- but not strictly monotone in text length). Cut 0 — the
/// whole string — is a candidate. `None` when no cut satisfies it.
fn smallest_char_cut<F: Fn(usize) -> bool>(s: &str, ok: F) -> Option<usize> {
    let mut cuts: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    cuts.push(s.len());
    let (mut lo, mut hi) = (0usize, cuts.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if ok(cuts[mid]) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let mut idx = lo;
    while idx < cuts.len() && !ok(cuts[idx]) {
        idx += 1;
    }
    (idx < cuts.len()).then(|| cuts[idx])
}

/// The largest char-boundary cut of `s` satisfying `ok`, by binary search with a linear step-back
/// (token counts are near- but not strictly monotone in text length). `None` when no cut
/// satisfies it.
fn largest_char_cut<F: Fn(usize) -> bool>(s: &str, ok: F) -> Option<usize> {
    let cuts: Vec<usize> = s.char_indices().map(|(i, _)| i).skip(1).chain([s.len()]).collect();
    if cuts.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (0usize, cuts.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if ok(cuts[mid]) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let mut idx = lo;
    while idx > 0 && !ok(cuts[idx - 1]) {
        idx -= 1;
    }
    (idx > 0).then(|| cuts[idx - 1])
}

struct Splitter<'a> {
    content: &'a str,
    syntax: Option<(&'a SyntaxTree, ByteSpan)>,
    header_text: &'a str,
    params: &'a ChunkParams,
}

impl Splitter<'_> {
    fn slice(&self, range: (usize, usize)) -> &str {
        &self.content[range.0..range.1]
    }

    /// Divide `range` into atoms, each fitting beside the header, descending the boundary
    /// hierarchy only where the level above overshoots. `descend` is the document-local span of
    /// the syntax node this range came from, when it came from one.
    fn atomize(&self, range: (usize, usize), descend: Option<ByteSpan>, out: &mut Vec<Atom>) {
        if fits(self.header_text, self.slice(range), self.params.chunk_size) {
            out.push(Atom { range, windowed: false });
            return;
        }
        if let Some(units) = self.syntax_units(range, descend) {
            for (sub, node) in units {
                self.atomize(sub, node, out);
            }
            return;
        }
        for cuts in [
            paragraph_cuts(self.content, range),
            sentence_cuts(self.content, range),
            line_cuts(self.content, range),
        ] {
            if !cuts.is_empty() {
                let mut start = range.0;
                for cut in cuts.into_iter().chain([range.1]) {
                    self.atomize((start, cut), None, out);
                    start = cut;
                }
                return;
            }
        }
        self.window(range, out);
    }

    /// The sub-units of `range` cut at the direct children of its syntax node, each paired with
    /// the child span to descend into next. `None` when the range has no node or the node offers
    /// no boundary within it.
    #[allow(clippy::type_complexity)]
    fn syntax_units(
        &self,
        range: (usize, usize),
        descend: Option<ByteSpan>,
    ) -> Option<Vec<((usize, usize), Option<ByteSpan>)>> {
        let (tree, content_span) = self.syntax?;
        let node_span = descend?;
        let children = tree.child_spans_within(node_span);
        // A child span in content-local coordinates.
        let local = |span: &ByteSpan| {
            (span.start >= content_span.start && span.end <= content_span.end)
                .then(|| (span.start - content_span.start, span.end - content_span.start))
        };
        let mut cuts: Vec<(usize, ByteSpan)> = children
            .iter()
            .filter_map(|child| local(child).map(|(start, _)| (start, *child)))
            .filter(|(start, _)| *start > range.0 && *start < range.1)
            .collect();
        cuts.sort_by_key(|(start, _)| *start);
        cuts.dedup_by_key(|(start, _)| *start);
        if cuts.is_empty() {
            return None;
        }
        // The node whose content-local start equals a unit's start is the unit's descend target.
        let node_at = |start: usize| {
            children
                .iter()
                .find(|child| local(child).is_some_and(|(s, _)| s == start))
                .copied()
        };
        let mut units = Vec::new();
        let mut start = range.0;
        for (cut, _) in &cuts {
            units.push(((start, *cut), node_at(start)));
            start = *cut;
        }
        units.push(((start, range.1), node_at(start)));
        Some(units)
    }

    /// The terminal fallback: divide a boundary-less range by plain token window. Each window is
    /// the largest run fitting beside the header; a non-zero overlap re-carries the trailing
    /// tokens of the preceding window.
    fn window(&self, range: (usize, usize), out: &mut Vec<Atom>) {
        let mut start = range.0;
        let mut prev_end = range.0;
        while start < range.1 {
            let slice = &self.content[start..range.1];
            let end = match largest_char_cut(slice, |cut| {
                cut > 0 && fits(self.header_text, &slice[..cut], self.params.chunk_size)
            }) {
                Some(cut) => start + cut,
                // Even one char overflows beside the header; take one char rather than stall.
                None => start + slice.chars().next().map_or(1, char::len_utf8),
            };
            // Progress over any overlap that would re-cover the previous window entirely.
            let end = end.max(prev_end + 1).min(range.1).max(start + 1);
            out.push(Atom {
                range: (start, end),
                windowed: true,
            });
            if end >= range.1 {
                return;
            }
            prev_end = end;
            start = if self.params.overlap > 0 {
                // The next window re-carries the largest trailing suffix within the overlap
                // budget: the smallest cut whose suffix fits it. The suffix is also capped at
                // half the window, so every window advances by at least half its span and an
                // overlap near the chunk size cannot degenerate into one-char creep.
                let window = &self.content[start..end];
                let half = window.len() / 2;
                let overlap_start = smallest_char_cut(window, |cut| {
                    cut >= half && embed::count_tokens(&window[cut..]) <= self.params.overlap
                })
                .map_or(end, |cut| start + cut);
                overlap_start.min(end).max(start + 1)
            } else {
                end
            };
        }
    }

    /// Pack atoms into chunks: greedy accumulation of consecutive units up to the chunk size, an
    /// estimate-first sum verified against the real token count of the joined run. A windowed atom
    /// is already a complete chunk. A non-zero overlap extends a chunk's start backward over whole
    /// trailing units of its predecessor, up to the overlap budget; a trailing unit that does not
    /// fit contributes nothing.
    fn pack(&self, atoms: &[Atom]) -> Vec<Chunk> {
        let size = self.params.chunk_size;
        let atom_tokens: Vec<usize> = atoms
            .iter()
            .map(|atom| embed::count_tokens(self.slice(atom.range)))
            .collect();
        let header_tokens = embed::count_tokens(self.header_text);
        let room = size.saturating_sub(header_tokens);

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut i = 0;
        while i < atoms.len() {
            if atoms[i].windowed {
                chunks.push(Chunk {
                    text: chunk_text(self.header_text, self.slice(atoms[i].range)),
                    content_range: atoms[i].range,
                });
                i += 1;
                continue;
            }
            let own_start = atoms[i].range.0;
            // Overlap: whole trailing units of the predecessor, nearest first, while they stay
            // within the overlap budget and the chunk still fits with its first own unit.
            let mut start = own_start;
            if self.params.overlap > 0 && !chunks.is_empty() {
                let mut j = i;
                while j > 0 && !atoms[j - 1].windowed {
                    let candidate = atoms[j - 1].range.0;
                    if embed::count_tokens(&self.content[candidate..own_start]) > self.params.overlap
                        || !fits(self.header_text, &self.content[candidate..atoms[i].range.1], size)
                    {
                        break;
                    }
                    start = candidate;
                    j -= 1;
                }
            }
            // Accumulate own units by token-sum estimate, then verify the joined run.
            let overlap_tokens = embed::count_tokens(&self.content[start..own_start]);
            let mut sum = overlap_tokens + atom_tokens[i];
            let mut k = i + 1;
            while k < atoms.len() && !atoms[k].windowed && sum + atom_tokens[k] <= room {
                sum += atom_tokens[k];
                k += 1;
            }
            while k > i + 1 && !fits(self.header_text, &self.content[start..atoms[k - 1].range.1], size) {
                k -= 1;
            }
            let end = atoms[k - 1].range.1;
            chunks.push(Chunk {
                text: chunk_text(self.header_text, &self.content[start..end]),
                content_range: (start, end),
            });
            i = k;
        }
        chunks
    }
}

/// Interior cut points after blank-line separators, each attached to the preceding unit.
fn paragraph_cuts(content: &str, range: (usize, usize)) -> Vec<usize> {
    let slice = &content[range.0..range.1];
    let mut cuts = Vec::new();
    let bytes = slice.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            let cut = range.0 + i + 2;
            if cut > range.0 && cut < range.1 {
                cuts.push(cut);
            }
        }
        i += 1;
    }
    cuts.dedup();
    cuts
}

/// Interior cut points after sentence terminators (`.`, `!`, `?`) followed by whitespace, the
/// whitespace attached to the preceding sentence.
fn sentence_cuts(content: &str, range: (usize, usize)) -> Vec<usize> {
    let slice = &content[range.0..range.1];
    let mut cuts = Vec::new();
    let mut chars = slice.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?')
            && let Some(&(j, next)) = chars.peek()
            && next.is_whitespace()
        {
            let cut = range.0 + j + next.len_utf8();
            if cut > range.0 && cut < range.1 {
                cuts.push(cut);
            }
        }
    }
    cuts.dedup();
    cuts
}

/// Interior cut points after each newline.
fn line_cuts(content: &str, range: (usize, usize)) -> Vec<usize> {
    let slice = &content[range.0..range.1];
    slice
        .bytes()
        .enumerate()
        .filter(|(_, b)| *b == b'\n')
        .map(|(i, _)| range.0 + i + 1)
        .filter(|cut| *cut > range.0 && *cut < range.1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::syntax::{Language, parse_count, reset_parse_count};

    fn header<'a>() -> PassageHeader<'a> {
        PassageHeader {
            identity: "with backoff function app retry\nfn with_backoff(tries: u32)",
            documentation: None,
        }
    }

    fn params(chunk_size: usize, overlap: usize) -> ChunkParams {
        ChunkParams { chunk_size, overlap }
    }

    fn decl_content(n: usize) -> (String, Vec<usize>) {
        let decls: Vec<String> = (0..n)
            .map(|i| format!("fn item{i}(value: u32) -> u32 {{ value + {i} * distinctive_offset_{i} }}\n"))
            .collect();
        let starts: Vec<usize> = decls
            .iter()
            .scan(0usize, |acc, d| {
                let start = *acc;
                *acc += d.len();
                Some(start)
            })
            .collect();
        (decls.concat(), starts)
    }

    /// Every content byte lies in at least one chunk's range.
    fn assert_covers(chunks: &[Chunk], len: usize) {
        let mut covered = vec![false; len];
        for chunk in chunks {
            for flag in &mut covered[chunk.content_range.0..chunk.content_range.1] {
                *flag = true;
            }
        }
        assert!(covered.iter().all(|f| *f), "chunks must cover the whole content");
    }

    /// Content fitting beside the header is exactly one chunk covering it whole.
    #[test]
    fn content_within_the_size_is_one_chunk() {
        let content = "fn with_backoff(tries: u32) { sleep(tries) }";
        let chunks = split_passage(&header(), content, None, &params(512, 0));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content_range, (0, content.len()));
        assert!(chunks[0].text.starts_with(header().identity));
        assert!(chunks[0].text.ends_with(content));
    }

    /// Content exceeding the size yields several chunks that together cover all of it, and no
    /// chunk exceeds the size with its header included.
    #[test]
    fn oversized_content_is_covered_and_bounded() {
        let content: String = (0..400).map(|i| format!("word{i} ")).collect();
        let size = 64;
        let chunks = split_passage(&header(), &content, None, &params(size, 0));
        assert!(chunks.len() > 1, "content far over the size must split");
        assert_covers(&chunks, content.len());
        for chunk in &chunks {
            assert!(
                embed::count_tokens(&chunk.text) <= size,
                "chunk must be within the size, header included: {} tokens",
                embed::count_tokens(&chunk.text)
            );
            assert!(chunk.content_range.0 < chunk.content_range.1, "a chunk is never empty");
        }
    }

    /// With a syntax tree available, chunk boundaries fall at declaration starts: no chunk begins
    /// or ends part-way through a declaration.
    #[test]
    fn chunks_divide_at_declaration_boundaries() {
        let (content, decl_starts) = decl_content(12);
        let tree = SyntaxTree::parse(&content, Language::Rust).unwrap();
        let span = ByteSpan {
            start: 0,
            end: content.len(),
        };
        let chunks = split_passage(&header(), &content, Some((&tree, span)), &params(64, 0));
        assert!(chunks.len() > 1, "several declarations over the size must split");
        assert_covers(&chunks, content.len());
        for pair in chunks.windows(2) {
            let boundary = pair[1].content_range.0;
            assert!(
                decl_starts.contains(&boundary),
                "chunk boundary {boundary} must sit at a declaration start"
            );
        }
    }

    /// Prose with no syntax boundary divides at sentence boundaries: every chunk after the first
    /// begins right after a sentence terminator.
    #[test]
    fn prose_divides_at_sentence_boundaries() {
        let content: String = (0..80)
            .map(|i| format!("Sentence number {i} says something modestly distinctive. "))
            .collect();
        let chunks = split_passage(&header(), &content, None, &params(64, 0));
        assert!(chunks.len() > 1, "prose over the size must split");
        assert_covers(&chunks, content.len());
        for pair in chunks.windows(2) {
            let boundary = pair[1].content_range.0;
            let before = &content[..boundary];
            assert!(
                before.trim_end().ends_with('.'),
                "chunk boundary at {boundary} must follow a sentence terminator"
            );
        }
    }

    /// Content offering no boundary at all — one enormous unbroken run — is still bounded and
    /// wholly covered by token windows.
    #[test]
    fn boundaryless_content_is_bounded_and_covered() {
        // Spaces but no sentence terminators, newlines, or syntax: a single giant literal.
        let content: String = (0..600)
            .map(|i| format!("datum{i} "))
            .collect::<String>()
            .trim_end()
            .to_string();
        let size = 48;
        let chunks = split_passage(&header(), &content, None, &params(size, 0));
        assert!(chunks.len() > 1);
        assert_covers(&chunks, content.len());
        for chunk in &chunks {
            assert!(embed::count_tokens(&chunk.text) <= size);
        }
    }

    /// With an overlap of none, no content byte appears in more than one chunk.
    #[test]
    fn no_overlap_means_no_shared_content() {
        let content: String = (0..200).map(|i| format!("Sentence {i} is here. ")).collect();
        let chunks = split_passage(&header(), &content, None, &params(64, 0));
        assert!(chunks.len() > 1);
        for pair in chunks.windows(2) {
            assert!(
                pair[0].content_range.1 <= pair[1].content_range.0,
                "ranges must be disjoint at overlap none"
            );
        }
    }

    /// With an overlap set, each chunk after the first begins with whole trailing units of its
    /// predecessor: its start lies inside the predecessor's range, at a unit boundary, within the
    /// overlap budget.
    #[test]
    fn overlap_carries_whole_units() {
        let (content, decl_starts) = decl_content(12);
        let tree = SyntaxTree::parse(&content, Language::Rust).unwrap();
        let span = ByteSpan {
            start: 0,
            end: content.len(),
        };
        let chunks = split_passage(&header(), &content, Some((&tree, span)), &params(96, 24));
        assert!(chunks.len() > 1);
        let mut overlapped = 0;
        for pair in chunks.windows(2) {
            let (prev, next) = (&pair[0], &pair[1]);
            assert!(
                decl_starts.contains(&next.content_range.0),
                "an overlapped start must still sit at a unit boundary"
            );
            if next.content_range.0 < prev.content_range.1 {
                overlapped += 1;
                let carried = &content[next.content_range.0..prev.content_range.1];
                assert!(
                    embed::count_tokens(carried) <= 24,
                    "carried content must stay within the overlap budget"
                );
            }
        }
        assert!(overlapped > 0, "an overlap of 24 must carry some trailing unit");
    }

    /// An oversized header is trimmed to the budget with its identity-bearing head intact, and the
    /// passage's chunks still carry content.
    #[test]
    fn oversized_header_documentation_is_trimmed() {
        let docs: String = (0..300).map(|i| format!("docword{i} ")).collect();
        let header = PassageHeader {
            identity: "gamma ray function physics\nfn gamma_ray(x: u32) -> u32",
            documentation: Some(&docs),
        };
        let content: String = (0..100).map(|i| format!("Sentence {i} is content. ")).collect();
        let size = 64;
        let chunks = split_passage(&header, &content, None, &params(size, 0));
        assert!(chunks.len() > 1);
        assert_covers(&chunks, content.len());
        for chunk in &chunks {
            assert!(chunk.text.starts_with(header.identity), "the head is never trimmed");
            let full_docs_survives = chunk.text.contains("docword299");
            assert!(!full_docs_survives, "the documentation tail is trimmed");
            assert!(embed::count_tokens(&chunk.text) <= size);
            assert!(
                chunk.content_range.0 < chunk.content_range.1,
                "chunks still carry content"
            );
        }
    }

    /// Splitting every passage of a symbol-dense document performs no parse beyond the document's
    /// own: the splitter reads the prepared tree through the child-span accessor.
    #[test]
    fn splitting_performs_no_extra_parse() {
        let (content, _) = decl_content(20);
        reset_parse_count();
        let tree = SyntaxTree::parse(&content, Language::Rust).unwrap();
        assert_eq!(parse_count(), 1);
        let span = ByteSpan {
            start: 0,
            end: content.len(),
        };
        for _ in 0..5 {
            let chunks = split_passage(&header(), &content, Some((&tree, span)), &params(64, 0));
            assert!(chunks.len() > 1);
        }
        assert_eq!(
            parse_count(),
            1,
            "splitting must not parse beyond the document's own parse"
        );
    }

    /// Empty content still yields one chunk — the header alone — so every passage carries at
    /// least one vector.
    #[test]
    fn empty_content_yields_the_header_chunk() {
        let chunks = split_passage(&header(), "", None, &params(512, 0));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, header().identity);
    }

    /// A windowed run carries plain token overlap when overlap is set: consecutive windows share
    /// content, the carried suffix stays within the overlap budget, and every window is still
    /// within the size.
    #[test]
    fn windowed_runs_carry_token_overlap() {
        let content: String = (0..600)
            .map(|i| format!("datum{i} "))
            .collect::<String>()
            .trim_end()
            .to_string();
        let size = 64;
        let overlap = 32;
        let chunks = split_passage(&header(), &content, None, &params(size, overlap));
        assert!(chunks.len() > 2, "the boundary-less content spans several windows");
        assert_covers(&chunks, content.len());
        let mut overlapped = 0;
        for pair in chunks.windows(2) {
            assert!(embed::count_tokens(&pair[1].text) <= size);
            if pair[1].content_range.0 < pair[0].content_range.1 {
                overlapped += 1;
                let carried = &content[pair[1].content_range.0..pair[0].content_range.1];
                assert!(
                    embed::count_tokens(carried) <= overlap,
                    "the carried suffix stays within the overlap budget"
                );
            }
        }
        assert!(
            overlapped > 0,
            "windowed runs must carry token overlap when overlap is set"
        );
    }

    /// At the minimum accepted chunk size, every chunk — multi-byte content included — stays
    /// within the bound: the terminal fallback can never exceed it.
    #[test]
    fn the_minimum_chunk_size_holds_the_bound() {
        let tiny_header = PassageHeader {
            identity: "probe function fn probe()",
            documentation: None,
        };
        let content = "α β γ δ ε ζ η θ world 🌍 emoji test κόσμος";
        let chunks = split_passage(&tiny_header, content, None, &params(MIN_CHUNK_SIZE, 0));
        assert!(!chunks.is_empty());
        assert_covers(&chunks, content.len());
        for chunk in &chunks {
            assert!(
                embed::count_tokens(&chunk.text) <= MIN_CHUNK_SIZE,
                "the bound holds at the minimum size: {} tokens",
                embed::count_tokens(&chunk.text)
            );
        }
    }

    /// The terminal header fallback: an identity head that alone exceeds the header budget is
    /// itself tail-trimmed — the hard size bound outranks header wholeness — with the head's
    /// front (the identifying part) preserved and every chunk still within the size.
    #[test]
    fn an_identity_head_exceeding_the_budget_is_tail_trimmed() {
        let identity: String = format!(
            "gamma ray function physics\nfn gamma_ray({})",
            (0..200)
                .map(|i| format!("param_{i}: u32"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let header = PassageHeader {
            identity: &identity,
            documentation: None,
        };
        let content: String = (0..60).map(|i| format!("Sentence {i} is content. ")).collect();
        let size = 64;
        let chunks = split_passage(&header, &content, None, &params(size, 0));
        assert!(!chunks.is_empty());
        assert_covers(&chunks, content.len());
        for chunk in &chunks {
            assert!(
                embed::count_tokens(&chunk.text) <= size,
                "the size bound holds even against an oversized head"
            );
            assert!(
                chunk.text.starts_with("gamma ray function physics"),
                "the head's identifying front is preserved"
            );
            assert!(
                !chunk.text.contains("param_199"),
                "the head's tail is trimmed, from the parameter run"
            );
            assert!(
                chunk.content_range.0 < chunk.content_range.1,
                "chunks still carry content"
            );
        }
    }

    /// The no-token-limit property at the passage level: content past any single chunk still
    /// affects the passage's representation set. Two long contents sharing their head and
    /// differing only at the very end yield different vector sets — the tail is represented, in a
    /// later chunk, rather than falling past a cap.
    #[test]
    fn content_past_a_chunk_affects_the_representation_set() {
        let head: String = (0..400).map(|i| format!("word{i} ")).collect();
        let a = split_passage(
            &header(),
            &format!("{head}zebra quantum waterfall"),
            None,
            &params(64, 0),
        );
        let b = split_passage(&header(), &format!("{head}igloo cathedral spark"), None, &params(64, 0));
        assert!(a.len() > 1, "the content spans several chunks");
        let vectors = |chunks: &[Chunk]| -> Vec<Vec<u32>> {
            chunks
                .iter()
                .map(|chunk| embed::embed(&chunk.text).iter().map(|v| v.to_bits()).collect())
                .collect()
        };
        let (a_vectors, b_vectors) = (vectors(&a), vectors(&b));
        assert_eq!(a_vectors[0], b_vectors[0], "the shared head embeds identically");
        assert_ne!(
            a_vectors, b_vectors,
            "the differing tail changes the representation set"
        );
    }
}
