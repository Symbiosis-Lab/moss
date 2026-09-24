// Shared types AND pure helpers for the CM6 completion source
// (`cm-completion.ts`) and the wikilink typing helpers
// (`cm-wikilink-complete.ts`).
//
// Everything here is pure: no CM6 import, no DOM, no RPC. That is what makes
// it safe for both to depend on, and what lets its test suite run in the
// `node` vitest environment instead of jsdom.
//
// `ExtKind` is declared locally rather than imported from `../bindings` —
// depcruise's `interim-bindings-tauri-importers` rule restricts bindings.ts
// imports to the platform seam; neither completion source calls the RPC
// directly; the caller (`editor-main.ts`, already seam-baselined) does, via
// the `complete` callback below. Mirrors `crates/moss-core/src/resolve/ext_kind.rs`'s
// `ExtKind` enum (bindings.ts's generated union has the same members).
export type ExtKind =
  | 'Image'
  | 'Iframe'
  | 'Pdf'
  | 'Video'
  | 'Audio'
  | 'Model'
  | 'Transclusion'
  | 'Notebook'
  | 'Table'
  | 'Other';

/**
 * The link syntax the caret is inside. Mirrors Rust
 * `link_completions::LinkSyntax` (serde kebab-case); declared locally for the
 * same reason `ExtKind` is. The backend reads it, with the typed prefix, to
 * decide the FORM every row inserts — the author links to a thing, moss
 * writes the address.
 */
export type LinkSyntax = 'wikilink' | 'embed' | 'inline' | 'asset-path';

/** One completion returned by the backend (mirrors Rust `WikilinkCompletion`). */
export interface WikilinkCompletionItem {
  /** The exact text accepting the row writes into the link. */
  insert: string;
  /** The thing's name: page title, filename, `dir/`, term text, heading. */
  label: string;
  /**
   * Dimmer second column: the insert text when it differs from the label (so
   * the author learns the address on the spot), `H2` for a heading whose text
   * is its own insert, else null.
   */
  detail: string | null;
  kind: TargetKind;
}

/** A target's kind, as the dropdown paints it. Mirrors moss-core's `TargetKind`. */
export type TargetKind = 'page' | 'asset' | 'folder' | 'generated' | 'heading';

export interface WikilinkCompleteOptions {
  /**
   * Compute completions for the current cursor context. Backed by the Rust
   * `wikilink_completions` command (Result already unwrapped to an array; an
   * error/empty backend yields `[]`).
   *
   * @param prefix   The already-typed text to rank against.
   * @param fromFile The active file; biases ranking toward same-language-tree
   *                 and closer-in-tree candidates (backend relativizes it).
   * @param syntax   The link syntax the caret is in — decides the insert form
   *                 and which kinds rank first.
   * @param page     Heading target page (project-relative `.md`) for `[[Page#…`
   *                 / `[[#…` / `[…](page#…`; null for link-target completion.
   * @param allowedKinds Restrict link-target results to assets of these
   *                 `ExtKind`s (only meaningful when `page` is null). Null for
   *                 the unrestricted link syntaxes; the shortcode asset phases
   *                 pass their `assetKinds`/`bodyAssetKinds`.
   */
  complete: (
    prefix: string,
    fromFile: string,
    syntax: LinkSyntax,
    page: string | null,
    allowedKinds?: ExtKind[] | null,
  ) => Promise<WikilinkCompletionItem[]>;
  /** Current file's page target (for `[[#` same-page lookup). null if none. */
  getCurrentPath: () => string | null;
  /**
   * Resolve ONE completion candidate to asset metadata for the preview card
   * CM6 renders beside the highlighted row (`Completion.info`).
   *
   * `target` is the row's `insert` — so the card is resolved by exactly the
   * text and from exactly the document that acceptance will produce. The
   * panel therefore previews the INSERTION, not "a file": two candidates that
   * insert the same text are the same completion, and showing them the same
   * preview is the truth.
   *
   * Optional: absent ⇒ completions carry no info panel. Resolving to null means
   * "no metadata" — the card keeps the candidate's name and format badge.
   */
  resolveAssetPreview?: (target: string) => Promise<AssetPreviewData | null>;
}

