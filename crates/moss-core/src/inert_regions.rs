//! The one answer to "which parts of this markdown are NOT live syntax?"
//!
//! moss runs several scanners over raw markdown *before* pulldown-cmark ever
//! sees it — `:::shortcode` extraction ([`crate::ast::shortcode_extract`]),
//! transclusion/folder-embed lowering ([`crate::resolve`]), CriticMarkup
//! accept and `%%comment%%` stripping (src-tauri's `html_post`). Each of them
//! has to know which byte ranges are *inert*: regions where author text that
//! merely looks like syntax must be left completely alone.
//!
//! Every one of those scanners used to carry its own private answer, and each
//! private answer was a different subset of the truth. The one that only knew
//! about fenced code blocks shipped moss#903 bug 2: a `:::gallery` written
//! inside an authored `<!-- TODO … -->` block was extracted as a live
//! shortcode, which spliced a sentinel into the middle of the comment,
//! destroyed the comment's own `-->`, and made every paragraph *after* the
//! comment vanish from the built page. Consolidating on one scanner is what
//! stops that class of bug from being re-derivable.
//!
//! # What counts as inert
//!
//! | Region | Rule |
//! |---|---|
//! | fenced code block | 3+ `` ` ``/`~` run through its closing run (or EOF) |
//! | indented code block | 4-space-indented run that starts a new block |
//! | inline code span | matched backtick runs, on one line |
//! | HTML comment | `<!--` through `-->`, possibly spanning lines (or EOF) |
//!
//! Deliberately NOT inert: raw HTML tags and HTML blocks other than comments
//! (`<div>` wrappers are a documented moss authoring idiom and shortcodes
//! nest inside them), and math spans (`$…$` is handled by the parser, and
//! `:::` inside math is not a shape anyone writes).
//!
//! # UTF-8 safety
//!
//! The scan walks bytes, matching only ASCII (`` ` ``, `<`, `-`, `>`, space,
//! tab, newline). No byte of a multi-byte UTF-8 sequence is ever ASCII, so
//! every offset this module produces lands on a char boundary — the property
//! that hand-rolled byte loops in this codebase have repeatedly failed to
//! preserve (see the `html_prefix_is_balanced` panic on CJK prose in #903
//! bug 1).
//!
//! # Which shape do I want?
//!
//! - Scanning a whole document for a token, and you want the inert
//!   occurrences to simply not be found: [`mask_inert`]. Byte-length- and
//!   line-preserving, so a match offset in the mask is the same offset in the
//!   original.
//! - Walking line by line, and you want to skip inert lines:
//!   [`inert_lines`].
//! - Anything else: [`InertRegions::scan`] plus [`InertRegions::is_inert`] /
//!   [`InertRegions::intersects`].
//!
//! Not yet consolidated onto this module (each still carries a fence-only
//! scan): [`crate::resolve::block_refs`], [`crate::ast::editor_scan`], and
//! src-tauri's `build::scan::scan`. They should move here as they are next
//! touched. [`crate::resolve::md_extract`] moved here 2026-08-03.

use std::ops::Range;

/// Sorted, non-overlapping byte ranges of `markdown` that are inert.
///
/// Ranges that cover a whole line include that line's terminator, so a blank
/// line inside a fence or comment is reported as inert too.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InertRegions {
    ranges: Vec<Range<usize>>,
    unterminated_comment: Option<usize>,
}

/// Convenience for the common "find tokens outside inert regions" shape:
/// returns a copy of `markdown` with every inert byte replaced by a space.
///
/// See [`InertRegions::mask`] for the guarantees.
pub fn mask_inert(markdown: &str) -> String {
    InertRegions::scan(markdown).mask(markdown)
}

/// Convenience for line-based scanners: one flag per line of `markdown`,
/// aligned index-for-index with [`str::lines`].
///
/// See [`InertRegions::inert_lines`] for what makes a line inert.
pub fn inert_lines(markdown: &str) -> Vec<bool> {
    InertRegions::scan(markdown).inert_lines(markdown)
}

