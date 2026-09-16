/**
 * Pure Lezer-based image/asset-target extractor.
 *
 * Walks the full `syntaxTree(state)` and collects the TARGET reference for
 * every image or asset embed in the document:
 *
 *   - Standard markdown image  `![alt](url)`    → Lezer `Image`, URL child range
 *   - Wikilink embed           `![[file.png]]`  → `WikilinkEmbed`, whole node range
 *
 * The two are DIFFERENT node types and are told apart by NAME, via
 * [`isEmbedNode`] — never by probing for an absent `URL` child. See ADR-041.
 *
 * For standard images the `from`/`to` span is the URL child node — i.e. the
 * path text only, not the surrounding `(…)`.
 * For wikilink embeds the `from`/`to` span is the WHOLE node — i.e.
 * `![[file.png]]` in its entirety. (A `WikilinkTarget` child exists, but
 * narrowing to it would shrink every hover/lint/dim span; deliberately not
 * done here.)
 *
 * This module is PURE — it only reads `EditorState` and returns data.
 * It has no CM6 side effects (no ViewPlugin, no StateField, no Decoration).
 */

import type { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { shortcodeAssetRef } from '../shortcode.js';

/**
 * True for the Lezer node names that mean "an asset embed": a standard
 * markdown image `![alt](url)` (`Image`) and a wikilink embed `![[file]]`
 * (`WikilinkEmbed`).
 *
 * ADR-041 invariant: an embed is identified by node NAME. The old
 * `node.name === 'Image' && !getChild('URL')` discriminator was a negative
 * test four modules re-derived independently; this is the one predicate.
 */
export function isEmbedNode(name: string): boolean {
  return name === 'Image' || name === 'WikilinkEmbed';
}

export interface ImageTarget {
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

const NAMED_WIDTHS = new Set(['body', 'wide', 'page', 'screen', 'full']);

/**
 * Recognize one width segment (named or percent) → canonical string.
 * MIRROR of moss-core `media::parse_image_width` — agrees for every
 * canonical/moss-emitted width (named tokens, `NN%`, `NN.N%`; the write path
 * only ever produces these). They may diverge on malformed hand-typed input
 * (e.g. `"55 %"`, `".5%"`) which this read-side regex rejects but Rust's
 * `f64::parse` accepts; harmless since the editor never authors such strings.
 * Keep the canonical/clamp/format behavior in sync.
 */
function parseImageWidth(seg: string): string | undefined {
  const s = seg.trim();
  if (NAMED_WIDTHS.has(s)) return s === 'full' ? 'screen' : s;
  const m = /^(\d+(?:\.\d+)?)%$/.exec(s);
  if (m) {
    const v = Math.min(parseFloat(m[1]), 100);
    if (v <= 0) return undefined;
    // `${v}%` already formats 50 → "50" and 50.5 → "50.5" (matches Rust).
    return `${v}%`;
  }
  return undefined;
}

/**
 * Pull a width segment out of pipe-delimited text; returns the first match.
 * Exported so the live-preview layer (cm-live-preview.ts) reuses this single
 * read-side width parser instead of writing a third copy — keeping editor↔build
 * parity (this module MIRRORS moss-core `parse_image_width`).
 */
export function widthFromPipe(text: string): string | undefined {
  for (const seg of text.split('|')) {
    const w = parseImageWidth(seg);
    if (w) return w;
  }
  return undefined;
}

// ── embedParts — the ONE embed accessor ──────────────────────────────

/** Which syntax produced an embed. */
export type EmbedSyntax = 'markdown-image' | 'wikilink-embed';

/** Everything a consumer needs from an embed node, read from the tree. */
export interface EmbedParts {
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
export interface EmbedNodeRef {
  name: string;
  from: number;
  to: number;
  node: { getChild(name: string): { from: number; to: number } | null };
}

/** Structural slice of a Lezer SyntaxNode (avoids importing @lezer/common). */
export interface TreeNode {
  name: string;
  from: number;
  to: number;
  getChild(name: string): TreeNode | null;
  parent: TreeNode | null;
}

interface DocSlice { sliceString(from: number, to: number): string }

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
export function embedParts(node: EmbedNodeRef, doc: DocSlice): EmbedParts | null {
  if (node.name === 'Image') {
    const url = node.node.getChild('URL');
    if (!url) return null;
    // Deliberately NOT trimmed — matches what extractImageTargets has always
    // reported. cm-email-guard trims at its own call site.
    const target = doc.sliceString(url.from, url.to);
    if (!target.trim()) return null;
    // KNOWN BUG, reproduced verbatim: @lezer/markdown has no
    // `ImageClosingMark` node type, so this is always null and every markdown
    // image renders alt="". Fixing it changes ImageBlockWidget.eq() identity
    // for every markdown image in every open document (a mass widget rebuild)
    // and is unrelated to the grammar — tracked separately.
    const closeBracket = node.node.getChild('ImageClosingMark');
    const alt = closeBracket ? doc.sliceString(node.from + 2, closeBracket.from) : '';
    // Width rides in the alt text between `![` and `](`.
    const altMatch = /^!\[([^\]]*)\]/.exec(doc.sliceString(node.from, node.to));
    return {
      syntax: 'markdown-image',
      target,
      targetFrom: url.from,
      targetTo: url.to,
      alt,
      pothole: null,
      width: altMatch ? widthFromPipe(altMatch[1]) : undefined,
    };
  }

  if (node.name !== 'WikilinkEmbed') return null;
  const t = node.node.getChild('WikilinkTarget');
  if (!t) return null;
  const target = doc.sliceString(t.from, t.to).trim();
  if (!target) return null;
  // The pothole is everything between the `|` and the closing `]]`. The
  // grammar ends WikilinkTarget at the `|` and the node ends just past `]]`,
  // so both bounds are structural.
  const hasPothole = doc.sliceString(t.to, t.to + 1) === '|';
  const pothole = hasPothole ? doc.sliceString(t.to + 1, node.to - 2) : '';
  return {
    syntax: 'wikilink-embed',
    target,
    // The WHOLE node, not the WikilinkTarget child: narrowing would shrink
    // every hover/lint/unresolved-dim span this feeds.
    targetFrom: node.from,
    targetTo: node.to,
    // A PRESENT pothole wins even when empty — `![[a.png|]]` has alt ''.
    alt: hasPothole ? pothole.trim() : target,
    pothole: hasPothole ? pothole : null,
    width: hasPothole ? widthFromPipe(pothole) : undefined,
  };
}

// ── The folder-listing pothole ───────────────────────────────────────

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
export interface FolderEmbedParams {
  style?: string;
  sort?: 'date' | 'weight' | 'title';
  depth?: string;
  group?: string;
  limit?: number;
}

/** `sort:` values the build recognises; anything else clears the axis. */
const SORT_AXES = new Set(['date', 'weight', 'title']);

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
export function parseFolderParams(raw: string): FolderEmbedParams {
  const out: FolderEmbedParams = {};
  for (const rawTok of raw.split(',')) {
    const tok = rustTrim(rawTok);
    if (!tok) continue;
    const colon = tok.indexOf(':');
    if (colon === -1) continue; // bare flag / sizing token — not charted here
    const k = rustTrim(tok.slice(0, colon));
    const v = rustTrim(tok.slice(colon + 1));
    switch (k) {
      case 'limit': out.limit = parseUsize(v); break;
      case 'sort': out.sort = SORT_AXES.has(v) ? (v as FolderEmbedParams['sort']) : undefined; break;
      case 'style': out.style = v; break;
      case 'depth': out.depth = v; break;
      case 'group': out.group = v; break;
      default: break; // unknown key: dropped, exactly as the build drops it
    }
  }
  return out;
}

/**
 * Rust `str::trim`, not JS `String.prototype.trim`.
 *
 * The two disagree on exactly two code points, and a fuzz differential over
 * the real parsers found both: `U+0085` NEL is Unicode `White_Space` (Rust
 * trims it, JS does not) and `U+FEFF` ZWNBSP is in the JS `WhiteSpace`
 * production but has no `White_Space` property (JS trims it, Rust does not).
 * The BUILD is the authority for what a param means, so the card mirrors
 * Rust — `sort:<NEL>date` sorts by date, `limit:<BOM>3` sets no limit.
 *
 * The class below is the complete Unicode `White_Space` set, which is fixed
 * (no new members since Unicode 4.1), so this cannot drift with a Node
 * upgrade. Pinned by the `nel-*` / `bom-*` shared vectors.
 */
const RUST_WS = '\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const RUST_TRIM_RE = new RegExp(`^[${RUST_WS}]+|[${RUST_WS}]+$`, 'g');
function rustTrim(s: string): string {
  return s.replace(RUST_TRIM_RE, '');
}

/** Rust `usize::from_str`: an optional leading `+`, digits only, in range. */
function parseUsize(v: string): number | undefined {
  if (!/^\+?\d+$/.test(v)) return undefined;
  const n = BigInt(v);
  if (n > 18446744073709551615n) return undefined; // u64::MAX — out of usize range
  return Number(n);
}

/**
 * The parsed folder pothole of an embed, or **null** when this is not a
 * wikilink embed.
 *
 * Null for a markdown image `![](/awards/)` on purpose: the build dispatches a
 * folder-listing marker only from the `![[…]]` form, so that form lists
 * nothing and a card must not claim otherwise.
 */
export function folderParamsFromEmbed(parts: EmbedParts): FolderEmbedParams | null {
  if (parts.syntax !== 'wikilink-embed') return null;
  return parseFolderParams(parts.pothole ?? '');
}

/** Chip order is fixed so two embeds with identical semantics render
 *  identically regardless of typing order. */
const CHIP_ORDER = ['style', 'sort', 'depth', 'group', 'limit'] as const;

/**
 * Canonical `key:value` chips for a parsed pothole.
 *
 * Derived from the PARSED params, never from the raw text: a token the build
 * silently drops produces no chip, and that absence is the diagnostic.
 * `limit:0` is omitted because the build's truncation guard is `n > 0` — a
 * zero limit does nothing.
 */
export function folderChips(p: FolderEmbedParams): string[] {
  const out: string[] = [];
  for (const key of CHIP_ORDER) {
    const v = p[key];
    if (v === undefined) continue;
    if (key === 'limit' && v === 0) continue;
    out.push(`${key}:${v}`);
  }
  return out;
}

// ── The linked-embed unit ────────────────────────────────────────────

/** A `[…](url)` link whose entire text is one embed — ONE clickable image. */
export interface LinkedEmbed {
  /** The embed that is the link's whole text. */
  embed: TreeNode;
  /** The whole `[…](url)` span. */
  link: { from: number; to: number };
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
export function linkedEmbedOf(link: TreeNode, doc: DocSlice): LinkedEmbed | null {
  if (link.name !== 'Link') return null;
  const url = link.getChild('URL');
  if (!url) return null;
  const href = doc.sliceString(url.from, url.to).trim();
  if (!href) return null;
  const embed = link.getChild('WikilinkEmbed') ?? link.getChild('Image');
  if (!embed) return null;
  // Nothing but whitespace between the opening `[` and the embed …
  if (doc.sliceString(link.from + 1, embed.from).trim() !== '') return null;
  // … and nothing but whitespace between the embed and the closing `]`.
  let k = embed.to;
  while (doc.sliceString(k, k + 1) === ' ') k++;
  if (doc.sliceString(k, k + 1) !== ']') return null;
  if (doc.sliceString(link.from, link.to).indexOf('\n') !== -1) return null;
  return { embed, link: { from: link.from, to: link.to }, href, urlFrom: url.from, urlTo: url.to };
}

/** The linked-embed unit an embed node belongs to, or null if it is bare. */
export function linkUnitOfEmbed(embed: TreeNode, doc: DocSlice): LinkedEmbed | null {
  const parent = embed.parent;
  return parent && parent.name === 'Link' ? linkedEmbedOf(parent, doc) : null;
}

/**
 * The asset a `ShortcodeOpenLine` names in its attributes, as a document-span
 * `ImageTarget` — or null when the fence has no name, no attrs, or names no
 * asset (every shortcode but `hero` today).
 *
 * Covered through `extractImageTargets`, which is its only caller — testing it
 * directly would need a hand-built cursor and would assert the same thing.
 */
function shortcodeOpenLineAsset(
  openLine: { node: { cursor(): TreeCursorLike } },
  doc: DocSlice,
): ImageTarget | null {
  let name = '';
  let attrsFrom = -1;
  let attrsTo = -1;
  const c = openLine.node.cursor();
  if (c.firstChild()) {
    do {
      if (c.name === 'ShortcodeName') name = doc.sliceString(c.from, c.to);
      else if (c.name === 'ShortcodeAttrs') { attrsFrom = c.from; attrsTo = c.to; }
    } while (c.nextSibling());
  }
  if (!name || attrsFrom < 0) return null;
  const ref = shortcodeAssetRef(name, doc.sliceString(attrsFrom, attrsTo));
  if (!ref) return null;
  return { target: ref.target, from: attrsFrom + ref.from, to: attrsFrom + ref.to, attr: true };
}

/** The slice of CM6's `TreeCursor` this module walks. */
interface TreeCursorLike {
  name: string;
  from: number;
  to: number;
  firstChild(): boolean;
  nextSibling(): boolean;
}

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
export function extractImageTargets(state: EditorState): ImageTarget[] {
  const out: ImageTarget[] = [];

  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === 'ShortcodeOpenLine') {
        const ref = shortcodeOpenLineAsset(node, state.doc);
        if (ref) out.push(ref);
        return; // descend — nothing else on the line, but no reason to prune
      }
      // `![alt](url)` is an `Image`; `![[file]]` is a `WikilinkEmbed`.
      // Identified by NAME (ADR-041) — not by probing for a URL child.
      if (!isEmbedNode(node.name)) return;
      const parts = embedParts(node, state.doc);
      if (parts) {
        out.push({
          target: parts.target,
          from: parts.targetFrom,
          to: parts.targetTo,
          width: parts.width,
        });
      }
      return false; // atomic — embed children need not be visited
    },
  });

  return out;
}

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
export function imageNodeAtWidget(
  view: { state: EditorState; posAtDOM(node: Node): number },
  dom: Node,
  intraLineOffset: number,
): { from: number; to: number } | null {
  let pos: number;
  try {
    pos = view.posAtDOM(dom);
  } catch (_) {
    return null;
  }
  if (pos < 0 || pos > view.state.doc.length) return null;
  const line = view.state.doc.lineAt(pos);
  const candidates: { from: number; to: number }[] = [];
  syntaxTree(view.state).iterate({
    from: line.from,
    to: line.to,
    enter(node) {
      // `return;` — NEVER `return false`. Pruning descent here would stop the
      // iteration entering a `Link` that wraps an embed, so a linked embed's
      // resize gesture would find zero candidates and silently do nothing.
      if (!isEmbedNode(node.name)) return;
      // Always the EMBED span, never the wrapping Link: `set_image_width`
      // requires an exact `![[…]]` / `![…](…)` string and returns its input
      // unchanged for anything else.
      candidates.push({ from: node.from, to: node.to });
      return false;
    },
  });
  if (candidates.length === 0) return null;
  let best = candidates[0];
  let bestDist = Math.abs(best.from - line.from - intraLineOffset);
  for (const c of candidates) {
    const d = Math.abs(c.from - line.from - intraLineOffset);
    if (d < bestDist) { best = c; bestDist = d; }
  }
  return best;
}

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
export function embedNodeAt(
  state: EditorState,
  pos: number,
): { from: number; to: number } | null {
  let node: { name: string; from: number; to: number; parent: any } | null =
    syntaxTree(state).resolveInner(pos, 1);
  while (node && !isEmbedNode(node.name)) node = node.parent;
  return node ? { from: node.from, to: node.to } : null;
}