/**
 * Structural subset of Rust `ResolvedAsset` (bindings.ts) that the preview card
 * needs. Mirrored here for the same reason `WikilinkCompletionItem` is — this
 * module is the shared pure DTO layer, and depcruise's bindings allowlist is
 * shrink-only, so neither this module nor `asset-preview.ts` may import
 * `../bindings`. `ResolvedAsset` is structurally assignable to this.
 * `size_bytes` is a u64 serialized to string by specta; accepted as either.
 */
export interface AssetPreviewData {
  absolute_path: string;
  request_url: string;
  mime_type: string;
  width: number | null;
  height: number | null;
  size_bytes: string | number;
}

// ── Pure line parsers (no CM6 dependency, unit-tested) ───────────────
// `parseCompletionContext` in `cm-completion.ts` composes these; it
// stays there because it consults SHORTCODE_CATALOG, and importing that here
// would close a cycle.

/**
 * Where the caret is and what it is asking for. Every `from`/`to` is an index
 * INTO THE LINE. The link phases (`wikilink`, `heading`, `link`, `image`,
 * `body`, `attr-value`) all carry the full target span, so one accept path
 * replaces the whole token rather than the prefix before the cursor.
 */
export type CompletionPhase =
  | { phase: 'name'; query: string; from: number }
  | { phase: 'attr-name'; blockName: string; query: string; from: number }
  | {
      phase: 'attr-value';
      blockName: string;
      attrName: string;
      query: string;
      from: number;
      /**
       * In-line index of the value start INCLUDING any opening quote, while
       * `from` stays the query start (after the quote) so `CompletionResult.from`
       * is unchanged. The accept path needs the quote to replace the whole
       * value token — see `attrValueSpan`.
       */
      valueFrom: number;
    }
  | { phase: 'body'; blockName: string; query: string; from: number; to: number }
  | { phase: 'image'; query: string; from: number; to: number }
  /** An open `[[…` / `![[…`: page, asset or folder by source path. */
  | { phase: 'wikilink'; embed: boolean; query: string; from: number; to: number }
  /**
   * After a `#` inside a link target. `page` is the target's page part, or
   * null for the page being edited. The syntax decides what a heading inserts:
   * its text for `[[…#`, its anchor slug for `[…](…#`.
   */
  | { phase: 'heading'; syntax: 'wikilink' | 'inline'; page: string | null; query: string; from: number; to: number }
  /** An inline `[text](…)` target: source path, or the site with a leading `/`. */
  | { phase: 'link'; query: string; from: number; to: number };

