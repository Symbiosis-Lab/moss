// jsdom does not implement PointerEvent. Synthesize one based on MouseEvent
// + pointerId so handlers that destructure `e.pointerId` work. Used by
// file-tree drag tests; idempotent (safe to call from multiple test files).
//
// Reference: https://github.com/jsdom/jsdom/issues/2527 (open since 2019).
export function installPointerEventPolyfill(): void {
  type PointerInit = MouseEventInit & { pointerId?: number };
  if (typeof globalThis.PointerEvent === 'undefined') {
    // @ts-expect-error — polyfilling a missing global
    globalThis.PointerEvent = class PointerEvent extends MouseEvent {
      pointerId: number;
      constructor(type: string, init: PointerInit = {}) {
        super(type, init);
        this.pointerId = init.pointerId ?? 1;
      }
    };
  }
}

/**
 * Find a tree-row .seg by its data-path attribute. Folders in the tree body
 * carry data-path but NOT data-full-name (that's a file-only attribute set in
 * tree/render.ts), so path-keyed lookup is the reliable way for
 * tests to grab a folder row.
 */
export function findRowByPath(
  container: HTMLElement,
  path: string,
): HTMLElement | undefined {
  return Array.from(
    container.querySelectorAll<HTMLElement>('.seg:not(.bc):not(.leaf)'),
  ).find(el => el.dataset.path === path);
}

/**
 * jsdom HTMLElement doesn't implement Pointer Capture (setPointerCapture /
 * hasPointerCapture). Tests that exercise drag handlers need these stubbed so
 * the controller's `handle.setPointerCapture(e.pointerId)` doesn't throw and
 * subsequent `pointermove` handlers see `hasPointerCapture(id) === true`.
 *
 * Stubs a single boolean: once setPointerCapture is called with any id,
 * hasPointerCapture returns true for any id until releasePointerCapture.
 */
export function stubPointerCapture(el: HTMLElement): void {
  let captured = false;
  el.setPointerCapture = (_id: number) => { captured = true; };
  el.releasePointerCapture = (_id: number) => { captured = false; };
  el.hasPointerCapture = (_id: number) => captured;
}

/**
 * jsdom does not implement CSS.escape, which these tests use to build
 * attribute selectors (e.g. `.seg[data-path="..."]`). Install a spec-accurate
 * implementation (the CSSOM serialize-an-identifier algorithm) so selector-based lookups in
 * the file-tree tests escape the same characters the browser would. Idempotent.
 * Reference: https://drafts.csswg.org/cssom/#serialize-an-identifier
 * (mirrors the MDN / mathiasbynens CSS.escape polyfill).
 */
