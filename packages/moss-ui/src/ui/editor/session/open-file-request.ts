/**
 * open-file-request.ts — "open this named file for editing", asked from
 * anywhere in the app chrome.
 *
 * The notices rail is the caller today: an advisory that names a file names one
 * the author has to go and edit — a missing image reference, a page that failed
 * to render — so the name is the way in. Reading the name and then hunting for
 * it in the tree is work the app can do.
 *
 * WHY IT LIVES HERE. `editor/session/` is the target home for the editor's
 * open/load/save lifecycle — `doc-session.ts` plus `node-class.ts` beside it.
 * Opening a named
 * file is that concern, so it lands in the
 * target location now rather than as one more file in `editor/`'s 90-odd, or as
 * another method on `app.ts`. The slot transition is NOT owned here: it routes
 * through the existing ActionPanelManager, which is the single-slot logic the
 * same rule says new open/close code must sit beside.
 *
 * Structural over the panel and the view (see the interfaces below) rather than
 * importing them: the resolving is the part worth testing, and a test should not
 * have to build a webview to reach it.
 */

import { isAbsolutePath } from '../path-domain';

/** The one thing this needs from the action panel. */
export interface PanelSlot {
  getCurrentView(): unknown;
  switchTo(view: unknown): void;
}

/** The one thing this needs from the editor panel view. */
export interface EditorSlot {
  setNode(node: {
    sourcePath: string;
    isRoot: boolean;
    takeover: null;
  }): void;
  mount(): void;
}

/**
 * The absolute path the editor wants, from the path an advisory carries.
 *
 * Advisories name files relative to the site root, because that is what an
 * author recognizes. Already-absolute paths (POSIX or Windows) pass through, and
 * so does a relative path with no open folder to resolve it against — better a
 * path the editor rejects than a silently wrong one built from nothing.
 */
export function resolveAgainstFolder(filePath: string, folder: string | null): string {
  if (isAbsolutePath(filePath) || !folder) return filePath;
  return `${folder.replace(/\/+$/, '')}/${filePath}`;
}

/**
 * Show `filePath` in the editor panel, opening the panel if it is closed.
 *
 * `switchTo` is a no-op when the editor is ALREADY the active view (same
 * instance), which would leave the previously-open file on screen — so an
 * already-open panel is re-mounted instead. Mounting navigates the live webview
 * rather than recreating it, which is why that is cheap.
 */
export function openFileInEditor(
  panel: PanelSlot,
  editor: EditorSlot,
  filePath: string,
  folder: string | null,
): void {
  if (!filePath) return;

  editor.setNode({
    sourcePath: resolveAgainstFolder(filePath, folder),
    isRoot: false,
    takeover: null,
  });

  if (panel.getCurrentView() === editor) editor.mount();
  else panel.switchTo(editor);
}