// Matches an open, unclosed [[...]] (optionally embed-prefixed `![[`) ending at
// the cursor: group 1 is the optional `!`, group 2 is the inner text after the
// brackets up to the cursor (no closing ]] yet, no newline).
const OPEN_WIKILINK = /(!?)\[\[([^\]\n]*)$/;

/**
 * The wikilink phase at `cursorInLine`, or null when the cursor is not inside
 * an open `[[…` / `![[…` (plain text, or after a closed `[[x]]`). The span
 * runs from just after the brackets (or the `#`) over the typed query and the
 * rest of the target the cursor sits inside (`wikilinkTailLength`).
 */
export function parseWikilinkPhase(lineText: string, cursorInLine: number): CompletionPhase | null {
  const m = OPEN_WIKILINK.exec(lineText.slice(0, cursorInLine));
  if (!m) return null;
  const embed = m[1] === '!';
  const inner = m[2];
  const to = cursorInLine + wikilinkTailLength(lineText.slice(cursorInLine));
  const hash = inner.indexOf('#');
  if (hash === -1) {
    return { phase: 'wikilink', embed, query: inner, from: cursorInLine - inner.length, to };
  }
  const query = inner.slice(hash + 1);
  return {
    phase: 'heading',
    syntax: 'wikilink',
    page: inner.slice(0, hash) || null,
    query,
    from: cursorInLine - query.length,
    to,
  };
}

/**
 * The phase for an inline `![alt](…)` or `[text](…)` target containing the
 * cursor. An image target is the `image` phase (assets only). A link target
 * is the `link` phase, or the `heading` phase once the author types `#` — a
 * page part that opens with `/` names a place on the published site, whose
 * headings need a source file this parser cannot name, so it offers nothing.
 */
export function parseInlineTargetPhase(lineText: string, cursorInLine: number): CompletionPhase | null {
  const t = parseInlineTarget(lineText, cursorInLine);
  if (!t) return null;
  if (t.image) return { phase: 'image', query: t.query, from: t.from, to: t.to };
  const hash = t.query.indexOf('#');
  if (hash === -1) return { phase: 'link', query: t.query, from: t.from, to: t.to };
  const page = t.query.slice(0, hash);
  if (page.startsWith('/')) return null;
  const query = t.query.slice(hash + 1);
  return { phase: 'heading', syntax: 'inline', page: page || null, query, from: t.from + hash + 1, to: t.to };
}

/** Minimal doc-line accessor — same shape `blockAlreadyClosed` already uses. */
export interface DocLines {
  lines: number;
  line(n: number): { text: string };
}

/**
 * Parse the attr region of a fence line (cursor at or past the `{`).
 *
 * Scans left-to-right from just after `{` to the cursor, tracking whether
 * we're collecting an attr NAME or an attr VALUE. A bare (unescaped) `=`
 * switches name→value; unquoted whitespace ends a value and starts a new
 * name; a quote character as the FIRST char of a value opens a quoted span
 * that swallows internal whitespace verbatim (so `button="Sign up now"`
 * doesn't truncate the query at the space). Whichever mode is active when
 * the loop reaches the cursor determines the phase: 'attr-name' (still
 * typing the attr's name — matches against the static attrs list) or
 * 'attr-value' (typing the value of the most recently opened, still-open
 * `name=`) — this is the fix for `{image=photo.jpg}` being misread as an
 * attr-*name* query by the old single regex.
 *
 * Returns null when the cursor sits just past a CLOSED quoted value
 * (`{image="a.png"|}`): the value is finished, so there is nothing to complete,
 * and the old code produced the corrupt query `a.png"` there.
 */
export function parseAttrPhase(
  lineText: string,
  blockName: string,
  braceAbsIdx: number,
  cursorInLine: number,
): CompletionPhase | null {
  let mode: 'name' | 'value' = 'name';
  let nameStart = braceAbsIdx + 1;
  let curName = '';
  let inQuote = false;
  let quoteChar = '';
  let valueStart = -1;
  // Set when a quoted value's closing quote is consumed; reset whenever a new
  // value opens. Distinguishes "still typing inside the quotes" from "cursor
  // parked after the closing quote".
  let valueClosed = false;

  for (let i = braceAbsIdx + 1; i < cursorInLine; i++) {
    const ch = lineText[i];
    if (mode === 'name') {
      if (ch === '=') {
        curName = lineText.slice(nameStart, i);
        mode = 'value';
        valueStart = i + 1;
        inQuote = false;
        valueClosed = false;
      } else if (/[\s,]/.test(ch)) {
        nameStart = i + 1;
        curName = '';
      }
    } else {
      if (inQuote) {
        if (ch === quoteChar) {
          inQuote = false;
          valueClosed = true;
        }
      } else if ((ch === '"' || ch === "'") && i === valueStart) {
        inQuote = true;
        quoteChar = ch;
      } else if (/[\s,]/.test(ch)) {
        // Unquoted value ended by whitespace OR comma — start a fresh name
        // token. Comma matters for multi-attr shortcodes like
        // `{count=5,since=...}` (no space after the comma): without it as
        // a boundary here too, a comma-separated attr right after a value
        // gets swallowed into that value's query instead of starting its
        // own attr-name completion.
        mode = 'name';
        nameStart = i + 1;
        curName = '';
        valueStart = -1;
        valueClosed = false;
      }
    }
  }

  if (mode === 'value') {
    // Cursor is past a completed `"…"` — the value is done, offer nothing.
    if (valueClosed) return null;
    let queryFrom = valueStart;
    if (queryFrom < lineText.length && (lineText[queryFrom] === '"' || lineText[queryFrom] === "'")) {
      queryFrom += 1; // don't include the opening quote in the query text
    }
    const query = lineText.slice(queryFrom, cursorInLine);
    return {
      phase: 'attr-value',
      blockName,
      attrName: curName,
      query,
      from: queryFrom,
      valueFrom: valueStart,
    };
  }

  const query = lineText.slice(nameStart, cursorInLine);
  return { phase: 'attr-name', blockName, query, from: nameStart };
}

/** Parse a fence line (`:::name {attrs}`). Returns null if not a fence line. */
export function parseFenceLine(lineText: string, cursorInLine: number): CompletionPhase | null {
  const trimmed = lineText.trimStart();
  const leadingWs = lineText.length - trimmed.length;
  if (!trimmed.startsWith(':::')) return null;
  const afterFence = trimmed.slice(3);
  const fenceEnd = leadingWs + 3;
  if (cursorInLine < fenceEnd) return null;

  const braceIdx = afterFence.indexOf('{');
  const braceAbsIdx = braceIdx === -1 ? -1 : fenceEnd + braceIdx;

  if (braceAbsIdx !== -1 && cursorInLine > braceAbsIdx) {
    const nameMatch = afterFence.match(/^([\w-]+)/);
    const blockName = nameMatch ? nameMatch[1] : '';
    return parseAttrPhase(lineText, blockName, braceAbsIdx, cursorInLine);
  }

  // Between ::: and { — name completion
  const nameQuery = afterFence.slice(0, cursorInLine - fenceEnd).replace(/[^a-zA-Z0-9_-]/g, '');
  return { phase: 'name', query: nameQuery, from: fenceEnd };
}

/** Index of the first unescaped `|` in `text`, or -1. */
export function findUnescapedPipe(text: string): number {
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '|' && text[i - 1] !== '\\') return i;
  }
  return -1;
}

