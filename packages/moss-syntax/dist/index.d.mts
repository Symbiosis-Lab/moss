import { MarkdownConfig } from "@lezer/markdown";

//#region src/shortcode.d.ts

declare const SHORTCODE_OPEN_RE: RegExp;
/** True when `m` (a `SHORTCODE_OPEN_RE` match) is a real opener: either it
 *  has a name, or its remainder is an attrs block (starts with `{`). Shared
 *  by `parse` and `endLeaf` so the two can't drift on what counts as open. */
declare function isOpenMatch(m: RegExpExecArray | null): boolean;
/** A pure-colon line of arity >=3 is a close fence. */
declare function isCloseFence(lineText: string): boolean;
declare const shortcodeBlockConfig: MarkdownConfig;
/** One `key=value` item read off an attribute block. */
interface AttrKvSpan {
  key: string;
  /** Decoded value: quotes stripped, `\"` and `\\` escapes applied. */
  value: string;
  /**
   * Raw value span within the parsed string, quotes EXCLUDED — so a hover
   * lands on the path an author would click, not on the punctuation around
   * it. (Rust's `KvSpan.value` includes the quotes; it is used for rewriting,
   * where they matter, and this is used for pointing, where they don't.)
   */
  from: number;
  to: number;
  /** Span of the key name within the parsed string (for token highlighting). */
  keyFrom: number;
  keyTo: number;
}
/**
 * Read every `key=value` item out of an attribute block.
 *
 * `input` must start (after whitespace) with `{`. Returns `null` for any
 * structural error, which is the caller-visible spelling of Rust's
 * `Err(AttrError)` + `.unwrap_or_default()`: no attributes, not partial ones.
 *
 * Classes (`.foo`) and the id (`#bar`) are consumed and discarded — this
 * reader exists to find asset paths, and neither can hold one.
 */
declare function parseAttrKvSpans(input: string): AttrKvSpan[] | null;
/** An asset path named on a shortcode's opening line. */
interface ShortcodeAssetRef {
  /** The path, with any `|display attrs` suffix already split off. */
  target: string;
  /** Span of the path within the `args` string passed in. */
  from: number;
  to: number;
}
/**
 * The asset a shortcode's opening line names, or null.
 *
 * `args` is everything after `:::name` on that line — exactly the
 * `ShortcodeAttrs` node's text. A MIRROR of moss-core `extract_hero::parse_hero`'s
 * image-source priority order:
 *
 *   1. the `image=` attribute inside `{…}` (canonical grammar);
 *   2. a bare path written before the `{…}` (`:::hero ./cover.jpg`) — the
 *      legacy directive-line form still parsed by the build.
 *
 * Its priority 3 — a media reference on the first body line — is deliberately
 * not read here: that one IS a markdown embed, so live preview already paints
 * it and the Lezer walk already resolves it. Reading it again would double
 * every hero's hover and lint.
 */
declare function shortcodeAssetRef(name: string, args: string): ShortcodeAssetRef | null;
//#endregion
//#region src/wikilink-grammar.d.ts

declare const wikilinkConfig: MarkdownConfig;
//#endregion
//#region src/math-grammar.d.ts

declare const mathConfig: MarkdownConfig;
//#endregion
//#region src/wikilink-syntax.d.ts
/**
 * Remove the wikilink/embed delimiters (`[[ ]]`, optional leading `!`),
 * preserving the inner text — including any `|alias` / `#anchor`. A bare path
 * passes through unchanged. Use this for the round-tripping picker display
 * where the alias must survive an edit.
 */
declare function stripWikilinkBrackets(raw: string): string;
/**
 * Resolve a wikilink/embed value to the bare reference target: strip the
 * `[[ ]]` delimiters AND drop the `|alias`/`|size` pothole and `#anchor`,
 * mirroring `classify_reference`'s split. Use this before resolving an asset
 * (e.g. the cover FilePicker chip) so the editor resolves the same source file
 * the build does.
 */
declare function wikilinkTarget(raw: string): string;
/** Wrap a bare reference in `[[ ]]` (idempotent; trims first). */
declare function wrapWikilink(raw: string): string;
/**
 * Canonical moss embed insert: produces `![[name]]` from a bare filename or
 * folder path. No URL-encoding — the build fuzzy-resolves bare names;
 * separator/encoded paths 404 on deploy. Folder names ending with `/` are
 * kept as-is (e.g. `![[webapp/]]`).
 */
declare function wrapEmbedWikilink(name: string): string;
//#endregion
//#region src/completion-core.d.ts
type ExtKind = 'Image' | 'Iframe' | 'Pdf' | 'Video' | 'Audio' | 'Model' | 'Transclusion' | 'Notebook' | 'Table' | 'Other';
/**
 * The link syntax the caret is inside. Mirrors Rust
 * `link_completions::LinkSyntax` (serde kebab-case); declared locally for the
 * same reason `ExtKind` is. The backend reads it, with the typed prefix, to
 * decide the FORM every row inserts — the author links to a thing, moss
 * writes the address (docs/archive/2026-09-05-link-target-completion-audit-and-design.md).
 */
