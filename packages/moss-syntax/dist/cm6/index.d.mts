import * as _codemirror_state0 from "@codemirror/state";
import { EditorState, Extension, StateEffectType, StateField, Transaction } from "@codemirror/state";
import { HighlightStyle } from "@codemirror/language";
import { Decoration, DecorationSet, EditorView, ViewUpdate } from "@codemirror/view";
import { Diagnostic } from "@codemirror/lint";

//#region src/cm6/cm-active-lines.d.ts
/** Returns the set of 1-based line numbers that contain any selection range.
 *  In source mode: every line of the document. Unfocused: none. */
declare function getActiveLines(state: EditorState): Set<number>;
/** Returns true if any line of the node range overlaps with active lines. */
declare function isNodeActive(state: EditorState, from: number, to: number, activeLines: Set<number>): boolean;
/** True when [from, to] touches any line a selection range is on. */
declare function spanOnActiveLine(state: EditorState, from: number, to: number): boolean;
/**
 * True if any selection range overlaps the closed interval [from, to].
 *
 * Boundary-inclusive (non-strict `<=`): a collapsed caret parked exactly at
 * `from` or `to` counts as touching, so a node revealed on edge contact is
 * editable. Strict operators are deliberately avoided — they cause the
 * "markers re-hide and trap the cursor outside the run" class of bugs.
 *
 * This is the inline (per-node) reveal predicate — the granular counterpart to
 * isNodeActive's per-line test. Used by cm-live-preview (Emphasis/Strong/
 * Strikethrough/InlineCode/Link/Wikilink) and cm-link-resolver (link feedback).
 */
declare function nodeTouchesSelection(state: EditorState, from: number, to: number): boolean;
//#endregion
//#region src/cm6/cm-source-mode.d.ts
/** Set source mode on/off. Effect-only transactions (no doc change) — the
 *  save state machine never sees a toggle. */
declare const setSourceModeEffect: _codemirror_state0.StateEffectType<boolean>;
declare const sourceModeField: StateField<boolean>;
/** True when the document is in source mode. Safe on states without the
 *  field (non-markdown editors don't register it) — defaults to false. */
declare function isSourceMode(state: EditorState): boolean;
/**
 * True when this transaction flipped source mode. Decoration StateFields and
 * ViewPlugins gate their rebuilds on doc/selection changes; a toggle changes
 * neither, so every builder that reads the predicates must ALSO rebuild on
 * this — otherwise the mode switch would not repaint until the next keystroke.
 */
declare function sourceModeChanged(tr: Transaction): boolean;
/**
 * The one rebuild gate for every consumer of the reveal predicates: did this
 * transaction change anything getActiveLines / nodeTouchesSelection reads?
 * Doc, selection — and the mode flip and the editor gaining or losing focus,
 * which are effect-only and therefore invisible to the doc/selection checks
 * alone. Builders compose their own
 * extras (treeAdvanced, refsResolved) on top; they must never re-spell this
 * set, because a hand-written gate is how four consumers shipped stale
 * decorations across the flip (thermo review, 2026-09-01).
 */
declare function revealInputsChanged(tr: Transaction): boolean;
/** ViewUpdate-shaped twin of revealInputsChanged, for ViewPlugin update gates. */
declare function revealInputsChangedIn(update: ViewUpdate): boolean;
//#endregion
//#region src/cm6/cm-editor-focus.d.ts
/** The host's report that the user started or stopped working in the editor. */
declare const setEditorFocusedEffect: _codemirror_state0.StateEffectType<boolean>;
/** Install to make reveals follow focus. Starts unfocused, as a newly opened
 *  editor is until the user reaches into it. */
declare const editorFocusField: StateField<boolean>;
/** True when the host installed `editorFocusField` and the editor is not
 *  focused: no line is active and no node touches the selection. */
declare function isRevealSuspended(state: EditorState): boolean;
/** True when this transaction suspended or resumed reveals — effect-only, so
 *  invisible to the doc/selection checks alone. */
