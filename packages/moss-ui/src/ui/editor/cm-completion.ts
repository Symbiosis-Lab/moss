/**
 * THE editor completion source: shortcode names and attrs (static catalog),
 * and every link target — `[[…]]`, `![[…]]`, `[…](…)`, `![…](…)`, a shortcode
 * asset attr, a gallery body line, a `#heading` inside any of them.
 *
 * The frontend decides only WHERE the caret is (`parseCompletionContext`) and
 * replaces the whole token on accept (`applyCompletion`). Which targets exist,
 * how they rank, and the exact text each row inserts are the backend's
 * (`wikilink_completions`, syntax-aware); this file only renders the rows.
 *
 * The shortcode catalog it reads (names, attrs, templates, localized
 * strings) is `cm-shortcode-catalog.ts`.
 *
 * IMPORTANT: Do NOT call autocompletion() here. This source is installed by
 * the single autocompletion() call in cm-wikilink-complete.ts.
 */

import { startCompletion } from '@codemirror/autocomplete';
import type { CompletionContext, CompletionResult, Completion } from '@codemirror/autocomplete';
import type { EditorView } from '@codemirror/view';
import { t } from "../i18n/index";
import type {
  ExtKind,
  LinkSyntax,
  WikilinkCompleteOptions,
  WikilinkCompletionItem,
  CompletionPhase,
  DocLines,
} from '@symbiosis-lab/moss-syntax';
import {
  parseFenceLine,
  parseGalleryBodyLine,
  parseInlineTargetPhase,
  parseWikilinkPhase,
  findEnclosingBlockName,
  attrValueSpan,
  renderShortcodeAttrValue,
} from '@symbiosis-lab/moss-syntax';
import { assetCompletionInfo } from './asset-preview.js';
import { SHORTCODE_CATALOG, applyShortcodeTemplate, ts } from './cm-shortcode-catalog.js';

/** Kinds offered for a bare inline `![](…)` target (the 'image' phase). */
const IMAGE_PHASE_KINDS: ExtKind[] = ['Image', 'Video'];

/** Percent presets offered after `|` inside an open embed target. */
const WIDTH_PRESETS = ['25%', '33%', '50%', '66%', '75%', '100%'];

/**
 * Which asset kinds a parsed context accepts, or `null` when it does not
 * restrict the search — a link syntax (`[[`, `[…](`) takes any target, and a
 * `per-line=`/`count=` attr or a block body with no `bodyAssetKinds` takes no
 * file at all (`asksForFile` tells those two apart).
 *
 * ONE dispatch, two readers: the completion source asks it to decide what to
 * search for, and the Browse button asks it to decide what the system file
 * dialog should offer. A second copy would let the dropdown and the dialog
 * drift apart on the same caret position.
 */
export function assetKindsFor(ctx: CompletionPhase): ExtKind[] | null {
  if (ctx.phase === 'attr-value') {
    const spec = SHORTCODE_CATALOG.find((s) => s.name === ctx.blockName);
    return spec?.attrs.find((a) => a.name === ctx.attrName)?.assetKinds ?? null;
  }
  if (ctx.phase === 'body') {
    return SHORTCODE_CATALOG.find((s) => s.name === ctx.blockName)?.bodyAssetKinds ?? null;
  }
  // An inline `![](…)` anywhere — a `:::grid` cell, a linked image, prose.
  // Same kind pair `hero {image=}` uses; the live-preview classifier is
  // extension-driven, so `![](clip.mp4)` is expressible.
  if (ctx.phase === 'image') return IMAGE_PHASE_KINDS;
  return null;
}

/**
 * The text an attr-name completion inserts. A `flag` attr (`scroll`) is a
 * bare keyword the parser reads without `=value` — inserting `=` after it
 * would demand a value nothing consumes. Every other attr keeps the
 * `name=` shape so the caret lands ready to type the value.
 */
export function attrNameApplyText(attr: { name: string; flag?: boolean }): string {
  return attr.flag ? attr.name : `${attr.name}=`;
}

