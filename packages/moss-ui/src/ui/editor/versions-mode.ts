/**
 * Versions mode — the editor body under the heading becomes the version list.
 * A mode of the editor, not a place of its own: module state is
 * per-webview, so the tree's call — made from the editor webview — cannot go
 * through the main window's panel manager.
 *
 * The tree and create-file chrome do not apply once you're not editing a
 * file: `open()` calls `host.hideChrome()` (the same nav-surface/heading hide
 * the email composer uses) and `unmount()` calls `host.showChrome()` back,
 * since leaving this mode never reloads the page, so nothing else would undo
 * it. Scope has three values — 'page', 'folder', 'site' — and the folder's
 * path travels all the way to the backend; see the scope note on
 * `VersionsSurfaceContext` in versions-surface.ts.
 *
 * The breadcrumb strip stays up: `host.setTitleBar`
 * is passed straight through as `ctx.setTitleBar`, so the SURFACE builds and
 * owns every screen's title-bar content (back link, subject, trailing
 * actions). `unmount()` releases the strip (`setTitleBar(null)`) alongside
 * `showChrome()`.
 *
 * editor-main.ts owns the slot and the load paths; this module owns only
 * the mode's state (the live surface and where it returns to) and reaches
 * the slot through `host`. Every exit — "‹ Editor", a completed restore, and
 * any other tree entry being opened — runs through `unmount()`, which
 * editor-main calls from its two surface-switch teardowns.
 */

import { renderVersionsSurface, type VersionsSurfaceHandle, type VersionsRestored } from './versions-surface';
import { engine } from '../engine';
import { showAck } from "../shared/ui/toast/toast-manager";
import { runAction } from "../shared/ui/run-action";
import { t } from "../i18n/index";
import { uiEvent } from "../log";
import { baseName } from './path-domain';
import type { EditorSurface } from './session/node-class';
import { createGenerationToken } from "../shared/utils/generation-token";

type PaneSurface = Exclude<EditorSurface, { kind: 'content' }>;

export interface VersionsModeHost {
  container(): HTMLElement | null;
  projectRoot(): string | null;
  rootName(): string | null;
  currentFilePath(): string | null;
  /** The last non-content surface mounted — what a pane-origin exit restores. */
  lastSurface(): PaneSurface | null;
  flushPendingSave(): Promise<unknown>;
  /** Empty the slot the way applyEditorSurface does before it mounts. */
  clearSlot(cm: HTMLElement): void;
  /** Hide the tree, the create button and the window caption — none of it is
   *  relevant once the body isn't showing a file. The breadcrumb strip is
   *  NOT part of this hide: it stays up and `setTitleBar` below
   *  replaces it wholesale. */
  hideChrome(): void;
  /** Undo `hideChrome()`. Versions mode never navigates away, so nothing
   *  else restores this on exit. */
  showChrome(): void;
  /** Replace the breadcrumb with `content` (the title bar is a navigation
   *  stack) — `null` restores it. Passed straight through as
   *  `ctx.setTitleBar` so the surface, not this module, builds every
   *  screen's title-bar content. Reaches `FileTree.setTitleBar` through the
   *  host so this module never touches tree/ DOM directly. */
  setTitleBar(content: HTMLElement | null): void;
  /** The ordinary in-webview document load a tree click takes. */
  loadFile(path: string): Promise<unknown>;
  applyEditorSurface(surface: PaneSurface): void;
  resolveRootSurface(): Promise<void>;
  logInfo(message: string): void;
}

export interface VersionsMode {
  /** Enter the mode for `path`: a file in 'page' scope, a subfolder in
   *  'folder' scope, or the project root in 'site' scope (path is then
   *  PROJECT_ROOT_REL, ''). Re-entering from another row replaces the list
   *  but keeps the ORIGINAL origin. */
  open(scope: 'page' | 'folder' | 'site', path: string, isRoot: boolean): Promise<void>;
  /** Tear the surface down if it is up; safe to call when it is not. */
  unmount(): void;
  isActive(): boolean;
}