declare function revealSuspensionChanged(tr: Transaction): boolean;
//#endregion
//#region src/cm6/cm-link-extract.d.ts
interface ExtractedTarget {
  /** The link target text (URL path or wikilink page name). */
  target: string;
  /** Start offset of the target text in the document (inclusive). */
  from: number;
  /** End offset of the target text in the document (exclusive). */
  to: number;
  /**
   * Range of the ENCLOSING link node (the whole `[text](url)` / `[[Page]]`),
   * i.e. the unit cm-live-preview reveals as one. Feedback suppression keys on
   * THIS range so it matches per-node reveal granularity, while the dim mark /
   * diagnostic is still placed on the visible `from`/`to` target span.
   */
  nodeFrom: number;
  nodeTo: number;
}
/**
 * Walk the full syntax tree of `state` and return one `ExtractedTarget` for
 * every navigable link: standard markdown links and wikilinks.
 *
 * Results are in document order (tree iteration is depth-first, left-to-right).
 */
declare function extractLinkTargets(state: EditorState): ExtractedTarget[];
//#endregion
//#region src/cm6/cm-image-extract.d.ts
/**
 * True for the Lezer node names that mean "an asset embed": a standard
 * markdown image `![alt](url)` (`Image`) and a wikilink embed `![[file]]`
 * (`WikilinkEmbed`).
 *
 * ADR-041 invariant: an embed is identified by node NAME. The old
 * `node.name === 'Image' && !getChild('URL')` discriminator was a negative
 * test four modules re-derived independently; this is the one predicate.
 */
declare function isEmbedNode(name: string): boolean;
interface ImageTarget {
  /** The resolved asset reference: a URL path or a wikilink filename. */
  target: string;
  /** Start offset of the target span in the document (inclusive). */
  from: number;
  /** End offset of the target span in the document (exclusive). */
  to: number;
  /** Parsed width token (`"55%"` or named) if present, else undefined. */
  width?: string;
  /**
   * True when the path was written as a shortcode ATTRIBUTE
   * (`:::hero {image=photo.jpg}`) rather than as a markdown embed.
   *
   * Callers that ask "is this already on screen?" must branch on it. An
   * embed's answer is yes — live preview paints it full size, which is why
   * the hover popover suppresses itself over one. A hero attribute's answer
   * is no: nothing paints it but the micro-tag's ~18px thumbnail, so the
   * hover is the only way to actually see the picture.
   */
  attr?: boolean;
}
/**
 * Pull a width segment out of pipe-delimited text; returns the first match.
 * Exported so the live-preview layer (cm-live-preview.ts) reuses this single
 * read-side width parser instead of writing a third copy — keeping editor↔build
 * parity (this module MIRRORS moss-core `parse_image_width`).
 */
declare function widthFromPipe(text: string): string | undefined;
/** Which syntax produced an embed. */
type EmbedSyntax = 'markdown-image' | 'wikilink-embed';
/** Everything a consumer needs from an embed node, read from the tree. */
interface EmbedParts {
  syntax: EmbedSyntax;
  /** The asset reference text. */
  target: string;
  /** Target span: the URL child for a markdown image, the whole node for an embed. */
  targetFrom: number;
  targetTo: number;
  /** Display text (the pothole for `![[a|b]]`, the alt text for `![b](a)`). */
  alt: string;
  /**
   * The raw `|`-delimited pothole text, or null when the embed has none.
   *
   * Distinct from `alt`, which falls back to the target when a wikilink embed
   * carries no pothole: `![[a.png]]` has alt `'a.png'` and pothole `null`.
   * A markdown image has no pothole at all — its width rides in the alt text.
   */
  pothole: string | null;
  /** Parsed width token (`"55%"` or named) if present. */
  width?: string;
}
/**
 * Structural slice of a Lezer node ref — matching this module's existing
 * convention of never importing @lezer/common.
 */
