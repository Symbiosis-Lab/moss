// cm-link-clickable.ts — the followable-cursor mark on every link and embed,
// and the structural shape of the host's reference cache.
//
// Whether a link is broken, moved or ambiguous is the host's to say: it owns
// the resolution, the wording and the fixes, and reports every content problem
// through one mark of its own. This module only marks where a Cmd/Ctrl-click
// can follow, which needs no resolution at all.

import { ViewPlugin, ViewUpdate, EditorView, Decoration, DecorationSet } from '@codemirror/view';
import { Extension, RangeSetBuilder, type EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { extractLinkTargets } from './cm-link-extract.js';
import { isEmbedNode } from './cm-image-extract.js';

// ── Structural envelope type ──────────────────────────────────────────────────
// The shape of moss's generated `EditorReferenceResolution` (bindings.ts, from
// Rust's editor reference classifier), declared HERE structurally instead of
// imported — the `ExtKind` precedent in completion-core.ts. moss's generated
// type satisfies this with no cast (field-for-field supertype: literal unions
// widen to `string`, payload types to `unknown`/minimal shapes), so a host
// cache typed with the full envelope stays assignable to `ReferenceCacheRead`.
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

/** The (text, is_embed)-keyed lookup a host's reference cache exposes. */
export interface ReferenceCacheRead {
  get(text: string, isEmbed: boolean): EditorReferenceResolution | undefined;
}

const clickableLinkDecoration = Decoration.mark({ class: 'cm-link-clickable' });

/**
 * `cm-link-clickable` on every link NODE and every EMBED node — the class the
 * host's Cmd/Ctrl-held cursor rule keys on. Exported for unit tests.
 *
 * Embeds are a separate walk rather than an addition to `extractLinkTargets`:
 * that extractor skips `WikilinkEmbed` on purpose, because a host may render an
 * embed in place of its text. While it is rendered its source is hidden and
 * nothing is painted; the moment the cursor reveals the raw `![[ … ]]`, a press
 * follows it and the hand must say so.
 */
export function buildLinkDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  // A RangeSetBuilder demands ascending order and both walks below emit their
  // own, so collect and sort rather than interleaving two ordered streams.
  const ranges: Array<{ from: number; to: number }> = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (!isEmbedNode(node.name)) return;
      ranges.push({ from: node.from, to: node.to });
      return false; // a nested image inside a linked embed is the same region
    },
  });
  for (const { nodeFrom, nodeTo } of extractLinkTargets(state)) {
    // The WHOLE node, not the target span: in `[text](url)` the click hit test
    // uses the node, and the hand and the click must describe the same region.
    ranges.push({ from: nodeFrom, to: nodeTo });
  }
  ranges.sort((a, b) => a.from - b.from || b.to - a.to);
  for (const r of ranges) builder.add(r.from, r.to, clickableLinkDecoration);
  return builder.finish();
}

/** The followable-cursor mark, rebuilt as the document or the viewport changes. */
export function linkClickableExtension(): Extension {
  return ViewPlugin.fromClass(
    class LinkClickable {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = buildLinkDecorations(view.state);
      }

      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = buildLinkDecorations(update.state);
        }
      }
    },
    { decorations: v => v.decorations },
  );
}