/**
 * Chars in `lineText` starting at `parenOpen + 1` that belong to a markdown
 * link or image TARGET: bounded by the closing `)` (or end of line while still
 * typing) and, before that, by the first unescaped `|` (moss's display-attrs
 * suffix).
 */
function inlineTargetBound(lineText: string, parenOpen: number): number {
  let close = lineText.indexOf(')', parenOpen + 1);
  if (close === -1) close = lineText.length; // still typing — no closer yet
  const inner = lineText.slice(parenOpen + 1, close);
  const pipe = findUnescapedPipe(inner);
  return pipe === -1 ? close : parenOpen + 1 + pipe;
}

/**
 * Find the markdown target (`![alt](‹here›)` or `[text](‹here›)`) containing
 * `cursorInLine`, anywhere in the line — inside a `:::grid` cell, inside a
 * link (`[![alt](p)](/url)`), or in ordinary prose. First match containing the
 * cursor wins; `image` says which form it was.
 *
 * Deliberately a hand-rolled scanner rather than a reuse of the Lezer-based
 * `cm-image-extract.ts::extractImageTargets`: the markdown Lezer emits no
 * `Image`/`Link` node until the closing `)` exists, and moss ships no
 * `closeBrackets()` (`cm-editor.ts` uses `minimalSetup`), so during live
 * typing of `![](關於/頭` there is no node to resolve. A completion source must
 * answer over INCOMPLETE syntax, which a committed tree cannot do.
 *
 * Falsifier for that exception: if the lint's target spans and these ever
 * diverge on a real document, unify on Lezer with an explicit incomplete-token
 * fallback. Both are pure and separately tested, so the divergence is
 * observable.
 */