interface EmbedNodeRef {
  name: string;
  from: number;
  to: number;
  node: {
    getChild(name: string): {
      from: number;
      to: number;
    } | null;
  };
}
/** Structural slice of a Lezer SyntaxNode (avoids importing @lezer/common). */
interface TreeNode {
  name: string;
  from: number;
  to: number;
  getChild(name: string): TreeNode | null;
  parent: TreeNode | null;
}
interface DocSlice {
  sliceString(from: number, to: number): string;
}
/**
 * Read an embed node's target, display text and width FROM THE TREE.
 *
 * This is the single accessor that replaced four independent hand-rolled
 * re-scans of the raw source (in this module, cm-live-preview and
 * cm-email-guard) — the ADR-036 "parse once, lower to many" rule: never a
 * second hand-rolled scan of source the Lezer tree already models.
 *
 * Discriminates on `node.name` (ADR-041), never on the presence of a `URL`
 * child. Returns null when the node is not an embed, is structurally
 * incomplete, or has an empty target — `![[]]` (which the slash menu inserts
 * on every image verb) and `![]()` must NOT enter the resolve batch.
 */
declare function embedParts(node: EmbedNodeRef, doc: DocSlice): EmbedParts | null;
/**
 * The five params the folder-listing branch of the build acts on.
 *
 * `size` is deliberately absent: the listing branch ignores sizing entirely
 * (`moss_core::resolve::embed_renderer::folder_list`), so charting it would
 * promise the author something the site does not do.
 *
 * `style`, `depth` and `group` are `string`, never a union — the build keeps
 * any value for them and a narrower type here would silently drop a param the
 * site honours.
 */
interface FolderEmbedParams {
  style?: string;
  sort?: 'date' | 'weight' | 'title';
  depth?: string;
  group?: string;
  limit?: number;
}
/**
 * Parse the `key:value,key:value` pothole of a folder embed.
 *
 * Parity with the build's `parse_params` is GATED, not asserted in prose:
 * `crates/moss-core/tests/fixtures/folder-embed-params.vectors.json` is run by
 * this module's test AND by `crates/moss-core/tests/folder_embed_params.rs`.
 * **Add a vector before you add a key** — a param added on the Rust side alone
 * produces a silently missing chip rather than a red test.
 *
 * The rule most easily got wrong, and the reason the vectors exist: each keyed
 * token ASSIGNS, so a later unparseable value CLEARS an earlier good one
 * (`sort:weight,sort:weght` → no sort). Bare tokens produce none of the five.
 */
declare function parseFolderParams(raw: string): FolderEmbedParams;
/**
 * The parsed folder pothole of an embed, or **null** when this is not a
 * wikilink embed.
 *
 * Null for a markdown image `![](/awards/)` on purpose: the build dispatches a
 * folder-listing marker only from the `![[…]]` form, so that form lists
 * nothing and a card must not claim otherwise.
 */
declare function folderParamsFromEmbed(parts: EmbedParts): FolderEmbedParams | null;
/**
 * Canonical `key:value` chips for a parsed pothole.
 *
 * Derived from the PARSED params, never from the raw text: a token the build
 * silently drops produces no chip, and that absence is the diagnostic.
 * `limit:0` is omitted because the build's truncation guard is `n > 0` — a
 * zero limit does nothing.
 */
declare function folderChips(p: FolderEmbedParams): string[];
/** A `[…](url)` link whose entire text is one embed — ONE clickable image. */
interface LinkedEmbed {
  /** The embed that is the link's whole text. */
  embed: TreeNode;
  /** The whole `[…](url)` span. */
  link: {
    from: number;
    to: number;
  };
  href: string;
  urlFrom: number;
  urlTo: number;
}
/**
 * Recognise `[![[x.png]]](/url)` / `[![alt](x.png)](/url)` — a link whose text
 * is exactly one embed, which live preview renders as one clickable image.
 *
 * SHAPE ONLY. Non-null iff all of:
 *   1. the node is a `Link`;
 *   2. it has a `URL` child spanning a non-empty string (so `[…][ref]` and
 *      `[…]()` are rejected — they have no destination to click through to);
 *   3. it has exactly one embed child;
 *   4. the link's text is ONLY that embed (rejects `[see ![[x]] here](/u)` and
 *      `[![[a]] ![[b]]](/u)`);
 *   5. the whole thing is on ONE line — the multi-line card form
 *      (`[` \n … \n `](url)`) is `findBlockLinks`' domain, and keeping this
 *      single-line means this introduces no new overlap with it.
 */