impl InertRegions {
    /// Scan raw markdown source for inert byte ranges.
    ///
    /// Total function: any input, including unterminated fences and
    /// unterminated comments (both of which extend to end-of-input, matching
    /// CommonMark's treatment of an unclosed fenced code block and an
    /// unclosed HTML block).
    pub fn scan(markdown: &str) -> Self {
        let bytes = markdown.as_bytes();
        let mut ranges: Vec<Range<usize>> = Vec::new();

        // Open fence: (fence char, opening run length). CommonMark requires
        // the closing run to be the same char and at least as long.
        let mut fence: Option<(u8, usize)> = None;
        let mut in_comment = false;
        // Where the currently-open `<!--` started. Survives to the end of the
        // scan only when the comment is never closed, which is what
        // `unterminated_comment` reports.
        let mut comment_start: Option<usize> = None;
        let mut in_indented_code = false;
        // Start-of-document reads like "after a blank line": an indented
        // first line is an indented code block.
        let mut prev_blank = true;
        // Sticky list tracking — see `indented code` note in `scan_line`.
        let mut in_list = false;

        let mut line_start = 0usize;
        while line_start < bytes.len() {
            let (content_end, next_start) = line_bounds(bytes, line_start);
            let indent = indent_width(bytes, line_start, content_end);
            let text_start = first_non_ws(bytes, line_start, content_end);
            let is_blank = text_start == content_end;

            // 1. Inside a multi-line HTML comment: inert until `-->`.
            if in_comment {
                match find_bytes(bytes, line_start, content_end, b"-->") {
                    Some(at) => {
                        in_comment = false;
                        comment_start = None;
                        push_range(&mut ranges, line_start..at + 3);
                        // The rest of the line is live again — it can open
                        // another comment or a code span.
                        scan_inline(
                            bytes,
                            at + 3,
                            content_end,
                            next_start,
                            &mut ranges,
                            &mut in_comment,
                            &mut comment_start,
                        );
                    }
                    None => push_range(&mut ranges, line_start..next_start),
                }
                prev_blank = is_blank;
                line_start = next_start;
                continue;
            }

            // 2. Inside a fenced code block: inert until the closing run.
            if let Some((fence_char, open_len)) = fence {
                push_range(&mut ranges, line_start..next_start);
                if closes_fence(bytes, text_start, content_end, fence_char, open_len) {
                    fence = None;
                }
                prev_blank = is_blank;
                line_start = next_start;
                continue;
            }

            // 3. Inside an indented code block: any 4+-indented line
            //    continues it, and so do blank lines (a blank line only ends
            //    it if the next non-blank line dedents).
            if in_indented_code && (is_blank || indent >= 4) {
                push_range(&mut ranges, line_start..next_start);
                prev_blank = is_blank;
                line_start = next_start;
                continue;
            }
            in_indented_code = false;

            if is_blank {
                prev_blank = true;
                line_start = next_start;
                continue;
            }

            // 4. Start of an indented code block. Two guards keep this from
            //    swallowing live content: CommonMark forbids indented code
            //    from interrupting a paragraph (hence `prev_blank`), and
            //    indentation inside a list is list-item content, not code
            //    (hence `in_list`). `in_list` is deliberately sticky —
            //    over-reporting a region as live is a rendering nicety,
            //    under-reporting it silently deletes an author's shortcode.
            if indent >= 4 && prev_blank && !in_list {
                in_indented_code = true;
                push_range(&mut ranges, line_start..next_start);
                prev_blank = false;
                line_start = next_start;
                continue;
            }

            // 5. Opening fence line.
            if let Some((fence_char, run)) = opens_fence(bytes, text_start, content_end) {
                fence = Some((fence_char, run));
                push_range(&mut ranges, line_start..next_start);
                prev_blank = false;
                line_start = next_start;
                continue;
            }

            // 6. Ordinary line: track list context, then look for inline
            //    code spans and HTML comments.
            if is_list_marker(bytes, text_start, content_end) {
                in_list = true;
            } else if indent == 0 && prev_blank {
                in_list = false;
            }
            scan_inline(
                bytes,
                text_start,
                content_end,
                next_start,
                &mut ranges,
                &mut in_comment,
                &mut comment_start,
            );
            prev_blank = false;
            line_start = next_start;
        }

        Self {
            ranges,
            unterminated_comment: if in_comment { comment_start } else { None },
        }
    }

    /// Byte offset of an `<!--` that the document never closes, if there is
    /// one.
    ///
    /// Per CommonMark an unterminated HTML comment runs to end-of-input, so
    /// everything after it is inert — the whole tail of the page stops being
    /// markdown. That is silent content loss, which is the failure this
    /// module exists to stop, so callers with a diagnostics channel report
    /// it (see `ast::shortcode_extract`).
    pub fn unterminated_comment(&self) -> Option<usize> {
        self.unterminated_comment
    }