export function parseInlineTarget(
  lineText: string,
  cursorInLine: number,
): { query: string; from: number; to: number; image: boolean } | null {
  // Link text may not itself open with `![`: in `[![alt](p)](/url)` the image
  // is the target the cursor can be in, and letting the outer `[` swallow it
  // would read the image path as the link's. (The outer link's own target is
  // then unreachable — the scan resumes after the image, and no `[` remains.)
  const re = /(!?)\[(?!!\[)[^\]\n]*\]\(/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(lineText)) !== null) {
    const parenOpen = m.index + m[0].length - 1;
    const bound = inlineTargetBound(lineText, parenOpen);
    if (cursorInLine > parenOpen && cursorInLine <= bound) {
      const from = parenOpen + 1;
      return { query: lineText.slice(from, cursorInLine), from, to: bound, image: m[1] === '!' };
    }
  }
  return null;
}

/**
 * Parse a gallery body line for asset-path completion. Body lines are a bare
 * `path`, or `![alt](path)`, either optionally suffixed with `|attrs`
 * (`shortcode_extract.rs` `parse_gallery_body`). Gallery does not support
 * `![[...]]` wikilink-embed syntax in its body, so that form is not special-
 * cased here.
 *
 * A cursor past an unescaped `|` (editing the display-attrs suffix, not the
 * path) yields no completion. The `![alt](path)` form is delegated to
 * `parseInlineTarget` — one implementation for every image target in the
 * document; only the bare-path arm is gallery-specific.
 */
export function parseGalleryBodyLine(
  lineText: string,
  cursorInLine: number,
): { query: string; from: number; to: number } | null {
  const img = parseInlineTarget(lineText, cursorInLine);
  if (img?.image) return { query: img.query, from: img.from, to: img.to };
  if (/^\s*!\[/.test(lineText)) return null; // an image line, cursor outside its target

  const pipeIdx = findUnescapedPipe(lineText);
  if (pipeIdx !== -1 && cursorInLine > pipeIdx) return null;
  const boundary = pipeIdx === -1 ? lineText.length : pipeIdx;

  // Bare path line.
  const leadingWs = lineText.length - lineText.trimStart().length;
  if (cursorInLine < leadingWs || cursorInLine > boundary) return null;
  return { query: lineText.slice(leadingWs, cursorInLine), from: leadingWs, to: boundary };
}

/**
 * The full value token of a shortcode attr starting at `valueFrom` (which
 * INCLUDES an opening quote if there is one), split into the end of the token
 * and the `|attrs` suffix the accept path must preserve.
 *
 * - Quoted, closing quote on this line: the span includes both quotes. A `}`
 *   inside the quotes is part of the value and does not end it.
 * - Quoted, NO closing quote on this line: the value is unterminated, so its
 *   real extent is unknowable from this line. The span stops at the block's
 *   closing `}` if there is one, and otherwise at `fallbackTo` (the caller
 *   passes the cursor). **Never** to end of line — that swallowed the `}` and
 *   everything after it, turning `:::hero {image="關}` into an unparseable
 *   `:::hero {image="關於/頭像.png"`. Reachable straight from the shipped hero
 *   snippet: its `photo.jpg` tab-stop is selected, so typing `"` then a CJK
 *   prefix produces exactly that line.
 * - Bareword: ends at the first `[\s,}]`, else at `fallbackTo`.
 *
 * `pipeSuffix` is the display-attrs tail, UNESCAPED — the caller re-renders
 * `path + pipeSuffix` through `renderShortcodeAttrValue`, so handing back the
 * raw source slice would escape it a second time (`\"Hi\"` → `\\\"Hi\\\"`).
 */
export function attrValueSpan(
  lineText: string,
  valueFrom: number,
  fallbackTo: number,
): { to: number; pipeSuffix: string } {
  const q = lineText[valueFrom];
  const quoted = q === '"' || q === "'";
  const innerFrom = quoted ? valueFrom + 1 : valueFrom;
  let to: number;
  let terminated = false;

  if (quoted) {
    let j = innerFrom;
    while (j < lineText.length) {
      if (lineText[j] === '\\') { j += 2; continue; }
      if (lineText[j] === q) break;
      j += 1;
    }
    terminated = j < lineText.length;
    if (terminated) {
      to = j + 1;
    } else {
      const brace = lineText.indexOf('}', innerFrom);
      to = brace === -1 ? Math.max(fallbackTo, innerFrom) : brace;
    }
  } else {
    const stop = lineText.slice(valueFrom).search(/[\s,}]/);
    to = stop === -1 ? Math.max(fallbackTo, valueFrom) : valueFrom + stop;
  }

  const innerTo = quoted && terminated ? Math.max(innerFrom, to - 1) : to;
  const inner = lineText.slice(innerFrom, innerTo);
  const pipe = findUnescapedPipe(inner);
  if (pipe === -1) return { to, pipeSuffix: '' };
  const suffix = inner.slice(pipe);
  return { to, pipeSuffix: quoted ? suffix.replace(/\\(.)/g, '$1') : suffix };
}