/**
 * Whether the caret is asking for a FILE — the positions where "pick it from
 * my disk" is an answer: an embed wikilink, an inline image, a shortcode attr
 * or body line that takes assets. A `[[` or `[…](` link can name a file too,
 * but it is asking for a link target, and a dialog full of images is the
 * wrong offer there.
 */
export function asksForFile(ctx: CompletionPhase): boolean {
  if (ctx.phase === 'wikilink') return ctx.embed;
  if (ctx.phase === 'image') return true;
  return (ctx.phase === 'attr-value' || ctx.phase === 'body') && assetKindsFor(ctx) !== null;
}

/**
 * THE context parser: what the caret at `lineNumber`/`cursorInLine` is asking
 * for, or null. In order:
 *
 * 1. An open `[[…` / `![[…` ending at the cursor (`wikilink` / `heading`).
 *    First because it is anchored at the cursor and unambiguous, and the
 *    inline scanner below would otherwise read an embed's `!` as an image.
 * 2. A fence line (`:::name {attrs}`): name / attr-name / attr-value phases.
 * 3. A line inside a block with `bodyAssetKinds`: gallery-style `body`.
 * 4. An inline `![alt](…)` (`image`) or `[text](…)` (`link` / `heading`)
 *    target containing the cursor — inside a `:::grid` cell, a linked image,
 *    or ordinary prose.
 */
export function parseCompletionContext(
  doc: DocLines,
  lineNumber: number,
  cursorInLine: number,
): CompletionPhase | null {
  const lineText = doc.line(lineNumber).text;

  const wiki = parseWikilinkPhase(lineText, cursorInLine);
  if (wiki) return wiki;

  const fenceResult = parseFenceLine(lineText, cursorInLine);
  if (fenceResult) return fenceResult;
  if (/^\s*:{3,}/.test(lineText)) return null; // a fence line, just not a match above

  const blockName = findEnclosingBlockName(doc, lineNumber);
  const spec = blockName ? SHORTCODE_CATALOG.find((s) => s.name === blockName) : undefined;

  if (blockName && spec?.bodyAssetKinds) {
    const body = parseGalleryBodyLine(lineText, cursorInLine);
    if (!body) return null;
    return { phase: 'body', blockName, query: body.query, from: body.from, to: body.to };
  }

  return parseInlineTargetPhase(lineText, cursorInLine);
}

/**
 * True when the fence line at `lineNumber` already has a closing fence below
 * it — i.e. the user is re-completing the NAME of an EXISTING block. Scanning
 * stops at the first other OPEN fence (`:::name`): if another block starts
 * before any bare close fence, this block is unclosed.
 *
 * Exported for unit tests.
 */
export function blockAlreadyClosed(state: { doc: { lines: number; line(n: number): { text: string } } }, lineNumber: number): boolean {
  for (let n = lineNumber + 1; n <= state.doc.lines; n++) {
    const text = state.doc.line(n).text;
    if (/^\s*:{3,}\s*$/.test(text)) return true; // bare close fence
    if (/^:{3,}\s*\w/.test(text)) return false;  // another block opens first
  }
  return false;
}

/** The link syntax a phase writes in; null for the catalog phases. */
function syntaxOf(ctx: CompletionPhase): LinkSyntax | null {
  switch (ctx.phase) {
    case 'wikilink': return ctx.embed ? 'embed' : 'wikilink';
    case 'heading': return ctx.syntax;
    case 'link': return 'inline';
    case 'image':
    case 'body':
    case 'attr-value': return 'asset-path';
    default: return null;
  }
}

/** Whether the caret is still in the site the completion was built for: same phase, same block/attr, same link syntax. */
function sameSite(now: CompletionPhase, built: CompletionPhase): boolean {
  if (now.phase !== built.phase) return false;
  switch (built.phase) {
    case 'attr-value':
      return now.phase === 'attr-value' && now.blockName === built.blockName && now.attrName === built.attrName;
    case 'body': return now.phase === 'body' && now.blockName === built.blockName;
    case 'heading': return now.phase === 'heading' && now.syntax === built.syntax;
    default: return true;
  }
}