export function installCSSEscapePolyfill(): void {
  if (typeof globalThis.CSS !== 'undefined' && typeof globalThis.CSS.escape === 'function') {
    return;
  }
  const escape = (value: string): string => {
    const str = String(value);
    const length = str.length;
    let result = '';
    let index = -1;
    const firstCodeUnit = str.charCodeAt(0);
    while (++index < length) {
      const codeUnit = str.charCodeAt(index);
      // NULL (U+0000) → U+FFFD REPLACEMENT CHARACTER
      if (codeUnit === 0x0000) { result += '�'; continue; }
      if (
        // Control chars (U+0001–U+001F, U+007F)
        (codeUnit >= 0x0001 && codeUnit <= 0x001f) || codeUnit === 0x007f ||
        // Leading digit (U+0030–U+0039)
        (index === 0 && codeUnit >= 0x0030 && codeUnit <= 0x0039) ||
        // Second char is a digit and the first char is '-'
        (index === 1 && codeUnit >= 0x0030 && codeUnit <= 0x0039 && firstCodeUnit === 0x002d)
      ) {
        result += '\\' + codeUnit.toString(16) + ' ';
        continue;
      }
      // A single leading '-' must be escaped.
      if (index === 0 && length === 1 && codeUnit === 0x002d) { result += '\\' + str.charAt(index); continue; }
      if (
        codeUnit >= 0x0080 || codeUnit === 0x002d || codeUnit === 0x005f ||
        (codeUnit >= 0x0030 && codeUnit <= 0x0039) ||
        (codeUnit >= 0x0041 && codeUnit <= 0x005a) ||
        (codeUnit >= 0x0061 && codeUnit <= 0x007a)
      ) {
        result += str.charAt(index);
        continue;
      }
      result += '\\' + str.charAt(index);
    }
    return result;
  };
  const existing = (globalThis as unknown as { CSS?: { escape?: (s: string) => string } }).CSS;
  if (existing) {
    existing.escape = escape;
  } else {
    (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = { escape };
  }
}

import { openFolders, type TreeView } from '../model';
import type { TreeNode } from '@symbiosis-lab/moss-ui';
import type { TreeRender } from '../render';
import { PROJECT_ROOT_REL, baseName } from '../../path-domain';
import type { EntryId } from '../../entry-registry';

/**
 * A `TreeNode` the way `list_tree` returns one: every field present, `null`
 * where the backend serializes `None`. Tests used to hand-build these
 * literals and leave `file_id` out, so `undefined` reached the
 * registry as a shared identity key. Typing the fixture as `TreeNode` is
 * what closes that boundary; the builder is what makes it bearable. `name`
 * is the path's last segment unless `extra` says otherwise (the root's
 * display name is whatever the vault folder is called).
 */
export function file(path: string, extra: Partial<TreeNode> = {}): TreeNode {
  return {
    name: baseName(path), path, is_dir: false, modified: null, publish_date: null,
    date_source: 'none', is_home: false, file_id: null, children: null, hidden: 0, ...extra,
  };
}

export function dir(path: string, children: TreeNode[], extra: Partial<TreeNode> = {}): TreeNode {
  return { ...file(path), is_dir: true, children, ...extra };
}

/** The slice of the registry these helpers need to go from a path to an id. */
type PathToId = { getByPath(path: string): EntryId | null };

/**
 * `overrides` read by PATH. The map is EntryId-keyed; tests think
 * in paths, so this is the one place the translation lives.
 */
export function overrideAt(ft: unknown, path: string): string | undefined {
  const p = priv<{ view: TreeView; registry: PathToId }>(ft);
  const id = p.registry.getByPath(path);
  return id === null ? undefined : p.view.overrides.get(id);
}

/**
 * Put the controller's `view` into a state a test wants to start from — the
 * same one write path production uses (`update`), so a test never mutates the
 * live record in place.
 */
export function setView(ft: unknown, patch: Partial<TreeView>): void {
  priv<{ update(fn: (v: TreeView) => TreeView): void }>(ft).update(v => ({ ...v, ...patch }));
}

/** The EntryId at `path`, for tests that reach into the id-keyed `dirIndex`. */
export function idOf(ft: unknown, path: string): EntryId {
  const id = priv<{ registry: PathToId }>(ft).registry.getByPath(path);
  if (id === null) throw new Error(`[test-helpers] no registry id for '${path}'`);
  return id;
}

/**
 * A folder's child paths in display order, read through the id-keyed
 * `dirIndex`; null when the index has no entry for it.
 */
export function childPaths(ft: unknown, folderPath: string): string[] | null {
  const p = priv<{ dirIndex: Map<EntryId, EntryId[]>; registry: PathToId & { getById(id: EntryId): { path: string } | null } }>(ft);
  const id = p.registry.getByPath(folderPath);
  const ids = id === null ? undefined : p.dirIndex.get(id);
  return ids === undefined ? null : ids.map(c => p.registry.getById(c)!.path);
}

/**
 * Test-only replacement for the deleted `FileTree.enterDirMode()` (dead
 * production code):
 * open the project root plus every ancestor FOLDER of the active path so
 * sibling rows render. Hundreds of tests use this as an expansion harness;
 * production reaches the same state through user toggles, so the shortcut
 * lives here rather than on the class where it could be wired by accident.
 *
 * One `openFolders` transition through `update` — never an
 * in-place `.set()` on the live map.
 */
export function enterDirMode(ft: unknown): void {
  const p = priv<{
    view: TreeView;
    update(fn: (v: TreeView) => TreeView): void;
    registry: PathToId;
    isDirectory(path: string): boolean;
    render: Pick<TreeRender, 'render' | 'applyWrapperHeight'>;
  }>(ft);
  const folders = [PROJECT_ROOT_REL, ...p.view.anchor.filter(path => path !== PROJECT_ROOT_REL && p.isDirectory(path))];
  const ids = folders.map(path => p.registry.getByPath(path)).filter((id): id is EntryId => id !== null);
  p.update(v => openFolders(v, ids));
  p.render.render();
  p.render.applyWrapperHeight();
}

/**
 * Typed accessor for the FileTree controller's private fields/methods in tests.
 * Replaces the repeated `(ft as unknown as { x }).x` double-casts across the
 * file-tree test suite. The shape lists only the members the tests touch; extend
 * it as more private members are exercised.
 */
export interface FileTreePrivate {
  view: TreeView;
  update(fn: (v: TreeView) => TreeView): void;
  toggleFolderOpen(path: string): void;
  handleToggleTop(index: number): void;
  /** The renderer (tree/render.ts): the rebuild, the row count and the height write live there. */
  render: TreeRender;
  fillOrder(): string[];
  parkAtMinHeight(): void;
}

export function priv<T = FileTreePrivate>(ft: unknown): T {
  return ft as T;
}

/**
 * Start an inline rename on `row` the way a user does: select the row, then
 * press the rename key.
 *
 * Double-click used to be a third way in, and the suites leaned on it because
 * it was the shortest to write. Double-click opens,
 * as it does in Finder, Explorer, VS Code and every other tree whose items are
 * openable — so the two real entry points are this key and the context menu.
 * Tests that want a rename should go through one of them rather than reaching
 * for `startRename` directly, or they stop proving the gesture reaches it.
 *
 * The rename key rather than Enter only because it is unambiguous in a test; the handler
 * is the same for both.
 */
export function startRenameByKey(row: HTMLElement): void {
  row.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', bubbles: true }));
}
