/**
 * Pure Lezer-based link-target extractor.
 *
 * Walks the full `syntaxTree(state)` and collects the TARGET span for every
 * navigable link in the document:
 *
 *   - Standard markdown links  `[text](url)`  → `URL` node
 *   - Wikilinks                `[[Page]]`      → `WikilinkTarget` node
 *
 * Intentionally excluded:
 *   - Image embeds   `![alt](url)`       — `Image` node (not `Link`); skipped
 *   - Embed wikilinks `![[file]]`        — `WikilinkEmbed` node; skipped
 *   - Links inside fenced code blocks    — Lezer's block parser claims fences
 *     before inline parsers run, so no `Link`/`Wikilink` node is ever emitted
 *     inside a `FencedCode` span; no special guard is needed.
 *
 * The `from`/`to` positions are the TARGET span only (i.e. the URL text or the
 * wikilink target text), not the enclosing link node.  This allows a later lint
 * diagnostic to underline the target precisely.
 *
 * This module is PURE — it only reads `EditorState` and produces data.  It has
 * no CM6 side effects (no ViewPlugin, no StateField, no Decoration).
 */

import { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';

export interface ExtractedTarget {
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
export function extractLinkTargets(state: EditorState): ExtractedTarget[] {
  const results: ExtractedTarget[] = [];
  const tree = syntaxTree(state);

  tree.iterate({
    enter(node) {
      // ── Standard markdown link: [text](url) ───────────────────────────
      // Lezer emits a `Link` node for `[text](url)`.  The URL is a child `URL`
      // node.  `Image` nodes (`![alt](url)`) share the same URL child but are
      // NOT `Link` nodes, so we only reach here for real hyperlinks.
      if (node.name === 'Link') {
        // Walk children looking for the URL node.
        const inner = node.node;
        let child = inner.firstChild;
        while (child) {
          if (child.name === 'URL') {
            const from = child.from;
            const to = child.to;
            results.push({
              target: state.doc.sliceString(from, to),
              from,
              to,
              nodeFrom: node.from, // whole [text](url) — the reveal unit
              nodeTo: node.to,
            });
            break; // a Link has at most one URL child
          }
          child = child.nextSibling;
        }
        return false; // stop descending — we already handled the URL child
      }

      // ── Wikilink: [[Page]] or [[Page|Display]] ────────────────────────
      // Our custom `wikilinkConfig` (cm-wikilink-lezer.ts) emits a `Wikilink`
      // node with a `WikilinkTarget` child that spans only the page name.
      //
      // `target` is always the page name (the resolution key). `from`/`to` are
      // the VISIBLE span: the page name for a plain `[[Page]]`, or the alias for
      // `[[Page|Display]]` — matching exactly what cm-live-preview renders, so
      // the clickable/dim mark (cm-link-resolver) and Cmd+click nav (cm-link-nav)
      // land on the text the user actually sees.
      if (node.name === 'Wikilink') {
        // Embeds need no guard here: `![[file]]` is a `WikilinkEmbed` node,
        // a different name this branch never sees. The old
        // character peek at `node.from - 1` also suppressed the escaped form
        // `\![[Page]]`, which is a genuine link and is now extracted.

        // Walk children for the WikilinkTarget node and the closing mark.
        const inner = node.node;
        let targetNode: { from: number; to: number } | null = null;
        let closeMark: { from: number; to: number } | null = null;
        let sawOpenMark = false;
        let child = inner.firstChild;
        while (child) {
          if (child.name === 'WikilinkTarget') {
            targetNode = { from: child.from, to: child.to };
          } else if (child.name === 'WikilinkMark') {
            if (sawOpenMark) closeMark = { from: child.from, to: child.to };
            else sawOpenMark = true;
          }
          child = child.nextSibling;
        }
        if (targetNode) {
          // Trim whitespace for backend lookup (e.g. `[[  Research  ]]`).
          const target = state.doc.sliceString(targetNode.from, targetNode.to).trim();
          // Aliased when a `|` immediately follows the target (the grammar ends
          // WikilinkTarget at the `|`). Underline the alias span; fall back to
          // the target span for a plain or empty-alias wikilink.
          const labelFrom = targetNode.to + 1; // just past the `|`
          const hasAlias =
            closeMark != null &&
            state.doc.sliceString(targetNode.to, labelFrom) === '|' &&
            labelFrom < closeMark.from;
          if (hasAlias && closeMark) {
            results.push({ target, from: labelFrom, to: closeMark.from, nodeFrom: node.from, nodeTo: node.to });
          } else {
            results.push({ target, from: targetNode.from, to: targetNode.to, nodeFrom: node.from, nodeTo: node.to });
          }
        }
        return false; // stop descending
      }

      // Continue descent for all other node types.
    },
  });

  return results;
}