    /// The inert ranges, sorted by start and non-overlapping.
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    /// True if no region is inert.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// True if the byte at `offset` is inside an inert region.
    pub fn is_inert(&self, offset: usize) -> bool {
        // Ranges are sorted and disjoint: the only candidate is the last
        // range starting at or before `offset`.
        let idx = self.ranges.partition_point(|r| r.start <= offset);
        match idx.checked_sub(1).and_then(|i| self.ranges.get(i)) {
            Some(r) => offset < r.end,
            None => false,
        }
    }

    /// True if any part of `range` is inert. An empty range is tested as a
    /// single point.
    pub fn intersects(&self, range: Range<usize>) -> bool {
        if range.start >= range.end {
            return self.is_inert(range.start);
        }
        let idx = self.ranges.partition_point(|r| r.start < range.end);
        self.ranges
            .get(..idx)
            .unwrap_or_default()
            .iter()
            .rev()
            .take_while(|r| r.end > range.start)
            .any(|r| r.start < range.end)
    }

    /// Return a copy of `markdown` with every inert byte replaced by an ASCII
    /// space, leaving `\n` and `\r` in place.
    ///
    /// The result has the same byte length and the same line structure as the
    /// input, so a byte offset found in the mask indexes the original — the
    /// pattern src-tauri's CriticMarkup pass relies on (match on the mask,
    /// read the capture out of the original).
    pub fn mask(&self, markdown: &str) -> String {
        let mut out = markdown.as_bytes().to_vec();
        for range in &self.ranges {
            let end = range.end.min(out.len());
            if let Some(slice) = out.get_mut(range.start..end) {
                for b in slice {
                    if *b != b'\n' && *b != b'\r' {
                        *b = b' ';
                    }
                }
            }
        }
        // Inert range boundaries are ASCII-aligned (see the module docs), so
        // masking cannot split a multi-byte char. The fallback keeps the
        // function total rather than trusting that argument at runtime.
        String::from_utf8(out).unwrap_or_else(|_| markdown.to_string())
    }

    /// One flag per line of `markdown`, aligned index-for-index with
    /// [`str::lines`].
    ///
    /// A line is inert when its first non-whitespace byte is inert (or, for a
    /// blank line, when its start is). That is the question a line-based
    /// scanner actually asks — "is the token that begins this line live?" — so
    /// a line whose *tail* enters a comment (`text <!-- note`) is NOT inert:
    /// the syntax at its head is still live.
    pub fn inert_lines(&self, markdown: &str) -> Vec<bool> {
        let bytes = markdown.as_bytes();
        let mut flags = Vec::new();
        let mut line_start = 0usize;
        while line_start < bytes.len() {
            let (content_end, next_start) = line_bounds(bytes, line_start);
            let text_start = first_non_ws(bytes, line_start, content_end);
            // Blank line: probe its start (whole-line ranges cover the
            // terminator, so a blank line inside a fence still reads inert).
            let probe = if text_start == content_end {
                line_start
            } else {
                text_start
            };
            flags.push(self.is_inert(probe));
            line_start = next_start;
        }
        flags
    }
}

/// `(end of line content excluding the terminator, start of the next line)`.
fn line_bounds(bytes: &[u8], line_start: usize) -> (usize, usize) {
    let mut i = line_start;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    let next_start = if i < bytes.len() { i + 1 } else { bytes.len() };
    let mut content_end = i;
    if content_end > line_start && bytes.get(content_end - 1) == Some(&b'\r') {
        content_end -= 1;
    }
    (content_end, next_start)
}

