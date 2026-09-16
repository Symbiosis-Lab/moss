/**
 * @lezer/markdown fence-marker grammar for :::shortcode blocks.
 *
 * Emits the open and close fence lines as SEPARATE single-line marker nodes;
 * the body between is left to the normal markdown parser, so images/emphasis/
 * links become real nodes the live-preview layer renders. The decoration field
 * (cm-shortcode-block.ts) pairs Open↔Close markers with an arity stack.
 *
 * This is the single frontend parser for shortcode blocks — it replaces the
 * regex TS tokenizer and the async render-scan. Decorations read it from
 * syntaxTree(state), synchronously, so they never go stale (no edit glitches).
 *
 * NOT a composite block: composites are for prefix-marker blocks (Blockquote);
 * a fence-delimited composite infinite-loops because parse() can't advance past
 * the open line before the continuation re-examines it.
 */

import type { MarkdownConfig, BlockContext, Line } from '@lezer/markdown';
import { SHORTCODES } from './contract/shortcodes.generated.js';

// Opener: optional indent, >=3 colons, a name starting [A-Za-z], rest = attrs.
// The name is OPTIONAL — `:::{.class}` (docs/reference/shortcode-grammar.md
// "Pure-CSS region") has none, only an attrs block starting with `{`. A bare
// `:::` (no name, no `{`) is a close fence, not an opener — callers must
// still reject that case themselves (see `isOpenMatch`).
export const SHORTCODE_OPEN_RE = /^(\s*)(:{3,})([A-Za-z][\w-]*)?(.*)$/;

/** True when `m` (a `SHORTCODE_OPEN_RE` match) is a real opener: either it
 *  has a name, or its remainder is an attrs block (starts with `{`). Shared
 *  by `parse` and `endLeaf` so the two can't drift on what counts as open. */
export function isOpenMatch(m: RegExpExecArray | null): boolean {
  if (!m) return false;
  if (m[3]) return true;
  return m[4].trim().startsWith('{');
}

/** A pure-colon line of arity >=3 is a close fence. */
export function isCloseFence(lineText: string): boolean {
  const t = lineText.trim();
  return t.length >= 3 && /^:+$/.test(t);
}

export const shortcodeBlockConfig: MarkdownConfig = {
  defineNodes: [
    'ShortcodeOpenLine',
    'ShortcodeName',
    'ShortcodeAttrs',
    'ShortcodeCloseLine',
  ],
  parseBlock: [
    {
      name: 'ShortcodeOpenLine',
      parse(cx: BlockContext, line: Line): boolean {
        const m = SHORTCODE_OPEN_RE.exec(line.text.slice(line.pos));
        if (!isOpenMatch(m)) return false;
        const arity = m![2].length;
        const wsLen = m![1].length;
        const from = cx.lineStart + line.pos + wsLen;
        const lineEnd = cx.lineStart + line.text.length;
        const nameFrom = from + arity;
        const nameTo = nameFrom + (m![3]?.length ?? 0);
        const children = m![3] ? [cx.elt('ShortcodeName', nameFrom, nameTo)] : [];
        if (m![4] && m![4].trim().length) {
          // Attrs node spans end-of-name..EOL, intentionally including the
          // leading space before `{` (consumers trim). When there's no name,
          // nameTo === nameFrom, so this spans the whole `{...}` block.
          children.push(cx.elt('ShortcodeAttrs', nameTo, lineEnd));
        }
        cx.addElement(cx.elt('ShortcodeOpenLine', from, lineEnd, children));
        cx.nextLine();
        return true;
      },
      endLeaf(_cx: BlockContext, line: Line): boolean {
        const m = SHORTCODE_OPEN_RE.exec(line.text.slice(line.pos));
        return isOpenMatch(m);
      },
      before: 'FencedCode',
    },
    {
      name: 'ShortcodeCloseLine',
      parse(cx: BlockContext, line: Line): boolean {
        if (!isCloseFence(line.text.slice(line.pos))) return false;
        cx.addElement(cx.elt('ShortcodeCloseLine', cx.lineStart + line.pos, cx.lineStart + line.text.length));
        cx.nextLine();
        return true;
      },
      endLeaf(_cx: BlockContext, line: Line): boolean {
        return isCloseFence(line.text.slice(line.pos));
      },
      before: 'FencedCode',
    },
  ],
};

