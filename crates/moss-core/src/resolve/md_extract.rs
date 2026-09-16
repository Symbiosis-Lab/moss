//! Pure markdown reference extractor — zero I/O, no resolve, no indexes.
//!
//! Scans raw markdown source for every reference token (wikilink / embed /
//! markdown link / markdown image) and returns the raw text plus byte offsets
//! covering the whole token. The offsets let callers rewrite the source without
//! re-scanning.
//!
//! **No resolution** happens here. The caller (src-tauri) resolves each
//! `RawRef` against the project's indexes.
//!
//! Recognition runs over [`crate::inert_regions`]'s mask rather than a
//! private fence tracker, so this scanner and
//! [`crate::ast::shortcode_extract::shortcode_asset_spans`] give one
//! identical answer to "which bytes are live syntax". Two behaviours changed
//! when the private tracker was deleted (2026-08-03): references inside an
//! authored `<!-- … -->` comment and inside an indented code block are no
//! longer extracted. The old doc justified scanning comments with
//! build-internal `<!-- moss-embed:… -->` sentinels, which never appear in
//! the author files this module's only consumer (src-tauri's
//! `editor::ref_scan`) reads from disk.

/// Which surface syntax produced this reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefSyntax {
    /// `[[stem]]` — bare wikilink, stem only (no `/`)
    WikilinkStem,
    /// `[[a/b]]` — wikilink with a path component
    WikilinkPath,
    /// `![[x]]` — embed, bare stem
    WikilinkStemEmbed,
    /// `![[a/b]]` — embed, path
    WikilinkPathEmbed,
    /// `[[stem|Display]]` — wikilink with alias
    WikilinkAliased { display: String },
    /// `![[stem|Display]]` / `![[stem|500]]` — embed with pothole
    WikilinkAliasedEmbed { display: String },
    /// `[label](path)` — standard markdown link
    MarkdownLink { label: String },
    /// `![alt](path)` — standard markdown image
    MarkdownImage { alt: String },
}

/// A raw reference extracted from a markdown source string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRef {
    /// The resolved/target text (the inner `stem`, `a/b`, or `path` part — no
    /// brackets, no alias, no pothole). This is the string to pass to the
    /// classifier.
    pub text: String,
    /// Which syntax form produced this reference.
    pub syntax: RefSyntax,
    /// Byte offset in the source string where the token starts (inclusive).
    pub byte_from: usize,
    /// Byte offset in the source string where the token ends (exclusive).
    pub byte_to: usize,
    /// Byte span of [`text`](Self::text) itself inside the source — the
    /// wikilink target, or the markdown destination with any title stripped.
    /// `source[ref_from..ref_to] == text`.
    ///
    /// A RENAME replaces exactly this span, which is narrower than
    /// `byte_from..byte_to` and never covers a nested reference. That is what
    /// makes `[![[hero.png]]](/album/)` rewritable: the inner embed's span and
    /// the outer link's destination span are disjoint, so both can be edited
    /// in one pass. Rebuilding the whole token from `syntax` instead would
    /// re-emit the label verbatim and silently drop the inner rewrite.
    pub ref_from: usize,
    /// Exclusive end of [`ref_from`](Self::ref_from).
    pub ref_to: usize,
}

/// Extract all markdown references from `source`.
///
/// Recognition runs over [`crate::inert_regions::mask_inert`], the one
/// shared answer to "which bytes are not live syntax" — so references
/// inside fenced code blocks, indented code blocks, inline code spans and
/// HTML comments are skipped. Every *string* (`text`, `label`, `alt`, the
/// wikilink alias) is sliced from the ORIGINAL source, because the mask
/// blanks inline code spans and would otherwise corrupt a label like
/// ``[a `b` c](x.md)``.
///
/// External URLs (`http://…`, `https://…`, `//`, `mailto:`, `tel:`, `data:`)
/// are included as `MarkdownLink` / `MarkdownImage` — the caller decides
/// whether to filter them out.
///
/// **Nested references are reported too.** The label of a markdown link is
/// re-scanned, so `[![[hero.png]]](/album/)` — ADR-041's link-wrapped embed —
/// yields the outer `MarkdownLink` AND the inner `WikilinkStemEmbed`, and
/// `[![a](hero.png)](/album/)` yields the outer link and the inner image.
/// Before that, the inner reference existed only as a substring of the outer
/// ref's `label`, so a rename left it dangling with no report. Results stay in
/// source order, with an enclosing reference immediately preceding the ones
/// nested inside it.
///
/// An image's `alt` is deliberately NOT re-scanned: `![alt [![[x]]](/u)](y.png)`
/// is one image whose alt text happens to contain brackets, and rewriting
/// inside it would edit prose.
pub fn extract_md_references(source: &str) -> Vec<RawRef> {
    let mask = crate::inert_regions::mask_inert(source);
    let mut refs = Vec::new();
    scan_range(source, mask.as_bytes(), 0, source.len(), &mut refs);
    refs
}

