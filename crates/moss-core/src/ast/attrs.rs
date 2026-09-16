//! Attribute-block parser for the unified shortcode grammar.
//!
//! Parses the `{ ... }` portion of an opening fence into structured
//! classes, an optional id, and key/value pairs. Pure, no I/O.
//!
//! Grammar (from `docs/archive/2026-05-02-shortcode-grammar-design.md`):
//!
//! ```text
//! Attrs    := "{" AttrItem (whitespace AttrItem)* "}"
//! AttrItem := "." classname
//!           | "#" id
//!           | key "=" value
//! key      := [A-Za-z][A-Za-z0-9_-]*
//! value    := bareword | quoted
//! bareword := (Unicode-alphanumeric | [:_/.\-])+
//! quoted   := "\"" any-char-except-unescaped-quote* "\""
//! ```
//!
//! Whitespace inside `{}` (spaces, tabs, newlines) all separate items
//! identically — multi-line attribute blocks are first-class.
//!
//! The parser is forgiving on malformed bareword values (returns the
//! malformed token verbatim rather than erroring); it errors only on
//! structural problems (unterminated quote, bad key, no closing brace).
//! Renderers are responsible for validating typed values like `cols=int`.

/// Parsed attribute block.
///
/// `kvs` is a `Vec<(String, String)>` (not a `HashMap`) so iteration is
/// stable and matches source order — important for deterministic HTML
/// attribute output, snapshot tests, and diagnostics that point at the
/// offending entry. Last-write-wins on duplicate keys is enforced in the
/// parser, so callers can use [`AttrBlock::get`] without seeing stale
/// values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttrBlock {
    /// `.classname` shortcuts, in source order. Duplicates preserved so
    /// the renderer can detect explicit doubles if it wants.
    pub classes: Vec<String>,
    /// Last `#id` shortcut wins (multiple ids is malformed but not fatal).
    pub id: Option<String>,
    /// `key=value` pairs in source order. Last write wins when a key
    /// repeats — the parser drops earlier entries on collision.
    pub kvs: Vec<(String, String)>,
    /// Width flag (spec § P9): `body | wide | page | screen`. Recognized
    /// as a bare token in the attribute block; `{full}` is normalized to
    /// `"screen"`. Last write wins on repeats. `None` means the author did
    /// not specify a width — emitters should omit `data-width` in that
    /// case so the HTML stays sparse and themes can target the absence.
    pub width: Option<&'static str>,
}

impl AttrBlock {
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
            && self.id.is_none()
            && self.kvs.is_empty()
            && self.width.is_none()
    }

    /// Convenience for renderers: get the value for a key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.kvs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Space-joined class list, ready for `class="..."`.
    pub fn class_string(&self) -> String {
        self.classes.join(" ")
    }

    /// Set a key/value pair, replacing any existing entry for `key`
    /// in place to preserve source order of unrelated entries.
    fn set_kv(&mut self, key: String, value: String) {
        if let Some(slot) = self.kvs.iter_mut().find(|(k, _)| k == &key) {
            slot.1 = value;
        } else {
            self.kvs.push((key, value));
        }
    }
}

/// Errors parsing an attribute block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrError {
    /// Input did not start with `{`.
    MissingOpenBrace,
    /// Block opened with `{` but no matching `}` was found.
    UnclosedBrace,
    /// A `"` opened but had no closing `"` before the block ended.
    UnterminatedQuote,
    /// A `key=` was followed by no value (end of input or whitespace).
    EmptyValue { key: String },
    /// A token started with `=` (no key before it) or had a malformed key.
    InvalidKey { token: String },
}