declare function linkedEmbedOf(link: TreeNode, doc: DocSlice): LinkedEmbed | null;
/** The linked-embed unit an embed node belongs to, or null if it is bare. */
declare function linkUnitOfEmbed(embed: TreeNode, doc: DocSlice): LinkedEmbed | null;
/**
 * Walk the syntax tree of `state` and return one `ImageTarget` for every
 * image or asset embed:
 *
 *   - `![alt](url)`     → `{ target: url, from: urlFrom, to: urlTo }`
 *   - `![[file.png]]`   → `{ target: 'file.png', from: nodeFrom, to: nodeTo }`
 *   - `![[f.png|alt]]`  → `{ target: 'f.png', from: nodeFrom, to: nodeTo }`
 *
 * …plus the one place an asset is named OUTSIDE an embed — a shortcode
 * attribute on an opening fence:
 *
 *   - `:::hero {image=p.jpg}` → `{ target: 'p.jpg', …, attr: true }`
 *
 * That last case is why this function is the right seam for it rather than a
 * fourth scanner: resolution, the broken-asset underline and the hover
 * popover all read this one list, so a hero's image joins all three at once.
 *
 * Results are in document order (depth-first, left-to-right tree iteration) —
 * `cm-reference-resolver` feeds them straight to a `RangeSetBuilder`, which
 * requires it. A shortcode's target is pushed when its OPEN LINE is entered,
 * and everything already pushed lies before that line, so order holds.
 */
declare function extractImageTargets(state: EditorState): ImageTarget[];
/**
 * Locate the Image node a block widget belongs to, AT GESTURE TIME.
 *
 * Block image/embed widgets must not bake absolute document positions into
 * their DOM: `eq()` deliberately excludes positions (so edits above the image
 * reuse the DOM without an <img> flash), which means a captured `nodeFrom`
 * goes stale after any edit above. Instead, the widget asks the view where its
 * DOM currently sits (`view.posAtDOM`) and resolves the Image node on that
 * line fresh from the syntax tree.
 *
 * `intraLineOffset` is the Image node's offset WITHIN its source line,
 * captured at build time. It is stable under edits above the line and
 * disambiguates multiple images anchored to the same line end: the candidate
 * whose intra-line offset is closest wins.
 *
 * Returns null when the widget's position cannot be mapped or no Image node
 * exists on the line (e.g. the source was deleted mid-gesture) — callers
 * must treat that as "do not commit".
 */
declare function imageNodeAtWidget(view: {
  state: EditorState;
  posAtDOM(node: Node): number;
}, dom: Node, intraLineOffset: number): {
  from: number;
  to: number;
} | null;
/**
 * Resolve the embed node enclosing `pos` — the source span a width rewrite
 * replaces. Handles BOTH `![alt](url)` and every `![[…]]` form (folder, PDF,
 * video, iframe), because both are embeds by name (ADR-041).
 *
 * The one owner of the resolveInner-and-climb, so a caller cannot climb for
 * `'Image'` only and silently no-op on every wikilink embed.
 *
 * Returns null when `pos` is not inside an embed.
 */
declare function embedNodeAt(state: EditorState, pos: number): {
  from: number;
  to: number;
} | null;
//#endregion
//#region src/cm6/cm-criticmarkup.d.ts
type CMType = 'addition' | 'deletion' | 'substitution' | 'highlight' | 'comment';
interface ParsedMark {
  /** Character offset of the opening `{` */
  from: number;
  /** Character offset just after the closing `}` */
  to: number;
  /** Token type */
  type: CMType;
  /** For substitution only: offset of the `~>` split */
  mid?: number;
}
/**
 * Parse CriticMarkup tokens from the document, skipping tokens inside code
 * regions (fenced blocks, indented blocks, inline spans) as reported by the
 * SYNTAX TREE — the same code regions the editor highlights as code.
 *
 * @returns Array of parsed marks in document order.
 */
declare function parseMarks(state: EditorState): ParsedMark[];
/**
 * CM6 extension that highlights CriticMarkup tokens.
 *
 * Incremental strategy (PATTERN 7): on each transaction, `RangeSet.map` re-
 * positions the existing decorations through the change set in O(log n). Only
 * when the change inserts/deletes a CriticMarkup or code trigger character —
 * or the change is unusually large — do we fall back to a full re-parse.
 * Typical typing of plain prose costs one `RangeSet.map`, not a whole-document
 * regex scan. Effect-only transactions rebuild only when the incremental
 * parse advanced (code masking beyond the initially-parsed region).
 */