/// Scan `source[from..to]` for reference tokens, appending to `refs`.
///
/// `bytes` is the whole-source inert mask (byte-length preserving, so an
/// offset in it is an offset in `source`); `to` bounds every lookahead, which
/// is what keeps a nested scan of a link label from claiming a `]]` or a `)`
/// that lives past the label's end.
fn scan_range(source: &str, bytes: &[u8], from: usize, to: usize, refs: &mut Vec<RawRef>) {
    let len = to;
    let mut i = from;

    while i < len {
        // ── Backslash escape: `\[[note]]` / `\[t](p)` are NOT references ──
        // An escape is not an inert region (the mask leaves it alone), so it
        // stays a case here. Skip the backslash and the next char so the
        // escaped bracket can't start a reference token. Advance by a full
        // char (not a byte) so `i` stays on a UTF-8 boundary for later slices.
        if bytes[i] == b'\\' {
            i += 1; // past the backslash (ASCII, boundary-safe)
            if i < len {
                // SAFETY: `i` is a char boundary here; read one full char.
                #[allow(clippy::string_slice)]
                if let Some(ch) = source[i..].chars().next() {
                    i += ch.len_utf8();
                }
            }
            continue;
        }

        // ── Wikilink / embed: ![[…]] or [[…]] ───────────────────────────
        let is_embed_wikilink = i + 4 < len
            && bytes[i] == b'!'
            && bytes[i+1] == b'['
            && bytes[i+2] == b'[';
        let is_wikilink = !is_embed_wikilink
            && i + 3 < len
            && bytes[i] == b'['
            && bytes[i+1] == b'[';

        if is_embed_wikilink || is_wikilink {
            let token_start = i;
            let inner_start = if is_embed_wikilink { i + 3 } else { i + 2 };
            // Find closing ]] — in the MASK, so a `]]` hidden in inline code
            // does not close a live wikilink.
            if let Some(close) = find_double_bracket(bytes, inner_start, len) {
                // SAFETY: inner_start and close are valid UTF-8 char boundaries
                // because we only advance past ASCII bytes ([, !, ]) to reach them.
                #[allow(clippy::string_slice)]
                let inner = &source[inner_start..close];
                let token_end = close + 2;
                // Split on | for alias/pothole
                let (path_part, pipe_part) = match inner.split_once('|') {
                    Some((before, after)) => (before, Some(after)),
                    None => (inner, None),
                };
                // Only record non-empty targets
                if !path_part.trim().is_empty() {
                    let text = path_part.trim().to_string();
                    let has_slash = text.contains('/');
                    let syntax = match (is_embed_wikilink, pipe_part) {
                        (false, None) => {
                            if has_slash { RefSyntax::WikilinkPath } else { RefSyntax::WikilinkStem }
                        }
                        (false, Some(alias)) => RefSyntax::WikilinkAliased { display: alias.to_string() },
                        (true, None) => {
                            if has_slash { RefSyntax::WikilinkPathEmbed } else { RefSyntax::WikilinkStemEmbed }
                        }
                        (true, Some(pot)) => RefSyntax::WikilinkAliasedEmbed { display: pot.to_string() },
                    };
                    // `text` is `path_part` trimmed; its span starts past the
                    // leading whitespace `trim` removed.
                    let ref_from = inner_start + (path_part.len() - path_part.trim_start().len());
                    let ref_to = ref_from + text.len();
                    refs.push(RawRef {
                        text,
                        syntax,
                        byte_from: token_start,
                        byte_to: token_end,
                        ref_from,
                        ref_to,
                    });
                }
                i = token_end;
                continue;
            }
        }

        // ── Markdown image ![alt](path) ──────────────────────────────────
        if i + 3 < len && bytes[i] == b'!' && bytes[i+1] == b'[' {
            if let Some(link) = parse_md_link(source, bytes, i + 1, len) {
                let token_start = i;
                refs.push(RawRef {
                    text: link.path,
                    syntax: RefSyntax::MarkdownImage { alt: link.label },
                    byte_from: token_start,
                    byte_to: link.token_end,
                    ref_from: link.path_from,
                    ref_to: link.path_to,
                });
                i = link.token_end;
                continue;
            }
        }

        // ── Markdown link [label](path) ──────────────────────────────────
        if bytes[i] == b'[' {
            // Guard: not a wikilink (already handled above)
            if i + 1 < len && bytes[i+1] != b'[' {
                if let Some(link) = parse_md_link(source, bytes, i, len) {
                    let (label_from, label_to) = (link.label_from, link.label_to);
                    let end = link.token_end;
                    refs.push(RawRef {
                        text: link.path,
                        syntax: RefSyntax::MarkdownLink { label: link.label },
                        byte_from: i,
                        byte_to: end,
                        ref_from: link.path_from,
                        ref_to: link.path_to,
                    });
                    // The label can itself hold references — ADR-041's
                    // `[![[hero.png]]](/album/)`, or the CommonMark spelling
                    // `[![a](hero.png)](/album/)`. Scan it, bounded by the
                    // label's own end.
                    scan_range(source, bytes, label_from, label_to, refs);
                    i = end;
                    continue;
                }
            }
        }

        i += 1;
    }
}

