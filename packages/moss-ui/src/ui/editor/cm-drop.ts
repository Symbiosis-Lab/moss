// Internal tree→editor drop (Flow 3): insert an embed per dropped entry at
// the block-snapped drop position — `![](path)` written from the page for
// every kind (embedFor); `![[name]]` only where the backend refuses a destination.
//
// TWO entry points share insertEntryEmbedsAtCoords():
//   1. editor-main.ts's Tauri onDragDropEvent 'drop' routing — the PRODUCTION
//      path. Under `dragDropEnabled: true` Tauri's drag_drop_handler
//      intercepts the ENTIRE native drag session (including in-app row
//      drags), so DOM dragover/drop never fire; editor-main detects the
//      internal drag via FileTree.hasActiveInternalDrag() and resolves the
//      dragged EntryIds via activeInternalDragEntryIds().
//   2. dropExtension() below — the DOM-event safety net, kept in case a
//      platform/config ever lets the DOM events through. Reads the
//      `application/x-moss-entries` dataTransfer key (set by
//      FileTree.startMultiRowDrag). OS file drops (`Files` type) are NOT
//      handled here — they go through the Tauri path too
//      (handleEditorFileDrop); this handler returns false for them.
//
// The horizontal block-boundary indicator for internal drags is provided by
// blockDropCursor() (cm-os-drop-caret.ts), registered in cm-editor.ts instead
// of the built-in dropCursor(). Do NOT add another caret here.

import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import { snapToBlockBoundary } from './cm-os-drop-caret';
import { lookupAsset } from './asset-registry.generated';

/**
 * The standard-form destination for the project file `target`, written from
 * the page being edited, or null when the backend refuses it. Supplied by
 * editor-main through `asset_ref_for_path`, so the path and its encoding are
 * the engine's (`asset_ref_in`), the same text its link completion writes.
 */
export type DestinationFor = (target: string) => Promise<string | null>;

/**
 * The embed the editor inserts for the project file `target` (a folder ends
 * in `/`). The engine renders `![](path)` of a folder, or of a kind the
 * registry flags `standardEmbed` (images, video, audio, PDF, HTML, 3D, and the
 * transclusions: page, table, notebook), as the embed itself, so those take
 * that form with the path from the page. Any other file, or a destination the
 * backend refused, keeps `![[name]]`.
 */
export async function embedFor(target: string, destinationFor: DestinationFor = async () => null): Promise<string> {
  const isFolder = target.endsWith('/');
  if (isFolder || lookupAsset(target)?.standardEmbed) {
    const dest = await destinationFor(target);
    if (dest) return `![](${dest})`;
  }
  return isFolder ? `![[${target.slice(0, -1).split('/').pop()}/]]` : `![[${target.split('/').pop()}]]`;
}

export interface DropOptions {
  /**
   * Map dragged entry IDs to project-relative paths (a folder's ends with
   * `/`). IDs that cannot be resolved are filtered out. Provided by
   * editor-main.ts reading the FileTree's registry.
   */
  resolveEntries: (entryIds: number[]) => string[];
  destinationFor: DestinationFor;
}

/**
 * Insert one embed per target (embedFor) at the block-snapped position nearest
 * the (x, y) client coords. THE shared insert for both tree→editor drop
 * paths: the DOM drop handler below (safety net) and editor-main.ts's Tauri
 * onDragDropEvent 'drop' routing (the PRODUCTION path — under
 * `dragDropEnabled: true` Tauri's drag_drop_handler intercepts the native
 * drag session at the webview level, so the DOM handler never fires; see
 * hasActiveInternalDrag's doc in file-tree.ts).
 *
 * `targets` are project-relative paths (FileTree.resolveDropEntries), a
 * folder's ending in `/`; empty/falsy entries are filtered out. Resolves true
 * when an insert was dispatched, false when there was nothing to insert.
 *
 * Save-flush note: persisting the edit (flushPendingSave) is the CALLER's
 * job — this module cannot import editor-main (circular), and only the
 * Tauri-driven callers know a completed drop gesture just happened and
 * should hit disk immediately rather than wait out the autosave debounce.
 */
export async function insertEntryEmbedsAtCoords(
  view: EditorView,
  targets: string[],
  destinationFor: DestinationFor,
  x: number,
  y: number,
): Promise<boolean> {
  const filtered = targets.filter(Boolean);
  if (filtered.length === 0) return false;

  // Resolve the drop position. Fall back to document end when the coords
  // are outside the editor content area (matches OS-drop behavior in
  // handleEditorFileDrop). Block-snap so the embed always lands
  // on its own line between paragraphs rather than mid-word — matching the
  // horizontal indicator position.
  const rawAt = view.posAtCoords({ x, y }) ?? null;
  const at = rawAt != null
    ? snapToBlockBoundary(view, rawAt, y)
    : view.state.doc.length;

  // The position is read at drop time; the destinations are asked for after.
  const embeds = await Promise.all(filtered.map((t) => embedFor(t, destinationFor)));
  view.dispatch({
    changes: { from: Math.min(at, view.state.doc.length), insert: embeds.join('\n') + '\n' },
    userEvent: 'input.drop',
  });
  return true;
}

/**
 * CM6 extension that handles `application/x-moss-entries` drag-drop events
 * and inserts an embed per entry at the drop position.
 *
 * Wire this inside the `if (isMarkdown)` block in cm-editor.ts, passing a
 * `resolveEntries` callback that maps entry IDs → project-relative paths via
 * the FileTree registry.
 */
export function dropExtension(opts: DropOptions): Extension {
  return EditorView.domEventHandlers({
    dragover(event) {
      // Allow drop for our internal moss-entries payload so the browser
      // shows a "copy/move" cursor rather than the forbidden cursor.
      // OS file drags never reach here under dragDropEnabled:true.
      if (event.dataTransfer?.types.includes('application/x-moss-entries')) {
        event.preventDefault();
        return true; // handled (prevents default "not allowed" effect)
      }
      return false;
    },

    drop(event, view) {
      const types = event.dataTransfer?.types ?? [];

      // OS file drops are handled by the Tauri onDragDropEvent path and are
      // unreachable in practice under dragDropEnabled:true. Return false as a
      // safety net to avoid double-handling if that ever changes.
      if (types.includes('Files')) return false;

      // Not our payload — let CM6 handle it (e.g. text drag).
      if (!types.includes('application/x-moss-entries')) return false;

      const raw = event.dataTransfer!.getData('application/x-moss-entries');
      let ids: number[] = [];
      try {
        ids = (JSON.parse(raw) as { entryIds?: unknown }).entryIds as number[] ?? [];
      } catch {
        return false;
      }
      if (!Array.isArray(ids) || ids.length === 0) return false;

      const names = opts.resolveEntries(ids).filter(Boolean);
      if (names.length === 0) return false;

      event.preventDefault();

      void insertEntryEmbedsAtCoords(
        view,
        names,
        opts.destinationFor,
        (event as DragEvent).clientX,
        (event as DragEvent).clientY,
      );

      return true;
    },
  });
}