export function createVersionsMode(host: VersionsModeHost): VersionsMode {
  let handle: VersionsSurfaceHandle | null = null;
  /** Where "‹ Editor" and a completed restore return to: the document that
   *  was open when the mode began, or the pane that stood in the slot. */
  let origin: { filePath: string | null; surface: PaneSurface | null } | null = null;
  /** Captured at the top of every `open()`; a call superseded by a later one
   *  bails right after its `flushPendingSave` await, before touching `handle`
   *  or the DOM. Needed because two `flushPendingSave` calls can settle out
   *  of order: the one that triggers the write awaits the outer
   *  `saveActiveDocument()` promise, while one that only sees a write already
   *  in flight awaits the inner `saveInFlight` promise, which resolves a
   *  microtask earlier (editor-main.ts's `flushPendingSave`) — so a SECOND
   *  `open()` started while the first is still flushing can mount before the
   *  first does, and the first then clobbers it with the stale scope/path. */
  const openGeneration = createGenerationToken();

  function unmount(): void {
    if (!handle) return;
    handle.destroy();
    handle = null;
    origin = null;
    host.setTitleBar(null);
    host.showChrome();
    const cm = host.container();
    if (cm?.getAttribute('data-editor-surface') === 'versions') {
      cm.innerHTML = '';
      cm.removeAttribute('data-editor-surface');
      cm.removeAttribute('data-editor-surface-key');
    }
  }

  /** Leave the way the author came in: reopen the document that was open, or
   *  put back the pane that stood there, or — with nothing remembered —
   *  resolve the root the way boot does. A restore shows its Undo toast
   *  here, in this webview, once the source is back on screen. */
  async function exit(restored?: VersionsRestored): Promise<void> {
    const from = origin;
    if (from?.filePath) {
      // loadFile tears the surface down itself once the file's bytes are in
      // hand, so a failed read leaves the list standing, not a white pane.
      await host.loadFile(from.filePath);
    } else {
      unmount();
      if (from?.surface) host.applyEditorSurface(from.surface);
      else await host.resolveRootSurface();
    }
    if (!restored) return;
    showAck(restored.message, {
      actions: restored.undoId
        ? [{
            label: t('versions.undo'),
            // Same contract every other restore trigger in the surface uses
            // (run-action.ts): a failed Result or a rejection ends in a toast.
            onClick: () => void runAction({
              key: 'versions-undo',
              run: () => engine.restoreVersion(restored.undoId!, restored.path, 'in_place'),
            }),
          }]
        : undefined,
    });
  }

  async function open(scope: 'page' | 'folder' | 'site', path: string, isRoot: boolean): Promise<void> {
    uiEvent('versions.open', { scope, target: baseName(path) || path });
    const cm = host.container();
    if (!cm || !host.projectRoot()) {
      console.error('[versions-mode] missing #moss-editor-cm or projectRoot');
      return;
    }
    const gen = openGeneration.bump();
    // So the file on disk is current before Versions reads it.
    await host.flushPendingSave();
    // A later open() (a second right-click while this flush was still
    // in flight) has already claimed the slot — mounting now would clobber
    // it with this call's stale scope/path.
    if (openGeneration.isStale(gen)) return;
    if (!origin) {
      // A document leaves no `data-editor-surface` marker; a pane leaves its
      // kind. The registry's active entry can outlive a pane (nothing clears
      // it on applyEditorSurface), so the DOM marker decides what we return to.
      origin = cm.hasAttribute('data-editor-surface')
        ? { filePath: null, surface: host.lastSurface() }
        : { filePath: host.currentFilePath(), surface: null };
    }
    host.clearSlot(cm);
    host.hideChrome();
    handle?.destroy();
    cm.innerHTML = '';
    // The surface mounts into a child of its own so the container's class
    // and the `data-editor-surface` marker stay the container's.
    const surfaceHost = document.createElement('div');
    cm.appendChild(surfaceHost);
    cm.setAttribute('data-editor-surface', 'versions');
    cm.setAttribute('data-editor-surface-key', `${scope}:${path}`);
    // isRoot is 'site' scope's own path (PROJECT_ROOT_REL, '') asked from the
    // root menu item, so this is just "which name goes over the caption" —
    // never a second scope decision.
    const title = isRoot ? (host.rootName() ?? '') : (path.split('/').pop() || path);
    handle = renderVersionsSurface(surfaceHost, {
      scope,
      path,
      title,
      setTitleBar: (content) => host.setTitleBar(content),
      onExit: (restored) => void exit(restored),
    });
    host.logInfo(`[editor] showing non-file surface="versions" scope=${scope} (no editable CM6 document)`);
  }

  return { open, unmount, isActive: () => handle !== null };
}
