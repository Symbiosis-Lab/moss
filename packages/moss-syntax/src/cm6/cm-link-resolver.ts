// cm-link-resolver.ts — link validation as a PULL adapter over the unified cache.
//
// This module provides:
//   - linkValidationExtension() — the @codemirror/lint extension that READS the
//     shared reference cache (cm-reference-resolver.ts) and emits Diagnostic[].
//     It issues NO RPC of its own: the 100ms push adapter in
//     cm-reference-resolver already resolved every (text, is_embed) pair through
//     `editorResolveReferences` (moss-core's `classify_reference`). This linter
//     just maps the cached envelopes for LINK targets (is_embed:false) to
//     diagnostics.
//   - runLinkLintSource() — the pure pull mapper (exported for unit tests).
//
// Link feedback mapping (from the EditorReferenceResolution envelope):
//   - kind:"not-found"               → DIMMED mark (`cm-link-unresolved-dim`),
//     NEVER an error. Typing a link toward a page you plan to create is the
//     canonical wiki workflow — red squiggles punish drafting (Obsidian dims).
//   - kind:"link" with a message     → WARNING (case/slug mismatch advisory)
//   - kind:"moved"                   → ERROR. The URL is gone for certain and
//     the page lives at `url` (a claimed term's generated page); the message
//     names the new place and cmd-click follows it.
//   - kind:"link"/"external"/"anchor" with no message → no feedback (resolved)
//
// Per-node suppression: no diagnostic and no dim mark on the ONE link the
// selection touches (keyed on the whole link node, so editing the label of a
// markdown link suppresses too) — the cursor is the user's attention; feedback
// there is noise while they type. A sibling link on the same line keeps its
// feedback. (Matches the per-node reveal contract — cm-live-preview header.)
//
// The old `resolve_links` RPC + `ResolveLinksFn` + the four shared caches were
// removed in Task 12 — every consumer (lint, hover, cmd-click nav, image render)
// now reads the ONE shared reference cache.

import { ViewPlugin, ViewUpdate, EditorView, Decoration, DecorationSet } from '@codemirror/view';
import { Extension, RangeSetBuilder, type EditorState, type StateEffectType } from '@codemirror/state';
import { linter, type Diagnostic } from '@codemirror/lint';
import { extractLinkTargets } from './cm-link-extract.js';
import { isEmbedNode } from './cm-image-extract.js';
import { syntaxTree } from '@codemirror/language';
import { nodeTouchesSelection } from './cm-active-lines.js';
import { revealInputsChangedIn } from './cm-source-mode.js';