/**
 * THE accept path for every link target. Exported so the Browse button lands
 * a picked file exactly where an accepted completion would (asset-picker/).
 *
 * > An accepted completion replaces the whole reference TOKEN, not the prefix
 * > before the cursor, and re-renders that token in a form the enclosing
 * > syntax can parse.
 *
 * The token is re-resolved against the LIVE document — the doc may have moved
 * since the async RPC returned. If re-resolution cannot confirm the same phase
 * and block/attr, we fall back to replacing only what the user typed
 * (`[from, to]`), which is the conservative edit: it can leave a stale tail but
 * cannot corrupt an unrelated span from stale offsets.
 *
 * A wikilink is closed with `]]` (Obsidian's convention) and the cursor lands
 * after it — unless `#` or `|` follows the target, where the link continues.
 * A folder row never closes: it is an offer to descend, so the cursor stays at
 * the end of the path and the list reopens inside it. The kind is the
 * backend's word, never read off the text: in URL space a page inserts
 * `/about/`, which ends in `/` and is not a folder.
 */
export function applyCompletion(
  view: EditorView,
  from: number,
  to: number,
  item: Pick<WikilinkCompletionItem, 'insert' | 'kind'>,
  built: CompletionPhase,
): void {
  const text = item.insert;
  const line = view.state.doc.lineAt(to);
  const ctx = parseCompletionContext(view.state.doc, line.number, to - line.from);
  const descend = item.kind === 'folder';
  let start = from;
  let end = to;
  let insert = text;
  if (ctx && sameSite(ctx, built)) {
    if (ctx.phase === 'attr-value') {
      // The value token spans any quotes; a `|attrs` display suffix inside it
      // is the author's, not the path's, and survives verbatim. The cursor is
      // the fallback end for a value with no visible terminator — see
      // `attrValueSpan`.
      const span = attrValueSpan(line.text, ctx.valueFrom, to - line.from);
      start = line.from + ctx.valueFrom;
      end = line.from + span.to;
      insert = renderShortcodeAttrValue(text + span.pipeSuffix);
    } else if (ctx.phase !== 'name' && ctx.phase !== 'attr-name') {
      start = line.from + ctx.from;
      end = line.from + ctx.to;
    }
  }
  let cursor = start + insert.length;
  // Descending into a quoted attr value: stay inside the quotes, or the
  // reopened list would see a finished value and offer nothing.
  if (descend && insert.endsWith('"')) cursor -= 1;

  const closes = built.phase === 'wikilink' || (built.phase === 'heading' && built.syntax === 'wikilink');
  if (closes && !descend) {
    const after = view.state.sliceDoc(end, end + 2);
    if (!after.startsWith('#') && !after.startsWith('|')) {
      if (after !== ']]') insert += ']]';
      cursor += 2;
    }
  }
  view.dispatch({ changes: { from: start, to: end, insert }, selection: { anchor: cursor } });
  if (descend) startCompletion(view);
}

/**
 * One dropdown row for a backend item. The icon is the row's kind
 * (`.cm-completionIcon-<kind>`, styled in editor.css after the file tree's
 * glyphs: chevron for a folder, dot for a file, ring for a generated page);
 * a generated page also says so in words, because a ring alone already means
 * "pending" elsewhere in moss. Asset rows grow a preview card beside the
 * highlighted row, resolving the SAME text `apply` inserts.
 */
function completionRow(
  it: WikilinkCompletionItem,
  built: CompletionPhase,
  opts: WikilinkCompleteOptions,
): Completion {
  const detail = it.kind === 'generated'
    ? `${it.detail ?? it.insert} · ${t('completion.generated')}`
    : it.detail ?? undefined;
  return {
    label: it.label,
    detail,
    type: it.kind,
    apply: (view: EditorView, _c: Completion, applyFrom: number, applyTo: number) =>
      applyCompletion(view, applyFrom, applyTo, it, built),
    info: assetCompletionInfo(it, opts.resolveAssetPreview),
  };
}

/**
 * Ask the backend for the targets of one link phase and render them.
 *
 * No `validFor`: the Rust ranker owns matching AND ranking (substring,
 * same-language-tree, tree-proximity), so CM6 must re-run this source for
 * every new query rather than freezing on the first result; `filter: false`
 * keeps CM6 from re-ranking the backend's already-ordered rows.
 */