/// Byte spans of one `key=value` item inside an attribute block.
///
/// Produced as a by-product of [`parse_attrs_spanned`] so a caller that
/// wants to REWRITE one attribute value (rename tracking for
/// `:::hero {image=…}`) can do a surgical byte replacement instead of
/// re-serializing the block and losing the author's spacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvSpan {
    /// The item's key.
    pub key: String,
    /// RAW value span, quotes INCLUDED, relative to `input`.
    pub value: std::ops::Range<usize>,
    /// The whole `key=value` item, relative to `input`.
    pub item: std::ops::Range<usize>,
    /// Only ever `None` or `Some('"')` — this grammar has no single-quote
    /// form (`'` is not in [`is_bareword`], so `image='a b.jpg'` is an
    /// `EmptyValue` error, not a quoted value).
    pub quote: Option<char>,
}

/// Parse an attribute block.
///
/// `input` must include the surrounding braces: `{.foo key=bar}`.
/// Whitespace between items (including newlines) is permitted.
///
/// Errors only on structural problems. Unrecognized characters at the start
/// of an item — anything not `.`, `#`, or a valid key character — produce
/// `InvalidKey { token }` so the caller can surface a useful diagnostic.
pub fn parse_attrs(input: &str) -> Result<AttrBlock, AttrError> {
    parse_attrs_spanned(input).map(|(block, _)| block)
}

/// [`parse_attrs`] plus one [`KvSpan`] per `key=value` item.
///
/// Same loop, same grammar, same errors — the spans are pushed by the
/// existing `char_indices()` walk rather than by a second tokenizer, so
/// both of this parser's quirks are preserved by construction: an empty
/// `.`/`#` bareword is silently skipped, and the FIRST malformed item
/// aborts the whole block (discarding everything accumulated so far).
///
/// # Parsing past the closing brace
///
/// `parse_attrs_spanned` returns `Ok` at the first `}` **at item position**,
/// and a `}` inside a quoted value is consumed by `read_quoted`. That is what
/// lets [`crate::ast::shortcode_extract::shortcode_asset_spans`] hand it
/// `&source[brace_start..]` — the rest of the document — and get spans that
/// terminate exactly where the gathered-args form would, without duplicating
/// [`gather_multi_line_attrs`]. **If this grammar ever gains a nested `{`,
/// that equivalence breaks silently.**
pub fn parse_attrs_spanned(input: &str) -> Result<(AttrBlock, Vec<KvSpan>), AttrError> {
    let mut kv_spans: Vec<KvSpan> = Vec::new();
    let mut chars = input.char_indices().peekable();

    // Expect leading `{`.
    skip_ws(&mut chars);
    match chars.next() {
        Some((_, '{')) => {}
        _ => return Err(AttrError::MissingOpenBrace),
    }

    let mut block = AttrBlock::default();

    loop {
        skip_ws(&mut chars);
        match chars.peek().copied() {
            None => return Err(AttrError::UnclosedBrace),
            Some((_, '}')) => {
                chars.next();
                return Ok((block, kv_spans));
            }
            Some((_, '.')) => {
                chars.next();
                let class = read_bareword(&mut chars);
                if !class.is_empty() {
                    block.classes.push(class);
                }
            }
            Some((_, '#')) => {
                chars.next();
                let id = read_bareword(&mut chars);
                if !id.is_empty() {
                    block.id = Some(id);
                }
            }
            Some((item_start, c)) if is_key_start(c) => {
                let key = read_key(&mut chars);
                skip_ws_inline(&mut chars);
                match chars.peek().copied() {
                    Some((_, '=')) => {
                        chars.next();
                        skip_ws_inline(&mut chars);
                        let value_start = chars.peek().map_or(input.len(), |&(i, _)| i);
                        let quote = match chars.peek() {
                            Some(&(_, '"')) => Some('"'),
                            _ => None,
                        };
                        let value = read_value(&mut chars, &key)?;
                        // `read_value` stops on the first char it did not
                        // consume, so the peeked index IS the exclusive end.
                        let value_end = chars.peek().map_or(input.len(), |&(i, _)| i);
                        kv_spans.push(KvSpan {
                            key: key.clone(),
                            value: value_start..value_end,
                            item: item_start..value_end,
                            quote,
                        });
                        block.set_kv(key, value);
                    }
                    _ => {
                        // Bare keyword (no `=value`). Spec § P9 reserves four
                        // width tokens (`body | wide | page | screen`) plus
                        // the alias `full` (→ `screen`) as bare flags for
                        // hero / gallery / grid / embed / image-wrapper
                        // sizing. Any other bare keyword is still an error.
                        if let Some(width) = match_width_token(&key) {
                            block.width = Some(width);
                        } else {
                            return Err(AttrError::InvalidKey { token: key });
                        }
                    }
                }
            }
            Some((_, c)) => {
                // Anything else at item-start is invalid. Capture the run
                // up to the next whitespace/`}` for the diagnostic.
                let mut token = String::new();
                token.push(c);
                chars.next();
                while let Some(&(_, ch)) = chars.peek() {
                    if ch.is_whitespace() || ch == '}' {
                        break;
                    }
                    token.push(ch);
                    chars.next();
                }
                return Err(AttrError::InvalidKey { token });
            }
        }
    }
}