// ── Structural envelope type ──────────────────────────────────────────────────
// The shape of moss's generated `EditorReferenceResolution` (bindings.ts, from
// Rust's editor reference classifier), declared HERE structurally instead of
// imported — the `ExtKind` precedent in completion-core.ts. moss's generated
// type satisfies this with no cast (field-for-field supertype: literal unions
// widen to `string`, payload types to `unknown`/minimal shapes). Only
// `kind.kind` and `message` are read in this module; the other fields are kept
// so a host cache typed with the full envelope remains assignable and so
// downstream consumers of `ReferenceCacheRead` (asset lint, link nav) can read
// `resolved_path`, `candidates` and `resolved.provenance`.
export interface EditorReferenceResolution {
  /** The original reference text that was classified. */
  target: string;
  /** Whether the reference was resolved in embed context. */
  is_embed: boolean;
  /** The classified kind — discriminated on `kind.kind` ("link", "external", "anchor", "not-found", …). */
  kind: { kind: string };
  /** For file/asset embed kinds: the enriched asset. Null for non-asset kinds.
   *  `absolute_path` is the asset's source file — what following the embed
   *  opens (cm-link-nav), the sibling of `resolved_path` below. */
  resolved: { provenance: string; absolute_path: string } | null;
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

// ── Shared read-only cache interface ──────────────────────────────────────────
// The (text, is_embed)-keyed lookup exposed by cm-reference-resolver. Link
// targets are looked up with is_embed:false.
export interface ReferenceCacheRead {
  get(text: string, isEmbed: boolean): EditorReferenceResolution | undefined;
}

// ── Pure pull mapper (exported for unit tests) ────────────────────────────────

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
export function runLinkLintSource(
  view: EditorView,
  cache: ReferenceCacheRead,
): readonly Diagnostic[] {
  const targets = extractLinkTargets(view.state);
  if (targets.length === 0) return [];

  const diagnostics: Diagnostic[] = [];
  for (const { target, from, to, nodeFrom, nodeTo } of targets) {
    // Per-node suppression: feedback hides only for the link the cursor is on.
    // Key on the WHOLE link node (nodeFrom/nodeTo) so suppression matches what
    // cm-live-preview reveals — editing a markdown link's LABEL (not its URL)
    // still suppresses; a sibling link on the same line keeps its advisory.
    // The diagnostic itself stays on the visible target span (from/to).
    if (nodeTouchesSelection(view.state, nodeFrom, nodeTo)) continue;

    const env = cache.get(target, false);
    if (!env) continue; // not yet resolved → no diagnostic (re-run on batch land)

    if (env.kind.kind === 'link' && env.message) {
      // A resolved Link carrying a message is a case/slug mismatch advisory.
      diagnostics.push({
        from,
        to,
        severity: 'warning',
        message: env.message,
        source: 'link-validator',
      });
    } else if (env.kind.kind === 'moved') {
      // A certain 404: the page this URL named moved to `env.url`. Severity
      // is the class's, not parsed out of the message.
      diagnostics.push({
        from,
        to,
        severity: 'error',
        message: env.message ?? `no page at this URL any more — it lives at ${env.url ?? '?'}`,
        source: 'link-validator',
      });
    }
    // not-found → dimmed mark (buildLinkDecorations), never an error.
    // resolved link / external / anchor (no message) → no diagnostic
  }
  return diagnostics;
}

// ── Pure decoration builder (exported for unit tests) ─────────────────────────

const clickableLinkDecoration = Decoration.mark({ class: 'cm-link-clickable' });
const unresolvedDimDecoration = Decoration.mark({ class: 'cm-link-unresolved-dim' });

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
export function buildLinkDecorations(
  state: EditorState,
  cache: ReferenceCacheRead,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  // A RangeSetBuilder demands ascending order and both walks below emit their
  // own, so collect and sort rather than interleaving two ordered streams.
  const ranges: Array<{ from: number; to: number; deco: Decoration }> = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (!isEmbedNode(node.name)) return;
      ranges.push({ from: node.from, to: node.to, deco: clickableLinkDecoration });
      return false; // a nested image inside a linked embed is the same region
    },
  });
  for (const { target, from, to, nodeFrom, nodeTo } of extractLinkTargets(state)) {
    // The WHOLE node, not the target span: in `[text](url)` live preview hides
    // the `](url)` and shows `text`, so a cursor on the target span alone is a
    // cursor on something the reader cannot see — while cm-link-nav's hit test
    // (and therefore the click) has always used the node. The hand and the
    // click must describe the same region or one of them is lying.
    ranges.push({ from: nodeFrom, to: nodeTo, deco: clickableLinkDecoration });
    const env = cache.get(target, false);
    if (
      env?.kind.kind === 'not-found' &&
      // Per-node suppression: dim every not-found link EXCEPT the one the cursor
      // is on. Key on the WHOLE link node (nodeFrom/nodeTo) to match per-node
      // reveal — editing a markdown link's LABEL still suppresses its URL dim;
      // a sibling link keeps its dim cue. The dim mark stays on the target span.
      !nodeTouchesSelection(state, nodeFrom, nodeTo)
    ) {
      ranges.push({ from, to, deco: unresolvedDimDecoration });
    }
  }
  // Ties broken by the shorter range last: a dim mark sits inside its link's
  // clickable node, and RangeSet wants the enclosing range first.
  ranges.sort((a, b) => a.from - b.from || b.to - a.to);
  for (const r of ranges) builder.add(r.from, r.to, r.deco);
  return builder.finish();
}

// ── Extension result type ─────────────────────────────────────────────────────

export interface LinkValidationResult {
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
export function linkValidationExtension(opts: {
  resolvedCache: ReferenceCacheRead;
  refsResolvedEffect: StateEffectType<void>;
}): LinkValidationResult {
  const lintSource = (view: EditorView) => runLinkLintSource(view, opts.resolvedCache);

  const linterExtension = linter(lintSource, {
    delay: 600,
    // Re-run when the resolver's batch lands AND when a reveal input changes —
    // per-node suppression must lift once the cursor leaves the link, and the
    // source-mode flip suppresses every diagnostic at once.
    needsRefresh: (update) =>
      revealInputsChangedIn(update) ||
      update.transactions.some(tr =>
        tr.effects.some(e => e.is(opts.refsResolvedEffect))),
  });

  // ── Link decoration ViewPlugin ───────────────────────────────────────
  // `cm-link-clickable` on every target span (the CSS rule
  // `.cm-editor.cm-meta-held .cm-link-clickable { cursor: pointer; }` fires
  // when Cmd/Ctrl is held) + `cm-link-unresolved-dim` on not-found targets
  // except the link the cursor is on (per-node). Read-only — no doc changes.
  // cm-link-nav's click handler already guards on resolution before navigating,
  // so a pointer on an unresolved link that no-ops on click is acceptable UX.
  const linkDecorationsPlugin = ViewPlugin.fromClass(
    class LinkDecorator {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = buildLinkDecorations(view.state, opts.resolvedCache);
      }

      update(update: ViewUpdate) {
        // selectionSet: the dim mark follows the touched link (per-node).
        // refsResolvedEffect: dim marks appear when the batch lands.
        const batchLanded = update.transactions.some(tr =>
          tr.effects.some(e => e.is(opts.refsResolvedEffect)));
        if (revealInputsChangedIn(update) || update.viewportChanged || batchLanded) {
          this.decorations = buildLinkDecorations(update.state, opts.resolvedCache);
        }
      }
    },
    { decorations: v => v.decorations },
  );

  return {
    extension: [linterExtension, linkDecorationsPlugin],
  };
}
