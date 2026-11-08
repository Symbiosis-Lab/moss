// Pure state logic for the breadcrumb tree navigation.
// No DOM, no I/O, no imports from bindings or tauri.
// All functions operate on string paths and plain data structures.

import type { EntryId } from '../entry-registry';

/** Completed-build evidence keyed by physical source; folders aggregate descendants. */
export class PreflightEvidence {
  private readonly occurrences = new Map<string, number>();

  replace(sourcePaths: Iterable<string>): void {
    this.occurrences.clear();
    for (const path of sourcePaths) {
      this.occurrences.set(path, (this.occurrences.get(path) ?? 0) + 1);
    }
  }

  count(path: string): number {
    let count = 0;
    const prefix = path ? `${path}/` : '';
    for (const [source, amount] of this.occurrences) {
      if (source === path || source.startsWith(prefix)) count += amount;
    }
    return count;
  }
}

/**
 * Index in `activePath` of the deepest FOLDER segment — the breadcrumb segment
 * whose chevron reveals the tree body. -1 when the path holds no folder at all.
 */
export function deepestFolderIndex(
  activePath: string[],
  isDirectory: (p: string) => boolean,
): number {
  const last = activePath.length - 1;
  if (last < 0) return -1;
  return isDirectory(activePath[last]) ? last : last - 1;
}

/**
 * Per-folder expansion override. Single source of truth for "the user has
 * explicitly opened or closed this folder, regardless of the snap-state
 * baseline." Replaces the older dual sets `closedNodes: Set<string>` +
 * `openedNodes: Map<string, string>` whose duality reflected commit history
 * rather than design intent.
 *
 * Pattern: Zed project_panel uses `expanded_dir_ids: BTreeSet<ProjectEntryId>`
 * as its single override set; we mirror that one-set-of-truth discipline —
 * including the key. Keyed by `EntryId`: a rename changes a
 * folder's path and keeps its id, so an intent recorded against the id
 * survives the rename with no re-keying, and the remap this file used to
 * carry for `overrides` is gone.
 */
export type Overrides = Map<EntryId, 'open' | 'closed'>;

/** Functional update to overrides. Returns a new map; original untouched. */
export function applyOverride(
  overrides: Overrides,
  id: EntryId,
  intent: 'open' | 'closed' | 'default',
): Overrides {
  const next = new Map(overrides);
  if (intent === 'default') next.delete(id);
  else next.set(id, intent);
  return next;
}

/**
 * Build the active path from project root to a file.
 * Returns an array of cumulative path segments, each in the registry domain
 * (project-root-relative, forward-slash-delimited). The root segment is the
 * empty string `''` (PROJECT_ROOT_REL). For example:
 *   `buildActivePath('works/post.md')` →
 *   `['', 'works', 'works/post.md']`
 *
 * This function speaks the
 * same domain as the registry and DOM data-path attributes, so callers need not
 * thread `projectRoot` through.
 */
export function buildActivePath(filePath: string): string[] {
  if (filePath === '') return [''];
  const parts = filePath.split('/').filter(Boolean);
  const result: string[] = [''];
  let current = '';
  for (const part of parts) {
    current = current === '' ? part : current + '/' + part;
    result.push(current);
  }
  return result;
}

/**
 * Keep the breadcrumb (`activePath`) VALID after a rename/move WITHOUT
 * re-anchoring it to an unrelated file. A rename changes a path string, so if
 * the breadcrumb anchor's LEAF is the renamed path — or a descendant of a
 * renamed folder — its cumulative segments are now stale and must be rebuilt
 * from the leaf's new path. If the renamed path is NOT on the anchor, the
 * anchor is left frozen (returned by reference) — the renamed file lives
 * elsewhere in the tree, so the breadcrumb should not move.
 * Re-anchoring on the open file instead would yank the breadcrumb onto it,
 * and under the frozen model the open file can differ from the anchor.
 *
 * `oldPath`/`newPath` are registry-domain (project-root-relative). Matches on a
 * path boundary (`oldPath` exactly, or the `oldPath + '/'` prefix) so
 * `post` never matches `posts/...`.
 */
export function remapActivePathForRename(
  activePath: string[],
  oldPath: string,
  newPath: string,
): string[] {
  if (activePath.length === 0) return activePath;
  const leaf = activePath[activePath.length - 1];
  let newLeaf: string | null = null;
  if (leaf === oldPath) newLeaf = newPath;
  else if (leaf.startsWith(oldPath + '/')) newLeaf = newPath + leaf.slice(oldPath.length);
  if (newLeaf === null) return activePath; // anchor unaffected → frozen
  return buildActivePath(newLeaf);
}

