// The spine body — the single source of "what the tree body renders". Pure: no
// DOM, no I/O.
//
// THE INTEGRATED-SPINE INVARIANT: every node appears EXACTLY once. A node ON
// the active path renders in the BREADCRUMB (the horizontal spine); a node
// NOT on the active path renders in the BODY (iff its parent folder is
// open). Never both. So this body EXCLUDES every active-path node.
//
// BODY ORDER: deepest-first along the active path. The body renders the
// active path's ancestors' children DEEPEST-FIRST, so the active file's
// SIBLINGS sit directly under the breadcrumb, then the level above, etc.
// (NOT root-first, which stranded an active-ancestor's children at the
// ancestor's mid-tree position.) For the active file `getting-started.md`
// with root + docs open:
//
//   site › docs › getting-started     [breadcrumb = activePath]
//     · index  · editors  …           [docs's children, depth 2]   ← i=1 (deepest)
//   · index  · AGENTS  · footer        [root's children, depth 1]   ← i=0 (root)
//
// A NON-active open folder surfaced in the body recurses normally (root-first
// within its own subtree) via walkNested.
//
// The panel height follows this row count (renderedRowCount), so it can never
// disagree with what's rendered — the coupling that caused the blank-gap /
// drag-cap bug class.
//
// ── FOLDER-VALUED LEAF ───────────────────────────────────────────────────────
//
// When the active "file" is ITSELF a directory (a generated home,
// `activePath = ['']`, or dir mode's `activePath = ['', 'docs']`), the LAST
// activePath segment counts as the deepest openable level too — its children
// render as body rows (depth = activePath.length) exactly like any other open
// ancestor. The folder itself still renders ONLY in the breadcrumb
// (integrated-spine invariant unchanged).
//
// ── STATE ROWS (empty, unavailable, hidden-count) ────────────────────────────────────────────
//
// An open level that has no rows to show renders one explicit placeholder row
// instead of silently contributing zero rows: `kind: 'empty'` when the caller's
// `getChildren` returns an empty array (a real, empty folder), `kind:
// 'unavailable'` when it returns `null` (the caller doesn't know this folder —
// e.g. a `refreshTree` that failed or never resolved), or `kind: 'hidden'`
// when the array is empty but the folder's own listing filtered something out
// (a nonzero `hidden` count — moss's own `.moss/` housekeeping, never OS/VCS
// noise; see `TreeNode.hidden` on the Rust side) — distinct from a folder
// that is genuinely empty on disk.
// A folder whose only child is the ACTIVE node is deliberately NOT a state
// row — that child renders in the breadcrumb, not nowhere, so there is
// nothing degenerate to report.
//
// `stateRowKind` is the ONE place that turns `(children, hidden)` into a row
// kind. render.ts's `renderLevelBody` is its one real caller — that's the
// path that decides what the DOM actually shows, reading the folder's real
// `hidden` count from the registry. This module's own `renderLevel` only
// counts rows for the panel-height math, where 'hidden' and 'empty' cost the
// same one row, so it has no `hidden` signal to give `stateRowKind` and never
// produces a `'hidden'` kind — its own state rows are always 'empty' or
// 'unavailable'. Both still route through the one function so the two-way
// rule itself (null vs. empty) can't drift between the two call sites.

/** The three P2/hidden-count placeholder kinds a state row can render. */
export type StateRowKind = 'empty' | 'unavailable' | 'hidden';

/**
 * Decide which state-row kind an open, childless level should render.
 *
 * `children` mirrors `GetChildren`'s return: `null` when the index has no
 * entry for this folder at all ('unavailable'), otherwise the listed
 * children (possibly empty). `hidden` is the folder's own `hidden` count —
 * ignored unless `children` is empty, since a folder with real rows to show
 * is never a state row in the first place (every caller here only reaches
 * this function once it has already established `children === null ||
 * children.length === 0`; a non-empty array has no defined meaning and is
 * never passed by either caller).
 */
export function stateRowKind(children: string[] | null, hidden: number): StateRowKind {
  if (children === null) return 'unavailable';
  if (children.length === 0) return hidden > 0 ? 'hidden' : 'empty';
  return 'empty';
}