/// Find the byte index of the first `]]` in `bytes[start..limit]`.
/// Returns the index of the first `]` in the `]]` pair, or `None`.
fn find_double_bracket(bytes: &[u8], start: usize, limit: usize) -> Option<usize> {
    let bytes = &bytes[..limit];
    let mut j = start;
    while j + 1 < bytes.len() {
        if bytes[j] == b']' && bytes[j+1] == b']' {
            return Some(j);
        }
        // Bail on newline — wikilinks are single-line
        if bytes[j] == b'\n' {
            return None;
        }
        j += 1;
    }
    None
}

/// One parsed `[label](path)` / `![alt](path)` token, with the byte spans a
/// rewriter needs: `label_from..label_to` (re-scanned for nested references)
/// and `path_from..path_to` (the only bytes a rename touches).
struct ParsedLink {
    label: String,
    label_from: usize,
    label_to: usize,
    path: String,
    path_from: usize,
    path_to: usize,
    token_end: usize,
}

/// Parse a `[label](path)` or `![alt](path)` link starting at `bracket_pos`
/// (the position of the opening `[`), scanning no further than `limit`.
fn parse_md_link(
    source: &str,
    bytes: &[u8],
    bracket_pos: usize,
    limit: usize,
) -> Option<ParsedLink> {
    let len = limit;
    // Find closing ] — but respect nested brackets and bail on newline
    let mut depth = 0usize;
    let mut j = bracket_pos;
    while j < len {
        match bytes[j] {
            b'[' => { depth += 1; j += 1; }
            b']' => {
                depth -= 1;
                if depth == 0 { break; }
                j += 1;
            }
            b'\n' => return None,
            _ => { j += 1; }
        }
    }
    if j >= len || bytes[j] != b']' { return None; }
    let label_start = bracket_pos + 1;
    let label_end = j;
    #[allow(clippy::string_slice)]
    let label = source[label_start..label_end].to_string();

    // Expect `(` immediately after `]`
    let paren_open = j + 1;
    if paren_open >= len || bytes[paren_open] != b'(' { return None; }

    // Find closing `)` — respect nesting, bail on newline
    let mut depth = 0usize;
    let mut k = paren_open;
    while k < len {
        match bytes[k] {
            b'(' => { depth += 1; k += 1; }
            b')' => {
                depth -= 1;
                if depth == 0 { break; }
                k += 1;
            }
            b'\n' => return None,
            _ => { k += 1; }
        }
    }
    if k >= len || bytes[k] != b')' { return None; }
    let path_start = paren_open + 1;
    let path_end = k;
    #[allow(clippy::string_slice)]
    let raw = &source[path_start..path_end];
    // Strip optional title: `path "title"` → path. Both `trim` and
    // `strip_link_title` only ever cut from the ends, so the surviving text is
    // a prefix of the trimmed slice and its span is arithmetic, not a search
    // (a `find` would land on the wrong copy of a repeated path).
    let trimmed = raw.trim();
    let path_from = path_start + (raw.len() - raw.trim_start().len());
    let path = strip_link_title(trimmed);
    let path_to = path_from + path.len();

    Some(ParsedLink {
        label,
        label_from: label_start,
        label_to: label_end,
        path,
        path_from,
        path_to,
        token_end: k + 1,
    })
}

/// Strip an optional CommonMark link title from a raw link destination string.
/// `path "My Title"` → `path`, `path 'title'` → `path`, `path (title)` → `path`.
/// If no title is present, returns the input unchanged.
fn strip_link_title(raw: &str) -> String {
    let raw = raw.trim();
    // Find the last whitespace-separated token that looks like a title
    if let Some(ws) = raw.rfind(|c: char| c.is_ascii_whitespace()) {
        let (path_part, maybe_title) = raw.split_at(ws);
        let maybe_title = maybe_title.trim();
        let is_title = (maybe_title.starts_with('"') && maybe_title.ends_with('"'))
            || (maybe_title.starts_with('\'') && maybe_title.ends_with('\''))
            || (maybe_title.starts_with('(') && maybe_title.ends_with(')'));
        if is_title {
            return path_part.trim().to_string();
        }
    }
    raw.to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "md_extract_tests.rs"]