fn is_key_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Recognize the spec § P9 width tokens (`body | wide | page | screen | full`).
///
/// `full` is the author-facing alias for `screen` (the spec keeps both shapes
/// because `{full}` reads naturally in authoring contexts while the emitted
/// attribute value is the value-space term `screen`). All five tokens are
/// ASCII lowercase per the grammar; the parser feeds keys verbatim, so any
/// case-folding decision lives here.
fn match_width_token(s: &str) -> Option<&'static str> {
    match s {
        "body" => Some("body"),
        "wide" => Some("wide"),
        "page" => Some("page"),
        "screen" | "full" => Some("screen"),
        _ => None,
    }
}

fn is_key_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Whether `c` can appear in an unquoted attribute value.
///
/// Unicode-alphanumeric (not just ASCII) so a non-ASCII filename — `頭像.png`,
/// `café.jpg` — can be written unquoted in `:::hero {image=…}` the same way
/// it can appear bare anywhere else a path is written (gallery body,
/// frontmatter, wikilinks). Before this widened, `read_value` treated the
/// first non-ASCII byte as "not bareword", `parse_attrs` returned
/// `EmptyValue`, and every caller's `.unwrap_or_default()` silently dropped
/// the ENTIRE attribute block — not just the offending value, so `image=`
/// disappeared along with `width`/`classes`/`mobile` and the hero rendered
/// with no image at all. `required_quote` in moss-build's `editor/ref_rewrite.rs`
/// must accept exactly this same set — it calls this function rather than
/// keeping its own copy, so the two can't drift apart again.
pub fn is_bareword(c: char) -> bool {
    matches!(c, ':' | '/' | '.' | '-' | '_') || c.is_alphanumeric()
}

fn skip_ws<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = (usize, char)>,
{
    while let Some(&(_, c)) = iter.peek() {
        if c.is_whitespace() {
            iter.next();
        } else {
            break;
        }
    }
}

/// Like `skip_ws` but stops at the first newline — used after a `key`
/// before checking for `=`. Newline-after-key without `=` is a malformed
/// item, but the user might have written `key\n=value` which we should
/// accept (whitespace is whitespace inside `{}`). Today we treat all
/// whitespace identically — so this is currently equivalent to skip_ws.
/// Kept as a separate helper in case future grammars distinguish.
fn skip_ws_inline<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = (usize, char)>,
{
    skip_ws(iter);
}

