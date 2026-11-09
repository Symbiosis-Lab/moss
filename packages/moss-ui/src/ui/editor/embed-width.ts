// packages/moss-ui/src/ui/editor/embed-width.ts — `applyEmbedWidth`, split out of
// cm-editor.ts so the reference card's width-preset actions
// (cm-reference-card.ts) can call it without a circular import:
// cm-editor.ts already imports FROM cm-reference-card.ts (the card
// providers/facet), so cm-reference-card.ts importing it back from
// cm-editor.ts would close a dependency cycle.
// This module is the shared leaf both sides import instead.
//
// `createApplyEmbedWidth` takes the `setImageWidth` Tauri command as a
// PARAMETER so this module does not depend on a particular host transport.
// `cm-editor.ts` builds the instance from the engine command and is the
// only caller —
// cm-reference-card.ts never imports this module at all; cm-editor.ts
// passes the built function down to it as an option instead.

import type { EditorView } from '@codemirror/view';
import { embedNodeAt } from '@symbiosis-lab/moss-syntax/cm6';

export type SetImageWidthFn = (md: string, width: string | null) => Promise<string>;
export type ApplyEmbedWidthFn = (view: EditorView, storedFrom: number, width: string | null) => Promise<void>;

/**
 * Build `applyEmbedWidth`: rewrite the width token of the embed (or image)
 * whose source span starts at (or near) `storedFrom`, then dispatch a PLAIN
 * user edit. Invoked by the live-preview drag-resize handle (`width =
 * "NN%"`), double-click reset (`width = null`), and the reference card's
 * width-preset actions. Handles ANY `![[…]]` embed — folder, PDF, video,
 * iframe — as well as plain `![alt](url)` images: `embedNodeAt` resolves
 * both node kinds and moss-core's `set_image_width` round-trips
 * them uniformly.
 *
 * The caller resolves `storedFrom` fresh at gesture time (e.g.
 * `imageNodeAtWidget` via `view.posAtDOM`, or an embed's own `.from`), so it
 * is current when this runs; the `embedNodeAt` climb below re-anchors it
 * against the live tree as belt-and-braces (and again after the IPC await).
 * The width rewrite is owned by moss-core (`set_image_width`, via
 * `setImageWidth`) so the produced markdown round-trips with the build's
 * parser.
 *
 * Dispatch policy (deliberate): this is a normal user edit — it MUST flow
 * through onChange/auto-save and undo history. So it is NOT tagged
 * `externalUpdate` and does NOT route through `applyExternal()` (both of which
 * suppress auto-save and are reserved for external/file-reload updates).
 */
export function createApplyEmbedWidth(setImageWidth: SetImageWidthFn): ApplyEmbedWidthFn {
  return async function applyEmbedWidth(view, storedFrom, width) {
    // Re-resolve the live embed node around the stored position. `embedNodeAt`
    // owns the resolveInner-and-climb so this handles `![[…]]` (WikilinkEmbed)
    // as well as `![alt](url)` (Image) — climbing for `'Image'` alone here
    // would silently no-op every wikilink and folder resize.
    const node = embedNodeAt(view.state, storedFrom);
    if (!node) return;
    const from = node.from;
    const to = node.to;
    const md = view.state.doc.sliceString(from, to);
    const next = await setImageWidth(md, width);
    if (next === md) return;
    // Re-resolve AFTER the await — the doc may have changed during the IPC hop
    // (concurrent keystroke or an external file-reload can shift from/to or
    // mutate the node's content, making the stale span coordinates wrong).
    const n2 = embedNodeAt(view.state, from);
    if (!n2 || view.state.doc.sliceString(n2.from, n2.to) !== md) return; // stale → bail
    // Place the cursor on the image line so live-preview reveals the source with
    // the committed |NN% (or the bare embed after a reset). Releasing the handle
    // reveals the source line and its updated value as resize confirmation.
    view.dispatch({
      changes: { from: n2.from, to: n2.to, insert: next },
      selection: { anchor: n2.from },
    });
  };
}