declare function criticmarkupExtension(): Extension;
//#endregion
//#region src/cm6/cm-highlight.d.ts
declare const mossHighlight: HighlightStyle;
declare function mossHighlightExtension(): Extension;
//#endregion
//#region src/cm6/cm-shortcode-block.d.ts
interface ShortcodeBlockStrings {
  /** Tooltip on a resting tag whose named asset does not resolve. */
  assetMissingHint: () => string;
  /** Inline label after a deprecated `---` cell divider. */
  legacyDividerLabel: () => string;
  /** Tooltip on that label. */
  legacyDividerTooltip: () => string;
}
interface ShortcodeBlockInfo {
  from: number;
  to: number;
  name: string;
  attrs: string;
  closeFrom: number;
  arity: number;
  children: ShortcodeBlockInfo[];
}
/**
 * Pair Open↔Close fence markers with an arity stack. Returns the TOP-LEVEL
 * blocks; each carries its nested `children` so the decoration layer can render
 * `::::buttons` inside `:::grid` instead of leaking the inner fences as raw
 * text. (Nesting parity with the Rust extractor — see cm-shortcode-scanner.)
 */
declare function collectShortcodeBlocks(state: EditorState): ShortcodeBlockInfo[];
/** Flatten a block tree to a document-ordered list (parent before children). */
declare function flattenBlocks(blocks: ShortcodeBlockInfo[]): ShortcodeBlockInfo[];
/**
 * Char ranges covered by every `:::` fence BODY — the open and close fence
 * lines excluded, document order, non-overlapping.
 *
 * Only TOP-LEVEL blocks contribute a range. A nested `::::buttons` lives inside
 * its parent's body by construction, so adding it would produce a second range
 * covering ground the parent already covers — and a caller asking "is this
 * position in a fence body?" would then have to dedupe. One range per top-level
 * block answers that question in one pass.
 *
 * Exists so the editor can render a fence body at STRUCTURE density (media
 * become one-line tokens) while ordinary body text is untouched — see
 * docs/archive/2026-08-21-editor-container-media-density-design.md. Lives here
 * rather than in the editor so there stays exactly one reader of the block
 * tree; two readers of the same structure drift.
 */
declare function shortcodeBodyRanges(state: EditorState): {
  from: number;
  to: number;
}[];
/** True when `pos` falls inside any fence body. Linear over the ranges, which
 *  number one per top-level block on the page — the caller that runs per
 *  syntax-tree node computes the ranges ONCE and passes them in. */
declare function inShortcodeBody(ranges: {
  from: number;
  to: number;
}[], pos: number): boolean;
/** A body line that is exactly `+++` — the cell divider (mirrors the Rust
 *  `split_grid_cells` / `cells::split_cells` rule; legacy `---` form omitted). */
declare function isCellDividerLine(text: string): boolean;
/**
 * A body line that is exactly `---` — the OLD spelling of a cell divider.
 * Still accepted by the build (`split_grid_cells`), still deprecated, and only
 * ever a divider inside `:::grid`: `split_cells`, which every other cell type
 * uses, does not know `---` at all. Mirrors `match_divider` in the Rust
 * `editor_scan`, which likewise flags `---` only at grid depth.
 */
declare function isLegacyDividerLine(text: string): boolean;
/**
 * True when the INNERMOST block containing `lineFrom` in its body is a
 * cell-dividing shortcode (grid or buttons). A `+++` inside a nested
 * `::::buttons` still divides (buttons is a cell type); it renders as literal
 * body text only when the innermost container is a NON-cell type (e.g. hero) —
 * matching the build, which splits cells per-block, not recursively.
 */
declare function dividesCellsAt(lineFrom: number, subtree: ShortcodeBlockInfo[]): boolean;
/**
 * True when a `---` on this line is a deprecated cell divider rather than
 * ordinary content — i.e. the innermost enclosing block is a `:::grid`.
 * Inside `:::buttons` (or anywhere else) `---` is just text, and hinting
 * there would be wrong.
 */