// ── The view record ───────────────────────────────────────────────────
//
// What the tree shows, as one value: the breadcrumb `anchor` (the active
// path, root first), the open forest `overrides`, and the sash `park` — the
// height the user last set, or null for content-fit. The controller holds
// exactly one of these and writes it only through `update(fn)`, so the
// "one write path" is a type, not a comment: every transition below is a pure
// `(view, …) => TreeView` and the three fields can no longer disagree.

export type TreeView = {
  anchor: string[];
  overrides: Overrides;
  park: number | null;
};

/** A fresh session: nothing open, nothing parked, the anchor the caller names. */
export function initialView(anchor: string[] = []): TreeView {
  return { anchor, overrides: new Map(), park: null };
}

/** Warm reopen: forget the forest and the park, keep the anchor. */
export function collapseAll(view: TreeView): TreeView {
  return { ...view, overrides: new Map(), park: null };
}

/** One folder's intent, by id. */
export function setFolder(
  view: TreeView,
  id: EntryId,
  intent: 'open' | 'closed' | 'default',
): TreeView {
  return { ...view, overrides: applyOverride(view.overrides, id, intent) };
}

/**
 * Open every folder in `ids` that is not already open. Returns `view` itself
 * when nothing changed, so a caller can tell "opened something" by identity.
 * The breadcrumb toggle, the create session and the `*` key all open a chain
 * or a set of folders at once; this is their one loop.
 */
export function openFolders(view: TreeView, ids: EntryId[]): TreeView {
  let next = view.overrides;
  for (const id of ids) {
    if (next.get(id) !== 'open') next = applyOverride(next, id, 'open');
  }
  return next === view.overrides ? view : { ...view, overrides: next };
}

/** Keep only the overrides `keep` admits — the refresh prune. */
export function prune(view: TreeView, keep: (id: EntryId) => boolean): TreeView {
  return { ...view, overrides: new Map([...view.overrides].filter(([id]) => keep(id))) };
}

/** Drop the overrides of a removed subtree — delete, the `.moss` toggle, a discarded temp row. */
export function forgetSubtree(view: TreeView, gone: Set<EntryId>): TreeView {
  return prune(view, (id) => !gone.has(id));
}

/** Re-point the breadcrumb at `path`; the forest and the park stay as they are. */
export function anchorAt(view: TreeView, path: string): TreeView {
  return withAnchor(view, buildActivePath(path));
}

/** The anchor as given — for the writers that derive it themselves (pop a segment, remap a rename, append a temp row). */
export function withAnchor(view: TreeView, anchor: string[]): TreeView {
  return { ...view, anchor };
}

/**
 * Keep the breadcrumb VALID after a batch move WITHOUT moving it
 * Folds remapActivePathForRename over every moved pair: the
 * anchor is rewritten only if one of them IS the anchor leaf (or an ancestor
 * folder of it) — in which case it was genuinely relocated, so the breadcrumb
 * follows; otherwise it stays frozen. Shared by the drag drop, the "Move
 * to…" dialog and rename/move Undo so all of them behave identically.
 *
 * Takes the move's own `old_path`/`new_path` pairs (the backend's
 * `RenameRefsSummary.moved`, or the same pairs reversed for an undo) rather
 * than a target directory to re-derive a destination from: `move_entries`
 * can collision-suffix a destination (`name-2.md`), so re-deriving it here
 * from just the basename and a target dir — the shape this function had
 * before `RenameRefsSummary` existed — could point the breadcrumb at a
 * sibling name that was never real.
 */
export function reanchorAfterMove(
  view: TreeView,
  moved: readonly { old_path: string; new_path: string }[],
): TreeView {
  let anchor = view.anchor;
  for (const m of moved) {
    anchor = remapActivePathForRename(anchor, m.old_path, m.new_path);
  }
  return withAnchor(view, anchor);
}

/** Park the sash at `park`, or `null` for content-fit. */
export function parkAt(view: TreeView, park: number | null): TreeView {
  return { ...view, park };
}

// ── Home detection ────────────────────────────────────────────────────
//
// There is deliberately NO home predicate here. The backend elects each
// folder's home as `TreeNode.is_home` (marker › index › self-named › alpha,
// real root basename) and the file tree consumes `entries.find(e => e.is_home)`
// — see FileTree.homeEntry. A filename mirror of `moss_core::home` on this side
// would drift from the backend's rules, as it cannot see the `home: true`
// marker.