/**
 * The hint row under a folder that has a derivation to offer or a problem to
 * report (tree/derivations.ts). A state row like the others — not a file, no
 * path identity — but not a placeholder either: it sits FIRST under its
 * folder, in front of the real children, and so it needs the one-row height
 * those children's rows get.
 */
export type HintRowKind = 'hint';

/** One rendered tree-body row: a real node, or a state placeholder. */
export interface ForestRow {
  path: string;
  depth: number;
  isDir: boolean;
  /** Present only for a state-row placeholder — see `StateRowKind`. Absent
   *  for ordinary rows. */
  kind?: StateRowKind | HintRowKind;
}

/**
 * Child paths of a folder, in display order — or `null` when the caller's
 * index has no entry for this folder at all (degenerate: e.g. a failed/
 * unresolved refresh left it out). An actually-empty folder returns `[]`,
 * which is a different, legitimate state (the state rows tell them apart).
 */
export type GetChildren = (path: string) => string[] | null;

/**
 * Compute the tree-body rows for the active path, DEEPEST-ANCESTOR FIRST.
 *
 * For each ancestor folder of the active file, from the deepest (the active
 * file's own parent — or the active file itself, when it's a directory; see
 * the folder-valued-leaf note above) down to the project root, IF that
 * ancestor is open render its NON-active children at the ancestor's child
 * depth, recursing normally into any non-active open subfolders. Every node on
 * `activePath` is EXCLUDED from the body — it lives in the breadcrumb instead
 * (the integrated-spine invariant).
 *
 * A root-level active file (`activePath = ['', 'index.md']`, length 2) → only
 * i=0 runs → body = root's children minus the active file.
 *
 * @param activePath  the active file's path segments (root → leaf), each in the
 *                    registry domain; e.g. ['', 'notes', 'notes/index.md']. Index
 *                    0 is '' (root); the last entry is the active file. Ancestor
 *                    folders are indices 0..length-2. These nodes render in the
 *                    breadcrumb, never the body.
 * @param isOpen      whether a folder is open — the caller's read of the
 *                    `overrides` single source (EntryId-keyed, so the
 *                    path→id resolution stays with the owner)
 * @param getChildren child paths of a folder, in display order — or `null` if
 *                    the folder is not in the index at all ('unavailable')
 * @param isDir       whether a path is a directory
 * @param hasHint     whether a folder shows a hint row as its first child
 */
export function spineBody(
  activePath: string[],
  isOpen: (path: string) => boolean,
  getChildren: GetChildren,
  isDir: (path: string) => boolean,
  hiddenCount: (path: string) => number = () => 0,
  hasHint: (path: string) => boolean = () => false,
): ForestRow[] {
  const active = new Set(activePath);
  const rows: ForestRow[] = [];

  const stateRow = (folderPath: string, depth: number, kind: StateRowKind): ForestRow => ({
    // Namespaced so a state row can never collide with a real path.
    path: `${folderPath}\u0000${kind}`,
    depth,
    isDir: false,
    kind,
  });

  /** Render one open folder's children at `depth`, or a state row (or
   *  hidden-count) if it has none. Shared by the ancestor loop (below) and
   *  nested recursion. */
  const renderLevel = (folderPath: string, depth: number, recurse: (child: string, depth: number) => void): void => {
    if (hasHint(folderPath)) {
      rows.push({ path: `${folderPath}\u0000hint`, depth, isDir: false, kind: 'hint' });
    }
    const children = getChildren(folderPath);
    if (children === null || children.length === 0) {
      // Empty folders add no child row. Count hidden/unavailable evidence
      // with the same policy the DOM renderer uses.
      const kind = stateRowKind(children, hiddenCount(folderPath));
      if (kind !== 'empty') rows.push(stateRow(folderPath, depth, kind));
      return;
    }
    // A folder whose only children are active-path nodes is NOT a state row —
    // that content renders in the breadcrumb, so there is nothing degenerate.
    const nonActive = children.filter((c) => !active.has(c));
    for (const child of nonActive) {
      const dir = isDir(child);
      rows.push({ path: child, depth, isDir: dir });
      if (dir && isOpen(child)) recurse(child, depth + 1);
    }
  };

  // Ordinary subtree of a NON-active open folder: render its children at `depth`,
  // recursing root-first into open non-active subfolders. (An active node can't
  // occur off the active path, but the guard is cheap and safe.)
  const walkNested = (folderPath: string, depth: number): void => {
    renderLevel(folderPath, depth, walkNested);
  };

  // Openable levels, DEEPEST FIRST: the active leaf itself when it's a
  // directory (folder-valued leaf), then ancestor folders 0..length-2.
  const leaf = activePath[activePath.length - 1];
  const levels: { path: string; depth: number }[] = [];
  if (activePath.length > 0 && isDir(leaf)) {
    levels.push({ path: leaf, depth: activePath.length });
  }
  for (let i = activePath.length - 2; i >= 0; i--) {
    levels.push({ path: activePath[i], depth: i + 1 });
  }

  for (const { path: folder, depth: childDepth } of levels) {
    if (!isOpen(folder)) continue; // that level shows iff its folder is open
    renderLevel(folder, childDepth, walkNested);
  }

  return rows;
}