declare function legacyDividesCellsAt(lineFrom: number, subtree: ShortcodeBlockInfo[]): boolean;
/**
 * Count the legacy `---` dividers the Rust `editor_scan` would also count:
 * only those directly in a TOP-LEVEL `:::grid` body. Exists so the dev-only
 * divergence check can compare the two parsers on dividers, not just on block
 * names — the grid-only rule above is now written twice and would otherwise
 * drift silently.
 */
declare function topLevelLegacyDividerCount(state: EditorState): number;
/**
 * Block-range activation — the editor-wide reveal contract: any selection
 * range TOUCHING [from, to] reveals the block's source. Matches the
 * selection-overlap activation tables and inline marks use (getActiveLines /
 * isNodeActive in cm-live-preview), so Cmd+A and multi-line drags reveal
 * shortcode fences like everything else. (Was head-only before, which made
 * shortcode blocks the one construct Cmd+A did not reveal.)
 */
declare function isBlockActive(state: EditorState, b: ShortcodeBlockInfo): boolean;
/**
 * A nameless block has no name to label the tag with, so the class list
 * IS the name — it's the only thing the author typed that identifies the
 * block. `{.tagline}` → "tagline"; `{.subscribe-card .wide}` →
 * "subscribe-card wide". Returns null when there's no class to show (an
 * attrs-only block with no class, which shouldn't normally occur but must
 * not crash the tag into a blank label).
 */
declare function classListLabel(attrs: string): string | null;
/** Params shown in the tag (on hover), for layout-ambiguous types. */
declare function tagParams(attrs: string): string;
/**
 * What a resting block should draw where its icon goes.
 *
 * - `{ url }` — the shortcode names an image that resolved; draw it.
 * - `'missing'` — it names a path nothing resolves to. This is the ONLY place
 *   that can be said: a resting block's source line is replaced, so the
 *   `.cm-asset-unresolved` underline the same target earns elsewhere is not
 *   on screen to be seen. Without this the tag would look identical whether
 *   the file existed or not, right up until publish refuses.
 * - `null` — no asset named, not resolved yet, or resolved to something with
 *   no still frame (video). Falls back to the glyph, which is never wrong.
 */
type ShortcodeAsset = {
  url: string;
} | 'missing' | null;
/** Resolve a shortcode's asset path to what its tag should draw. */
type ResolveShortcodeAsset = (target: string) => ShortcodeAsset;
interface ShortcodeBlockOptions {
  /**
   * Resolves a shortcode's asset path to a thumbnail. Omitted → every tag
   * keeps its glyph, which is the pre-thumbnail behaviour exactly.
   */
  resolveAsset?: ResolveShortcodeAsset;
  /**
   * `cm-reference-resolver`'s "a batch of references resolved" effect, passed
   * in rather than imported: that module reaches `../bindings`, and this one
   * is kept off that import graph (same reason `asset-preview` declares its
   * own envelope type, and the same shape `assetLintExtension` takes).
   */
  refsResolvedEffect?: StateEffectType<unknown>;
  /**
   * Host-injected UI strings (thunks; see `ShortcodeBlockStrings`). Missing
   * entries fall back to the English defaults. moss passes `() => t('…')`
   * so locale switches stay live; other hosts may pass nothing.
   */
  strings?: Partial<ShortcodeBlockStrings>;
}
declare function shortcodeBlockExtension(opts?: ShortcodeBlockOptions): Extension;
//#endregion
//#region src/cm6/cm-link-resolver.d.ts
interface EditorReferenceResolution {
  /** The original reference text that was classified. */
  target: string;
  /** Whether the reference was resolved in embed context. */
  is_embed: boolean;
  /** The classified kind — discriminated on `kind.kind` ("link", "external", "anchor", "not-found", …). */
  kind: {
    kind: string;
  };
  /** For file/asset embed kinds: the enriched asset. Null for non-asset kinds.
   *  `absolute_path` is the asset's source file — what following the embed
   *  opens (cm-link-nav), the sibling of `resolved_path` below. */
  resolved: {
    provenance: string;
    absolute_path: string;
  } | null;
  /** For Link kinds: the resolved deployed URL. */
  url: string | null;
  /** For Link kinds with a `#fragment`: the anchor. */
  anchor: string | null;
  /** The absolute source FILE this reference opens when followed: a resolved
   *  internal Link, or a FolderListing embed (the folder's index source, never
   *  the directory). Always a file — every consumer opens it. */
  resolved_path: string | null;
  /** Human-readable diagnostic note (separator-fallback / case-mismatch / …). */
  message: string | null;
  /** For Ambiguous: all candidate paths. */
  candidates: string[];
  /** For FolderListing embeds: what the last build listed. */
  folder: unknown;
}
interface ReferenceCacheRead {
  get(text: string, isEmbed: boolean): EditorReferenceResolution | undefined;
}
/**
 * Map the cached LINK envelopes for every extracted link span to Diagnostic[].
 *
 * Reads the shared reference cache with is_embed:false. Targets not yet in the
 * cache (the batch hasn't landed) produce no diagnostic — the linter re-runs
 * via `needsRefresh(refsResolvedEffect)` once the batch dispatches its effect.
 *
 * Pure over (state, cache) — no async, no IPC. One diagnostic per extracted
 * span (so the same target appearing twice gets two diagnostics).
 */