// ── Attribute blocks ────────────────────────────────────────────────────────
//
// Where a shortcode's asset lives on its opening line.
//
// `:::hero {image=photo.jpg}` names an image, but the name sits in an
// ATTRIBUTE, not in a markdown embed — so `cm-image-extract`'s Lezer walk
// (`Image` / `WikilinkEmbed` nodes) never saw it. Three things fall out of
// that one blind spot: the reference resolver never resolved the path, so the
// hero tag had nothing to draw a thumbnail from; the hover popover had no
// target; and a hero pointing at a deleted file got no broken-asset cue while
// every `![[…]]` in the same document did. `extractImageTargets` calls
// `shortcodeAssetRef` below, so all three are fixed at one seam.
//
// This lives beside the fence grammar rather than in a module of its own
// because it IS grammar: the same opening line, one level further in. The
// Lezer parse above says where the attrs are; this says what they mean.
//
// ## Port, not an approximation
//
// `parseAttrKvSpans` is a MIRROR of moss-core `ast::attrs::parse_attrs_spanned`
// (crates/moss-core/src/ast/attrs.rs), including the two quirks that decide
// what an author's typo does: an empty `.`/`#` bareword is skipped silently,
// and the FIRST malformed item aborts the whole block. Rust's callers write
// `.unwrap_or_default()` on that error, i.e. the block parses as empty; here
// the same case returns `null` and the caller reads no attrs. A looser reader
// would light a "missing asset" underline under a path the build never looks
// at, which is worse than staying quiet.
//
// ## Known limit: single line only
//
// moss-core absorbs a `{` that doesn't close on its opening line
// (`gather_multi_line_attrs`). The grammar above does not — `ShortcodeAttrs`
// ends at EOL — so neither does this, and a hero whose attrs wrap gets no
// thumbnail and no hover. It also gets no micro-tag params today, for the same
// reason: `SHORTCODE_OPEN_RE` is line-based. One limitation, not a new one.