mod tests;

// ── Structural asset paths ────────────────────────────────────────────────
//
// `extract_md_references` above sees only BRACKETED markdown tokens. A
// `:::gallery` body line, a `:::hero {image=…}` attribute and a frontmatter
// `cover:` value are asset references with no reference syntax around them,
// so they were invisible to rename tracking and silently broke on rename.
// The types below are the second half of the answer; `ref_scan` (src-tauri)
// unions the two.

/// Which container a structurally-extracted path was found in.
/// Decides quoting when the value is rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathContainer {
    /// A body line of a `:::gallery` block.
    GalleryBody,
    /// A body media line of a `:::hero` block.
    HeroBodyMedia,
    /// The positional path on a `:::hero <path>` directive line.
    HeroDirective,
    /// A `key=value` attribute of a `:::` block (today only `image=`).
    ShortcodeAttr { key: String },
    /// A frontmatter field whose value names a project file.
    FrontmatterField { key: String },
}

/// An asset path that occupies a span with NO markdown reference syntax
/// around it.
///
/// # Contract
///
/// A rename replaces `value` with `render_bare_value(container, quote, path,
/// attrs)`; a delete removes `outer`. `attrs` is the `|attrs` suffix that
/// lives INSIDE `value` — it is empty when the author's attrs sit outside it
/// (`![alt](x.png)|cover`, where the attrs follow the closing paren), so
/// re-rendering never duplicates or drops them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPathSpan {
    /// Decoded and trimmed: no quotes, no `|attrs`, no `[[ ]]`.
    pub path: String,
    /// The `|attrs` suffix inside `value`, or `""` — rebuilt verbatim.
    pub attrs: String,
    /// Quote character the value was wrapped in, if any.
    pub quote: Option<char>,
    /// Span a RENAME rewrites — the whole value, quotes included.
    pub value: std::ops::Range<usize>,
    /// Span a DELETE removes — the whole gallery/frontmatter line
    /// (including its terminator) or the whole `key=value` attr item.
    pub outer: std::ops::Range<usize>,
    /// Where the path was found.
    pub container: PathContainer,
}

/// One recognized media reference on a single line, with LINE-RELATIVE
/// offsets. Produced by the gallery and hero line recognizers; lifted to
/// absolute offsets by the block-level scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLineSpan {
    /// The path text as the parser would read it.
    pub path: String,
    /// Alt text (`![alt](…)`), or `""`.
    pub alt: String,
    /// The full `|attrs` suffix the parser reads (may sit outside `value`).
    pub attrs: String,
    /// Line-relative span a rename replaces.
    pub value: std::ops::Range<usize>,
    /// The `|attrs` suffix contained WITHIN `value`.
    pub value_attrs: String,
    /// The line carried markdown reference syntax (`![[…]]` / `![…](…)`),
    /// so [`extract_md_references`] already sees it as a token.
    pub is_token: bool,
}

/// `(base, content_len, terminator_len)` per physical line of `source`,
/// index-aligned with [`str::lines`].
pub(crate) fn line_table(source: &str) -> Vec<(usize, usize, usize)> {
    let bytes = source.as_bytes();
    let mut table = Vec::new();
    let mut base = 0usize;
    while base <= bytes.len() {
        let nl = bytes[base..].iter().position(|&b| b == b'\n');
        match nl {
            Some(off) => {
                let mut content = off;
                if content > 0 && bytes[base + content - 1] == b'\r' {
                    content -= 1;
                }
                table.push((base, content, off - content + 1));
                base += off + 1;
            }
            None => {
                if base < bytes.len() {
                    table.push((base, bytes.len() - base, 0));
                }
                break;
            }
        }
    }
    table
}

/// Every structural asset path in `source`, ascending by `value.start`.
///
/// Complements [`extract_md_references`], which sees only bracketed markdown
/// tokens. A caller that REWRITES must union the two and resolve overlaps —
/// see `apply_edits` in src-tauri's `editor::ref_scan`.
pub fn extract_structural_asset_refs(source: &str) -> Vec<AssetPathSpan> {
    let mut v = crate::ast::shortcode_extract::shortcode_asset_spans(source);
    v.extend(crate::frontmatter::frontmatter_asset_spans(source));
    v.sort_by_key(|s| s.value.start);
    v
}