declare function runLinkLintSource(view: EditorView, cache: ReferenceCacheRead): readonly Diagnostic[];
/**
 * Build the link-feedback decorations:
 *   - `cm-link-clickable` on EVERY link NODE and every EMBED node (Cmd-held
 *     cursor affordance).
 *   - `cm-link-unresolved-dim` on not-found targets, EXCEPT the one the cursor
 *     is on (per-node) — while the user is typing toward a future page, no
 *     feedback at all for that link; sibling links keep their dim cue.
 *
 * Embeds get the hand but never the dim, which is why they are a separate walk
 * rather than an addition to `extractLinkTargets`: that extractor skips
 * `WikilinkEmbed` on purpose (ADR-041) because a lint underline belongs on link
 * text, not on a rendered card. The CURSOR is a different question — an embed
 * is exactly as followable as a link, and cm-link-nav has followed one since
 * `followTargetAt` learned about embeds. While the card is rendered its source
 * is hidden and nothing is painted; the moment the cursor reveals the raw
 * `![[ … ]]` above the card, the press follows it and the hand must say so.
 */
declare function buildLinkDecorations(state: EditorState, cache: ReferenceCacheRead): DecorationSet;
interface LinkValidationResult {
  /** CM6 extension array (linter + clickable-link decoration). Install via spread. */
  extension: Extension[];
}
/**
 * Create a CM6 extension that validates links by READING the shared reference
 * cache (no RPC of its own).
 *
 * - Uses Lezer-based `extractLinkTargets`.
 * - The lint source is synchronous (a pure pull over the cache); the `linter()`
 *   delay (600 ms) keeps the diagnostic paint debounced.
 * - `needsRefresh(refsResolvedEffect)` re-runs the source when the reference
 *   resolver's batch lands (initial resolve AND the post-FileChanged re-batch,
 *   which the resolver triggers by clearing its cache + re-scheduling).
 * - `@codemirror/lint` shows the diagnostic message on hover natively.
 *
 * @param opts.resolvedCache       Shared reference cache (read-only lookup).
 * @param opts.refsResolvedEffect  The effect the resolver dispatches after a batch.
 */
declare function linkValidationExtension(opts: {
  resolvedCache: ReferenceCacheRead;
  refsResolvedEffect: StateEffectType<void>;
}): LinkValidationResult;
//#endregion
//#region src/cm6/cm-footnote.d.ts
/** A decoration to place, in the shape cm-live-preview collects. */
interface FootnoteDeco {
  from: number;
  to: number;
  deco: Decoration;
}
/** Where a footnote label is written: the whole construct, and its label span. */
interface FootnoteSite {
  /** The whole `[^1]` / `[^1]: …` node. */
  from: number;
  to: number;
  /** The label text alone — where a jump puts the caret, and what is raised. */
  labelFrom: number;
  labelTo: number;
  /** End of the trailing mark (`]` or `]:`), i.e. the end of the CHROME. On a
   *  definition this is well short of `to`; the note body follows. */
  markEnd: number;
}
/**
 * Every footnote in the document, indexed by label: where it is defined, and
 * where it is first referenced.
 *
 * One walk answers all three questions the feature asks — may this marker
 * render (is it defined), does this definition earn a back-jump (is it
 * referenced), and where does either jump land. Splitting them across three
 * walks is how a marker and its jump end up disagreeing about which
 * definition is "the" one when a label is defined twice.
 *
 * Duplicate labels: FIRST wins, on both sides. The build renders the first
 * definition's body and back-links to the first reference (ADR-035), so the
 * editor's jump lands where the reader's would.
 */