async function searchTargets(
  opts: WikilinkCompleteOptions,
  ctx: CompletionPhase,
  syntax: LinkSyntax,
  page: string | null,
  from: number,
): Promise<CompletionResult | null> {
  let items: WikilinkCompletionItem[];
  try {
    items = await opts.complete(ctx.query, opts.getCurrentPath() ?? '', syntax, page, assetKindsFor(ctx));
  } catch {
    return null;
  }
  if (items.length === 0) return null;
  return { from, options: items.map((it) => completionRow(it, ctx, opts)), filter: false };
}

/**
 * The completion source. The name and attr-name phases stay synchronous
 * (static catalog lookups); every link phase dispatches one async RPC search
 * through `opts.complete`, scoped by the phase's syntax and asset kinds.
 * Exported so tests can drive it directly with a CompletionContext built from
 * a markdown EditorState.
 */
export function completionSource(
  opts: WikilinkCompleteOptions,
): (context: CompletionContext) => CompletionResult | Promise<CompletionResult | null> | null {
  return function source(
    context: CompletionContext,
  ): CompletionResult | Promise<CompletionResult | null> | null {
    const line = context.state.doc.lineAt(context.pos);
    const cursorInLine = context.pos - line.from;
    const ctx = parseCompletionContext(context.state.doc, line.number, cursorInLine);
    if (!ctx) return null;

    if (ctx.phase === 'name') {
      const lineNumber = line.number;
      const options: Completion[] = SHORTCODE_CATALOG
        .filter(s => s.name.startsWith(ctx.query))
        .map(s => ({
          label: s.name,
          detail: ts(s.detailKey),
          apply: (view: EditorView) => {
            // Re-resolve the line at APPLY time (the doc may have changed since
            // the source ran). If the block ALREADY has a close fence below,
            // insert only the name — injecting the full template (placeholder
            // body + a premature `:::`) would orphan the existing body.
            const applyLine = view.state.doc.line(
              Math.min(lineNumber, view.state.doc.lines),
            );
            const lineEnd = applyLine.from + applyLine.text.length;
            if (blockAlreadyClosed(view.state, applyLine.number)) {
              view.dispatch({
                changes: { from: applyLine.from + 3, to: lineEnd, insert: s.name },
              });
              return;
            }
            applyShortcodeTemplate(view, s, applyLine.from + 3, lineEnd);
          },
        }));
      if (options.length === 0) return null;
      return { from: line.from + ctx.from, options, validFor: /^[\w-]*$/ };
    }

    if (ctx.phase === 'attr-name') {
      const spec = SHORTCODE_CATALOG.find(s => s.name === ctx.blockName);
      if (!spec || spec.attrs.length === 0) return null;
      const options: Completion[] = spec.attrs
        .filter(a => a.name.startsWith(ctx.query))
        .map(a => ({ label: a.name, detail: ts(a.hintKey), apply: attrNameApplyText(a) }));
      if (options.length === 0) return null;
      return { from: line.from + ctx.from, options, validFor: /^[\w-]*$/ };
    }

    if (ctx.phase === 'width') {
      const options: Completion[] = WIDTH_PRESETS
        .filter(p => p.startsWith(ctx.query))
        .map(p => ({
          label: p,
          apply: (view: EditorView, _c: Completion, from: number, to: number) => {
            const after = view.state.sliceDoc(to, to + 2);
            const insert = after === ']]' ? p : `${p}]]`;
            view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + p.length } });
          },
        }));
      if (options.length === 0) return null;
      return { from: line.from + ctx.from, to: line.from + ctx.to, options, validFor: /^[\d%]*$/ };
    }

    const syntax = syntaxOf(ctx);
    if (!syntax) return null;
    // A `per-line=`/`count=` attr, or a block body that takes no assets.
    if ((ctx.phase === 'attr-value' || ctx.phase === 'body') && !assetKindsFor(ctx)) return null;
    let page: string | null = null;
    if (ctx.phase === 'heading') {
      page = ctx.page ?? opts.getCurrentPath();
      if (!page) return null;
    }
    return searchTargets(opts, ctx, syntax, page, line.from + ctx.from);
  };
}