type LinkSyntax = 'wikilink' | 'embed' | 'inline' | 'asset-path';
/** One completion returned by the backend (mirrors Rust `WikilinkCompletion`). */
interface WikilinkCompletionItem {
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
type TargetKind = 'page' | 'asset' | 'folder' | 'generated' | 'heading';
interface WikilinkCompleteOptions {
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
  complete: (prefix: string, fromFile: string, syntax: LinkSyntax, page: string | null, allowedKinds?: ExtKind[] | null) => Promise<WikilinkCompletionItem[]>;
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
interface AssetPreviewData {
  absolute_path: string;
  request_url: string;
  mime_type: string;
  width: number | null;
  height: number | null;
  size_bytes: string | number;
}
/**
 * Where the caret is and what it is asking for. Every `from`/`to` is an index
 * INTO THE LINE. The link phases (`wikilink`, `heading`, `link`, `image`,
 * `body`, `attr-value`) all carry the full target span, so one accept path
 * replaces the whole token rather than the prefix before the cursor.
 */
type CompletionPhase = {
  phase: 'name';
  query: string;
  from: number;
} | {
  phase: 'attr-name';
  blockName: string;
  query: string;
  from: number;
} | {
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
} | {
  phase: 'body';
  blockName: string;
  query: string;
  from: number;
  to: number;
} | {
  phase: 'image';
  query: string;
  from: number;
  to: number;
}
/** An open `[[…` / `![[…`: page, asset or folder by source path. */ | {
  phase: 'wikilink';
  embed: boolean;
  query: string;
  from: number;
  to: number;
}
/**
 * After a `#` inside a link target. `page` is the target's page part, or
 * null for the page being edited. The syntax decides what a heading inserts:
 * its text for `[[…#`, its anchor slug for `[…](…#`.
 */ | {
  phase: 'heading';
  syntax: 'wikilink' | 'inline';
  page: string | null;
  query: string;
  from: number;
  to: number;
}
/** An inline `[text](…)` target: source path, or the site with a leading `/`. */ | {
  phase: 'link';
  query: string;
  from: number;
  to: number;
};
/**
 * The wikilink phase at `cursorInLine`, or null when the cursor is not inside
 * an open `[[…` / `![[…` (plain text, or after a closed `[[x]]`). The span
 * runs from just after the brackets (or the `#`) over the typed query and the
 * rest of the target the cursor sits inside (`wikilinkTailLength`).
 */
declare function parseWikilinkPhase(lineText: string, cursorInLine: number): CompletionPhase | null;
/**
 * The phase for an inline `![alt](…)` or `[text](…)` target containing the
 * cursor. An image target is the `image` phase (assets only). A link target
 * is the `link` phase, or the `heading` phase once the author types `#` — a
 * page part that opens with `/` names a place on the published site, whose
 * headings need a source file this parser cannot name, so it offers nothing.
 */
declare function parseInlineTargetPhase(lineText: string, cursorInLine: number): CompletionPhase | null;
/** Minimal doc-line accessor — same shape `blockAlreadyClosed` already uses. */
interface DocLines {
  lines: number;
  line(n: number): {
    text: string;
  };
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
declare function parseAttrPhase(lineText: string, blockName: string, braceAbsIdx: number, cursorInLine: number): CompletionPhase | null;
/** Parse a fence line (`:::name {attrs}`). Returns null if not a fence line. */
declare function parseFenceLine(lineText: string, cursorInLine: number): CompletionPhase | null;
/** Index of the first unescaped `|` in `text`, or -1. */
declare function findUnescapedPipe(text: string): number;
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
declare function parseInlineTarget(lineText: string, cursorInLine: number): {
  query: string;
  from: number;
  to: number;
  image: boolean;
} | null;
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
declare function parseGalleryBodyLine(lineText: string, cursorInLine: number): {
  query: string;
  from: number;
  to: number;
} | null;
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
declare function attrValueSpan(lineText: string, valueFrom: number, fallbackTo: number): {
  to: number;
  pipeSuffix: string;
};
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
declare function renderShortcodeAttrValue(v: string): string;
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
declare function wikilinkTailLength(after: string): number;
/**
 * Scan backward from `lineNumber - 1` for the nearest unclosed `:::name`
 * fence open. A bare close fence (`:::`) encountered first means the last
 * block already closed before this line, so it returns null. Assumes
 * non-nested blocks (moss shortcodes don't nest), matching
 * `blockAlreadyClosed`'s scan direction/assumptions.
 */
declare function findEnclosingBlockName(doc: DocLines, lineNumber: number): string | null;
//#endregion
//#region src/scan.d.ts
/**
 * Text-driven shortcode block scanner — the parse over a plain string that
 * hosts without a `@lezer/markdown` seam need (Obsidian's markdown language
 * is closed to grammar extensions; see
 * packages/obsidian-moss/src/syntax/shortcode-parser.ts, the stub this
 * replaces, and #1020).
 *
 * The line predicates are REUSED from ./shortcode.ts, not copied: an open
 * fence is exactly `SHORTCODE_OPEN_RE` + `isOpenMatch`, a close fence is
 * exactly `isCloseFence`, and open↔close pairing uses the same arity stack as
 * the editor's tree walk (`collectShortcodeBlocks` in
 * ./cm6/cm-shortcode-block.ts): a close fence of arity N closes
 * the TOPMOST open of arity N (`::::` inside `:::` nests); opens above the
 * matched one never got a close and are reported as unclosed children.
 *
 * Two deliberate differences from the Lezer path, both on degenerate input:
 *
 * - **Unclosed blocks are reported, not dropped.** `collectShortcodeBlocks`
 *   discards an open that never closes (a decoration must not half-render);
 *   a host linting while the author is still typing needs the block BEFORE
 *   its close fence exists, so here it comes back with `closeFrom: null` and
 *   `to` at the end of its last body line.
 * - **No markdown block context.** A pure line scan cannot know a `:::`
 *   sits inside a fenced code block or blockquote; the grammar (which runs
 *   inside a real markdown parse) does. On documents without those wrappers
 *   the two agree exactly — `scan.test.ts` cross-checks against the Lezer
 *   parse over the shared structure corpus.
 *
 * CRLF is tolerated the same way the predicates tolerate it (they trim):
 * a trailing `\r` counts as line terminator, so `from`/`to` offsets never
 * include it.
 */
/** One `:::name … :::` block found in a text. Offsets are absolute character
 *  offsets into the string passed to `scanShortcodeBlocks`. */
interface ShortcodeBlock {
  /** Offset of the open-fence line start. */
  from: number;
  /** Offset of the close-fence line end (== last body offset when unclosed). */
  to: number;
  name: string;
  /** Attrs text after the name, trimmed (`{cols=3}`, `2`, `''` when none). */
  attrs: string;
  /** Colon count of the fence (3 for `:::`, 4 for `::::`, …). */
  arity: number;
  /** Offset of the close-fence line start; null when unclosed. */
  closeFrom: number | null;
  children: ShortcodeBlock[];
}
/** Scan `text` for shortcode blocks. Returns the TOP-LEVEL blocks; each
 *  carries its nested `children` (document order, parent before children). */
declare function scanShortcodeBlocks(text: string): ShortcodeBlock[];
/** A known shortcode name, with a one-line doc for diagnostics/completion. */
interface ShortcodeSpec {
  name: string;
  doc: string;
}
/**
 * The authorable shortcode vocabulary, derived from the generated catalog
 * (Rust SSOT: crates/moss-core/src/contract/shortcodes.rs). Non-authorable
 * kinds (`apply`) parse and render but are never offered to authors, so they
 * are excluded here — this list exists for unknown-name diagnostics and
 * completion, both author-facing.
 */
declare const KNOWN_SHORTCODES: ReadonlyArray<ShortcodeSpec>;
//#endregion
//#region src/contract/shortcodes.generated.d.ts
interface ShortcodeAttrInfo {
  readonly name: string;
  /** Present when this attr's VALUE names an asset file; editors scope asset search to these kinds. */
  readonly assetKinds?: readonly string[];
}
interface ShortcodeInfo {
  /** Fence name: `:::name`. */
  readonly name: string;
  readonly attrs: readonly ShortcodeAttrInfo[];
  /** The opening-line attr that names this shortcode's asset (`hero` → `image`). */
  readonly assetAttr?: string;
  /** Canonical English insertion template, CM6 snippet syntax (`${n:placeholder}` tab stops). */
  readonly canonicalTemplate: string;
  /** False = parses and renders but is never offered to authors (e.g. `apply`). */
  readonly authorable: boolean;
}
declare const SHORTCODES: readonly ShortcodeInfo[];
//#endregion
//#region src/footnote-grammar.d.ts

declare const footnoteConfig: MarkdownConfig;
//#endregion
export { AssetPreviewData, AttrKvSpan, CompletionPhase, DocLines, ExtKind, KNOWN_SHORTCODES, LinkSyntax, SHORTCODES, SHORTCODE_OPEN_RE, ShortcodeAssetRef, ShortcodeAttrInfo, ShortcodeBlock, ShortcodeInfo, ShortcodeSpec, TargetKind, WikilinkCompleteOptions, WikilinkCompletionItem, attrValueSpan, findEnclosingBlockName, findUnescapedPipe, footnoteConfig, isCloseFence, isOpenMatch, mathConfig, parseAttrKvSpans, parseAttrPhase, parseFenceLine, parseGalleryBodyLine, parseInlineTarget, parseInlineTargetPhase, parseWikilinkPhase, renderShortcodeAttrValue, scanShortcodeBlocks, shortcodeAssetRef, shortcodeBlockConfig, stripWikilinkBrackets, wikilinkConfig, wikilinkTailLength, wikilinkTarget, wrapEmbedWikilink, wrapWikilink };