declare function footnoteIndex(state: EditorState): {
  definitions: Map<string, FootnoteSite>;
  firstRefs: Map<string, FootnoteSite>;
};
/**
 * Where following the footnote at `pos` should land, or null.
 *
 * A footnote is a reference like any other, so it answers the same two
 * gestures every other reference does (Cmd/Ctrl+click and Mod-Enter, wired in
 * cm-link-nav.ts) — but it navigates WITHIN the document rather than opening a
 * file, so it cannot go through `navigateToReference`.
 *
 * Both directions, matching what the built page gives a reader:
 *   - on a marker  → its definition;
 *   - on a definition's MARKER → its first reference (the build's `↩`
 *     back-link).
 *
 * The definition side is deliberately scoped to the `[^1]:` chrome rather than
 * the whole note. The note body is prose the author edits, and Mod-Enter with
 * the caret in the middle of a sentence should insert a line, not teleport.
 *
 * Ends excluded, the rule `followTargetAt` uses and for the same reason: at
 * `node.from` the caret is beside the construct, not on it, and Mod-Enter
 * should still be Mod-Enter there.
 */
declare function footnoteJumpTarget(state: EditorState, pos: number): FootnoteSite | null;
/**
 * Decorations for every footnote marker and definition in `state`.
 *
 * Returns an empty array for the overwhelmingly common case of a document
 * with no footnotes, so the cost of the feature on an ordinary page is one
 * tree walk that matches nothing.
 */
declare function footnoteDecorations(state: EditorState, activeLines: Set<number>): FootnoteDeco[];
/** Base styles, so the feature needs no edit to a stylesheet to work. */
declare const footnoteTheme: Extension;
/**
 * The extension: a ViewPlugin that decorates the visible document, plus the
 * base theme. Registered separately from cm-live-preview's plugin rather than
 * folded into it — the two produce disjoint ranges, and keeping this pass
 * standalone means the footnote feature is one file to read and one line to
 * remove.
 */
declare function footnoteExtension(): Extension;
//#endregion
export { CMType, EditorReferenceResolution, EmbedNodeRef, EmbedParts, EmbedSyntax, ExtractedTarget, FolderEmbedParams, FootnoteDeco, FootnoteSite, ImageTarget, LinkValidationResult, LinkedEmbed, ParsedMark, ReferenceCacheRead, ResolveShortcodeAsset, ShortcodeAsset, ShortcodeBlockInfo, ShortcodeBlockOptions, ShortcodeBlockStrings, TreeNode, buildLinkDecorations, classListLabel, collectShortcodeBlocks, criticmarkupExtension, dividesCellsAt, editorFocusField, embedNodeAt, embedParts, extractImageTargets, extractLinkTargets, flattenBlocks, folderChips, folderParamsFromEmbed, footnoteDecorations, footnoteExtension, footnoteIndex, footnoteJumpTarget, footnoteTheme, getActiveLines, imageNodeAtWidget, inShortcodeBody, isBlockActive, isCellDividerLine, isEmbedNode, isLegacyDividerLine, isNodeActive, isRevealSuspended, isSourceMode, legacyDividesCellsAt, linkUnitOfEmbed, linkValidationExtension, linkedEmbedOf, mossHighlight, mossHighlightExtension, nodeTouchesSelection, parseFolderParams, parseMarks, revealInputsChanged, revealInputsChangedIn, revealSuspensionChanged, runLinkLintSource, setEditorFocusedEffect, setSourceModeEffect, shortcodeBlockExtension, shortcodeBodyRanges, sourceModeChanged, sourceModeField, spanOnActiveLine, tagParams, topLevelLegacyDividerCount, widthFromPipe };