fn read_bareword<I>(iter: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = (usize, char)>,
{
    let mut s = String::new();
    while let Some(&(_, c)) = iter.peek() {
        if is_bareword(c) {
            s.push(c);
            iter.next();
        } else {
            break;
        }
    }
    s
}

fn read_key<I>(iter: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = (usize, char)>,
{
    let mut s = String::new();
    if let Some(&(_, c)) = iter.peek() {
        if is_key_start(c) {
            s.push(c);
            iter.next();
        } else {
            return s;
        }
    }
    while let Some(&(_, c)) = iter.peek() {
        if is_key_continue(c) {
            s.push(c);
            iter.next();
        } else {
            break;
        }
    }
    s
}

fn read_value<I>(iter: &mut std::iter::Peekable<I>, key: &str) -> Result<String, AttrError>
where
    I: Iterator<Item = (usize, char)>,
{
    match iter.peek().copied() {
        Some((_, '"')) => {
            iter.next();
            read_quoted(iter)
        }
        Some((_, c)) if is_bareword(c) => Ok(read_bareword(iter)),
        _ => Err(AttrError::EmptyValue { key: key.to_string() }),
    }
}

/// Read until the closing `"`, supporting `\"` and `\\` escapes.
fn read_quoted<I>(iter: &mut std::iter::Peekable<I>) -> Result<String, AttrError>
where
    I: Iterator<Item = (usize, char)>,
{
    let mut s = String::new();
    loop {
        match iter.next() {
            None => return Err(AttrError::UnterminatedQuote),
            Some((_, '"')) => return Ok(s),
            Some((_, '\\')) => match iter.next() {
                None => return Err(AttrError::UnterminatedQuote),
                Some((_, ch)) => s.push(ch),
            },
            Some((_, ch)) => s.push(ch),
        }
    }
}

// ── Multi-line opener support ────────────────────────────────────────
//
// The shortcode extractor needs to know when an opener line's `{`
// doesn't close on the same line, so it can absorb subsequent lines
// into the attribute block before parsing. These helpers live here
// (next to `parse_attrs`) because they're grammar primitives, not
// extraction helpers — any future consumer that sees a partial attr
// block (e.g. an editor decoration that wants to highlight the live
// state of a fenced div as the user types) needs them too.

/// Quote-aware brace-depth tracker. Returns the depth after consuming
/// `s`, starting from `start_depth`.
///
/// Tracks `"`-quoted strings so that `{` and `}` inside a value like
/// `key="{name}"` don't shift the depth. Backslash escapes inside a
/// string consume the next character verbatim.
pub fn brace_depth(s: &str, start_depth: i32) -> i32 {
    let mut depth = start_depth;
    let mut in_quote = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if in_quote {
            match c {
                '"' => in_quote = false,
                '\\' => {
                    chars.next();
                }
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_quote = true,
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    depth
}

/// If `single_line_args` opens a `{` that doesn't close on the same line,
/// scan `following_lines` and join them onto args until the brace closes.
///
/// Returns `(extended_args_owned, lines_consumed)` where:
/// - `extended_args_owned = Some(s)` when multi-line scanning happened.
///   `None` means the single-line args already balance and the caller
///   should keep using the original `&str`.
/// - `lines_consumed` is how many lines after the opener were absorbed
///   into the attribute block. The body starts at
///   `following_lines[lines_consumed..]`.
///
/// Brace balancing tracks ASCII `{` and `}` outside quoted strings. A
/// `\"`-escape inside a quoted string is recognized so authored content
/// like `key="say \"hi\""` doesn't desynchronize the scanner.
///
/// If the brace never closes (ill-formed input), returns the gathered
/// content verbatim so the caller can pass it on; the attribute parser
/// will emit a structural error and the block falls through to the
/// pass-through path.
pub fn gather_multi_line_attrs(
    single_line_args: &str,
    following_lines: &[&str],
) -> (Option<String>, usize) {
    let depth_after_first = brace_depth(single_line_args, 0);
    if depth_after_first == 0 {
        return (None, 0);
    }

    let mut combined = single_line_args.to_string();
    let mut depth = depth_after_first;
    let mut consumed = 0;
    for &line in following_lines {
        // Insert a newline so the attribute parser sees the line break
        // as whitespace (its grammar treats all whitespace identically).
        combined.push('\n');
        combined.push_str(line);
        consumed += 1;
        depth = brace_depth(line, depth);
        if depth == 0 {
            return (Some(combined), consumed);
        }
    }

    // Brace never closed within the document.
    (Some(combined), consumed)
}

#[cfg(test)]
#[path = "attrs_tests.rs"]
mod tests;