// ── Sash arithmetic ──────────────────────────────────────────────────────────
//
// The height side of the projection: how tall the nav surface is for a row
// count, and the two clamps the sash machine reads. Pure, like spineBody.

export const ROW_H = 28;
export const ROW_GAP = 1;
export const NAV_PAD_BOTTOM = 0; // .nav-body has no bottom padding (breadcrumb row lives in sibling .nav-bc-strip; navH formula = TITLEBAR_H + rows*(ROW_H+ROW_GAP) still holds)
// Titlebar height mirrors the Rust const `TITLEBAR_HEIGHT` in
// the host window configuration, which is injected into CSS as
// `--moss-titlebar-height` when the webview is initialized.
// If that value ever changes, update this constant AND re-run vitest — a
// mismatch between nav-height math and the CSS titlebar would cause a
// one-frame layout shift at mount.
export const TITLEBAR_H = 48;
export const MIN_EDITOR_H = 144;

/** Height in pixels for the nav surface given extra tree rows beyond the breadcrumb.
 *  Breadcrumb sits in row 1 at TITLEBAR_H (48px); each extra row adds ROW_H + ROW_GAP. */
export function navH(extraRows: number): number {
  return TITLEBAR_H + extraRows * (ROW_H + ROW_GAP) + NAV_PAD_BOTTOM;
}

/**
 * The nav surface's height for `rows` rendered body rows: content-fit,
 * capped by `park` (the height the user last set — `null` for content-fit),
 * clamped between the breadcrumb-only minimum and `floor`, the most the
 * editor can spare. The minimum wins over a floor that has dropped beneath it.
 *
 *   (1) breadcrumb expand → content-fit grow  (the park is cleared first)
 *   (2) in-tree expand → keeps the parked size, the new rows scroll
 *   (3) drag-up-to-shrink stays (release parks the height it landed at)
 *   (4) collapse a folder → min(park, smaller content) shrinks below the park
 *
 * This is the height model: the wrapper is written from it, "is anything but
 * the breadcrumb showing" is `desiredHeight(...) <= navH(0)`, and the sash
 * takes its pointerdown baseline from it — nothing reads the height back out
 * of the DOM.
 */
export function desiredHeight(rows: number, park: number | null, floor: number): number {
  const contentH = navH(rows);
  const desired = park !== null ? Math.min(park, contentH) : contentH;
  return Math.max(navH(0), Math.min(desired, floor));
}

/**
 * True iff a stored sash height is a PARK at the minimum: the height cap is
 * itself the breadcrumb-only height, so no tree body can be seen no matter what
 * `overrides` says is open. That is the one case where "what is open" and "what
 * the user can see" are allowed to disagree, so every control that reads
 * `overrides` must also read this or it will act on invisible content .
 */
export function isParkedHeight(park: number | null): boolean {
  return park !== null && park <= navH(0);
}

/**
 * Maximum tree height that preserves a minimum editor height.
 * Pure arithmetic — no DOM access. The divider is the only chrome zone left
 * between the nav surface and the editor (the chip bar became document
 * content INSIDE the scroller with Amendment III, so it spends editor height,
 * not window chrome budget); the editor has no footer surface, so subtracting
 * the divider plus `minEditorH` from `windowH` is the full budget.
 */
export function maxTreeHeight(
  windowH: number,
  dividerH: number,
  minEditorH: number,
): number {
  return windowH - dividerH - minEditorH;
}