/** One `key=value` item read off an attribute block. */
export interface AttrKvSpan {
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

/** MIRROR of `attrs::is_key_start`. */
const isKeyStart = (c: string): boolean => /[A-Za-z]/.test(c);
/** MIRROR of `attrs::is_key_continue`. */
const isKeyContinue = (c: string): boolean => /[A-Za-z0-9_-]/.test(c);
/**
 * MIRROR of `attrs::is_bareword` — Unicode-alphanumeric, not ASCII, so a
 * `頭像.png` / `café.jpg` written unquoted reads the same here as in the
 * build. `\p{Alphabetic}` + `\p{N}` is Rust's `char::is_alphanumeric`.
 */
const isBareword = (c: string): boolean => /[:/._-]/.test(c) || /[\p{Alphabetic}\p{N}]/u.test(c);

/** MIRROR of `attrs::match_width_token` — the bare keywords that are NOT an error. */
const WIDTH_TOKENS = new Set(['body', 'wide', 'page', 'screen', 'full']);

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
export function parseAttrKvSpans(input: string): AttrKvSpan[] | null {
  const n = input.length;
  let i = 0;
  const skipWs = (): void => { while (i < n && /\s/.test(input[i])) i++; };

  skipWs();
  if (input[i] !== '{') return null; // MissingOpenBrace
  i++;

  const out: AttrKvSpan[] = [];
  for (;;) {
    skipWs();
    if (i >= n) return null; // UnclosedBrace
    const c = input[i];

    if (c === '}') return out;

    if (c === '.' || c === '#') {
      i++;
      while (i < n && isBareword(input[i])) i++; // read_bareword, discarded
      continue;
    }

    if (!isKeyStart(c)) return null; // InvalidKey

    const keyFrom = i;
    i++;
    while (i < n && isKeyContinue(input[i])) i++;
    const keyTo = i;
    const key = input.slice(keyFrom, keyTo);

    skipWs();
    if (input[i] !== '=') {
      // A bare keyword is legal only for the width tokens; anything else is
      // `InvalidKey`, which aborts the block.
      if (WIDTH_TOKENS.has(key)) continue;
      return null;
    }
    i++;
    skipWs();

    if (input[i] === '"') {
      i++;
      const from = i;
      let value = '';
      for (;;) {
        if (i >= n) return null; // UnterminatedQuote
        const ch = input[i];
        if (ch === '"') { out.push({ key, value, from, to: i, keyFrom, keyTo }); i++; break; }
        if (ch === '\\') {
          i++;
          if (i >= n) return null; // UnterminatedQuote
          value += input[i];
          i++;
          continue;
        }
        value += ch;
        i++;
      }
      continue;
    }

    if (i >= n || !isBareword(input[i])) return null; // EmptyValue
    const from = i;
    while (i < n && isBareword(input[i])) i++;
    out.push({ key, value: input.slice(from, i), from, to: i, keyFrom, keyTo });
  }
}

/**
 * The attribute each shortcode names an asset in, read off the generated
 * catalog (Rust SSOT: crates/moss-core/src/contract/shortcodes.rs). This used
 * to be a hand copy of a fact `SHORTCODE_CATALOG` also carried by hand;
 * both now derive from the same artifact, so there is nothing left to drift
 * (`cm-shortcode-lezer.test.ts` still asserts the derivation reads what the
 * artifact says).
 *
 * `gallery` has no `assetAttr` on purpose: its assets are markdown images in
 * the BODY, which the Lezer walk already sees.
 */
const ASSET_ATTR_BY_SHORTCODE: ReadonlyMap<string, string> = new Map(
  SHORTCODES.flatMap((s) => (s.assetAttr ? [[s.name, s.assetAttr] as const] : [])),
);

/** An asset path named on a shortcode's opening line. */
export interface ShortcodeAssetRef {
  /** The path, with any `|display attrs` suffix already split off. */
  target: string;
  /** Span of the path within the `args` string passed in. */
  from: number;
  to: number;
}

/**
 * MIRROR of moss-core `media::split_pipe`: everything before the first `|` is
 * the path, the rest is display attributes (`cover top`, `color=…`).
 */
function splitPipe(raw: string): string {
  const bar = raw.indexOf('|');
  return bar === -1 ? raw : raw.slice(0, bar);
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
export function shortcodeAssetRef(name: string, args: string): ShortcodeAssetRef | null {
  const attr = ASSET_ATTR_BY_SHORTCODE.get(name);
  if (!attr) return null;

  const brace = args.indexOf('{');

  if (brace !== -1) {
    const kvs = parseAttrKvSpans(args.slice(brace));
    const hit = kvs?.find((kv) => kv.key === attr);
    if (hit) {
      const path = splitPipe(hit.value);
      const trimmed = path.trimStart();
      const lead = path.length - trimmed.length;
      const target = trimmed.trimEnd();
      if (!target) return null;
      const from = brace + hit.from + lead;
      return { target, from, to: from + target.length };
    }
  }

  // Priority 2: a bare path before the attribute block.
  const positionalRaw = brace === -1 ? args : args.slice(0, brace);
  const path = splitPipe(positionalRaw);
  const trimmed = path.trimStart();
  const target = trimmed.trimEnd();
  if (!target) return null;
  const from = path.length - trimmed.length;
  return { target, from, to: from + target.length };
}