/// Offset of the first non-space/tab byte in `[from, to)`, or `to`.
fn first_non_ws(bytes: &[u8], from: usize, to: usize) -> usize {
    let mut i = from;
    while i < to && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

/// CommonMark indent width of the line: tabs advance to the next 4-column
/// stop.
fn indent_width(bytes: &[u8], from: usize, to: usize) -> usize {
    let mut width = 0usize;
    let mut i = from;
    while i < to {
        match bytes[i] {
            b' ' => width += 1,
            b'\t' => width += 4 - (width % 4),
            _ => break,
        }
        i += 1;
    }
    width
}

/// A run of 3+ identical `` ` ``/`~` starting at `text_start`, returned as
/// `(char, run length)`. A backtick fence's info string may not contain a
/// backtick (CommonMark), which is what keeps `` `a` `b` `` from being read
/// as a fence opener.
fn opens_fence(bytes: &[u8], text_start: usize, content_end: usize) -> Option<(u8, usize)> {
    let ch = *bytes.get(text_start)?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut run = 0usize;
    while bytes.get(text_start + run) == Some(&ch) {
        run += 1;
    }
    if run < 3 {
        return None;
    }
    if ch == b'`' {
        let info = bytes.get(text_start + run..content_end).unwrap_or_default();
        if info.contains(&b'`') {
            return None;
        }
    }
    Some((ch, run))
}

/// True if the line is a closing fence for `(fence_char, open_len)`: a run of
/// at least `open_len` of the same char, then nothing but whitespace.
fn closes_fence(
    bytes: &[u8],
    text_start: usize,
    content_end: usize,
    fence_char: u8,
    open_len: usize,
) -> bool {
    let mut run = 0usize;
    while text_start + run < content_end && bytes.get(text_start + run) == Some(&fence_char) {
        run += 1;
    }
    if run < open_len {
        return false;
    }
    bytes
        .get(text_start + run..content_end)
        .unwrap_or_default()
        .iter()
        .all(|b| *b == b' ' || *b == b'\t')
}

/// True if the line starts a list item (`-`/`*`/`+` or `N.`/`N)` followed by
/// whitespace or end-of-line).
fn is_list_marker(bytes: &[u8], text_start: usize, content_end: usize) -> bool {
    let after_marker = match bytes.get(text_start) {
        Some(b'-') | Some(b'*') | Some(b'+') => text_start + 1,
        Some(d) if d.is_ascii_digit() => {
            let mut i = text_start;
            while i < content_end && bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            match bytes.get(i) {
                Some(b'.') | Some(b')') => i + 1,
                _ => return false,
            }
        }
        _ => return false,
    };
    match bytes.get(after_marker) {
        None => true,
        Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => true,
        Some(_) => after_marker >= content_end,
    }
}

/// Scan `[from, content_end)` of one line for inline code spans and HTML
/// comments, pushing the inert ranges it finds.
///
/// Sets `in_comment` (and records `comment_start`) when the line ends inside
/// an unterminated `<!--`; the range pushed in that case runs to `next_start`
/// so the line's terminator is covered.
#[allow(clippy::too_many_arguments)]
fn scan_inline(
    bytes: &[u8],
    from: usize,
    content_end: usize,
    next_start: usize,
    ranges: &mut Vec<Range<usize>>,
    in_comment: &mut bool,
    comment_start: &mut Option<usize>,
) {
    let mut i = from;
    while i < content_end {
        match bytes[i] {
            b'`' => {
                let mut run = 0usize;
                while i + run < content_end && bytes[i + run] == b'`' {
                    run += 1;
                }
                match closing_backtick_run(bytes, i + run, content_end, run) {
                    Some(close) => {
                        push_range(ranges, i..close + run);
                        i = close + run;
                    }
                    // No matching run on this line: not inline code per
                    // CommonMark (a span may not contain a blank line, and
                    // moss's line-at-a-time scanners never look further).
                    None => i += run,
                }
            }
            b'<' if bytes.get(i..i + 4) == Some(b"<!--".as_slice()) => {
                // Search from `i + 2` so the degenerate empty comments
                // `<!-->` and `<!--->` terminate on their own `-->`
                // instead of swallowing the rest of the document.
                match find_bytes(bytes, i + 2, content_end, b"-->") {
                    Some(at) => {
                        push_range(ranges, i..at + 3);
                        i = at + 3;
                    }
                    None => {
                        push_range(ranges, i..next_start);
                        *in_comment = true;
                        *comment_start = Some(i);
                        return;
                    }
                }
            }
            _ => i += 1,
        }
    }
}

/// Offset of the start of a backtick run of exactly `run` length in
/// `[from, to)`, per CommonMark's "same number of backticks" rule.
fn closing_backtick_run(bytes: &[u8], from: usize, to: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i < to {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let mut len = 0usize;
        while i + len < to && bytes[i + len] == b'`' {
            len += 1;
        }
        if len == run {
            return Some(i);
        }
        i += len;
    }
    None
}

/// First offset of `needle` within `[from, to)` of `bytes`.
fn find_bytes(bytes: &[u8], from: usize, to: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || to < from {
        return None;
    }
    let hay = bytes.get(from..to)?;
    hay.windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

/// Append `range`, coalescing with the previous range when they touch or
/// overlap, so [`InertRegions::ranges`] stays sorted and disjoint.
fn push_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.start >= range.end {
        return;
    }
    if let Some(last) = ranges.last_mut() {
        if range.start <= last.end {
            last.end = last.end.max(range.end);
            return;
        }
    }
    ranges.push(range);
}

#[cfg(test)]
#[path = "inert_regions_tests.rs"]
mod tests;