/**
 * Render `v` as a shortcode attr value: bareword when the grammar accepts it
 * verbatim, else a double-quoted string with `\` and `"` escaped.
 *
 * THE one renderer — `cm-completion.ts`'s accept path and
 * `cm-insert-bar.ts`'s hero template both call it. The bareword character set
 * mirrors `is_bareword` in `crates/moss-core/src/shortcode/attrs.rs`; escaping
 * mirrors `read_quoted` there. Drift is gated by
 * `crates/moss-core/tests/fixtures/attr-value.vectors.json`, which both the
 * Rust parser test and the TS renderer test read — not by a comment.
 */
export function renderShortcodeAttrValue(v: string): string {
  if (v.length > 0 && /^[A-Za-z0-9:/._-]+$/.test(v)) return v;
  return `"${v.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/**
 * Chars after the cursor that still belong to the wikilink TARGET: up to the
 * first `]`, `|` or `#`. Lets a mid-token accept replace the whole target
 * instead of splicing the completion in front of its tail.
 *
 * Returns 0 when no terminator is found before end of line. An unclosed `[[`
 * has no target end to find, and treating end-of-line as one deleted arbitrary
 * prose: `see [[Res and then the rest of the sentence` swallowed 33 characters
 * on accept. Extending only over a target we can actually see the end of is the
 * whole safety margin here.
 */
export function wikilinkTailLength(after: string): number {
  const stop = after.search(/[\]|#\n]/);
  return stop === -1 ? 0 : stop;
}

/**
 * Scan backward from `lineNumber - 1` for the nearest unclosed `:::name`
 * fence open. A bare close fence (`:::`) encountered first means the last
 * block already closed before this line, so it returns null. Assumes
 * non-nested blocks (moss shortcodes don't nest), matching
 * `blockAlreadyClosed`'s scan direction/assumptions.
 */
export function findEnclosingBlockName(doc: DocLines, lineNumber: number): string | null {
  for (let n = lineNumber - 1; n >= 1; n--) {
    const text = doc.line(n).text;
    if (/^\s*:{3,}\s*$/.test(text)) return null; // bare close fence
    const openMatch = /^\s*:{3,}\s*([\w-]+)/.exec(text);
    if (openMatch) return openMatch[1];
  }
  return null;
}
