// The file-tree controller for the editor action panel — the composition root
// of the `tree/` subunit, reached from outside only through `tree/index.ts`.
// It owns mount/unmount and the delegated listeners, the `TreeView` record and
// the anchor policy, the sash machine, the selection facade, `handleKey`, the
// gesture policy, and the engine commands its modules reach by host.
//
// A horizontal breadcrumb (the open file's path) over a vertical tree body
// that renders the OPEN-FOLDER FOREST, with a divider that is a SASH — it sets
// how much of the forest you see, never what's in it.
//
// ── Architecture: expansion is decoupled from viewport ──────────────────────
//
// One state map is the single source of truth for "what is open":
//
//   view.overrides: Map<EntryId, 'open' | 'closed'>
//
// It is one third of `view: TreeView` (tree/model.ts) — anchor, overrides,
// park — the record every write goes through `update(fn)` to change.
//
// The tree BODY renders the active path's ancestors' children DEEPEST-FIRST
// (see tree/spine-projection.ts / spineBody()): the active file's siblings sit directly
// under the breadcrumb, then the level above, etc. Each ancestor level shows iff
// its folder is open; nested non-active open folders recurse normally. The
// breadcrumb strip is a separate path indicator built from `activePath`.
//
// `overrides` changes only for a reason the user can point at: the folder
// chevron / name click handlers (toggleFolderOpen / handleToggleTop) and
// drag-fill (see below) express intent through openFolders() / setFolder();
// a refresh prunes folders that no longer exist; a delete forgets a subtree;
// a new session starts empty. EVERY one of them is a pure transition applied
// through `update()` — never an imperative .set/.delete on the live map.
// That one write path is the invariant that makes "who
// changed this?" answerable.
//
// VIEWPORT HEIGHT FOLLOWS CONTENT: height = clamp(navH(renderedRowCount),
// minH, floor). Open a folder → more rows → the panel grows (up to floor;
// scroll beyond). Close folders → fewer rows → the panel shrinks to fit — a
// blank gap is structurally impossible because height is a pure function of
// the rendered forest.
//
// ── The divider is a SASH, not a collapse affordance ─────────────────────────
//
// The divider sets how much of the tree you see; clicks set what the tree
// contains; and when the user asks to see more than exists, the tree opens
// the active path — nearest level first — until it fills what was asked.
//
// COLLAPSED IS NOT A STATE. Breadcrumb-only is the sash at its minimum height
// (navH(0) = 48px). Nothing is ever wiped, remembered, or restored — the open
// forest (`overrides`) survives untouched through every divider gesture; the
// user simply sees none of it.
//
// DRAG DOWN = REVEAL, THEN FILL. Height follows the cursor 1:1. First it
// uncovers rows that are already open (no state change at all). Only once
// the cursor pushes past content-fit by half a row of slack does the tree
// open the deepest CLOSED ancestor level of the active path — parent first,
// then upward, root last — one level per crossing. See setupDragHandle().
//
// RELEASE COMMITS NOTHING BUT THE HEIGHT. No threshold, no snap-back, no
// epilogue that can disagree with the gesture. Drag up clips rows away
// (folders stay open); Cmd+Shift+← parks the height at the minimum and the
// divider dblclick TOGGLES park/reveal — neither wipes any state.
//
// A PARKED SASH MUST NOT MAKE ITS OWN CONTROLS LIE. Parking leaves `overrides`
// untouched, so "open" and "visible" may disagree. isParkedHeight() closes
// that gap; BOTH consumers are required — chevrons paint closed while
// parked, so handleToggleTop must open while parked.
//
// moss follows Zed project_panel's pattern
// (https://github.com/zed-industries/zed/tree/main/crates/project_panel):
// one override set, mutated only by explicit user actions, separate from
// the panel size.
//
// ── Sibling connector rail ────────────────────────────
//
// The vertical line joining a folder's children is a DECLARATIVE CSS rail, not a
// measured overlay. Each role="group" wrapper (mkGroup) carries --ft-rail-offset
// = its children's column; a CSS ::before draws the rail there, behind the
// children's icons. syncRailOffsets() (tree/render.ts) copies each group's first child's live
// --ft-offset onto the wrapper after the SYNCHRONOUS alignHomePairChildren(), so
// the rail tracks the rows with no getBoundingClientRect and no rAF (a prior
// rAF-measured line went missing/stale in the idle action-panel WKWebView).
//
// ── DO NOT ──────────────────────────────────────────────────────────────────
//
// Never mutate `this.view.overrides` in place (`.set` / `.delete` on the
// live map): apply a tree/model.ts transition through `update()`. That is
// The invariant: openFolders()/setFolder() is how a gesture does it.
//
// Never wipe `overrides` from an event handler (pointermove, click, …) —
// the bug the one-write-path rule exists to prevent. The whole-map rebuilds that DO exist
// are all bookkeeping, not gesture: the refreshTree prune, the delete purge
// (forget), and the mount / unmount / collapseForReopen resets. Those are allowed; a
// gesture-driven wipe is not.

import { host as uiHost, type Documents } from "../../host";
import { sessionDocuments } from '../session/documents';
import { engine } from '../../engine';
import { info as logInfo } from "../../log";
import { EntryRegistry, type EntryId, type EntryRecord } from '../entry-registry';
import { toAbsolute, toRelative, normalizePath, PROJECT_ROOT_REL, childPath, parentDirOf, baseName, displayName, resolveCreateTarget, isMossInternalPath } from '../path-domain';
import { onLocaleChange } from "../../i18n/index";
import { markDragRegion } from '../../window-drag';
import { setTitleBar as setTitleBarImpl } from './title-bar';
import { closeActiveInputPopover } from "../../shared/ui/input-popover";
import type { FileEventsCoordinator } from '../editor-file-events';
import { EditGate, EDIT_PHANTOM_GRACE_MS, EDIT_WATCHDOG_MS } from './edit-gate';
import { TreeEditSession } from './edit-session';
import { rowAt, isChevron, isTextEntry, type TreeRow, TreeRender } from './render';
import { FileSelection, type SelectionSnapshot } from './selection';
import { keyToAction, resolveClickAction, computeNextFocusIndex, matchTypeAhead, TypeAhead, type ClickAction } from './keyboard';
import {
  buildActivePath, deepestFolderIndex, PreflightEvidence,
  initialView, collapseAll, setFolder, openFolders, anchorAt, withAnchor, parkAt, type TreeView,
} from './model';
import { TreeOps } from './ops';
import { TreeMenu } from './menu';
import { TreeRefresh } from './refresh';
import { TreeDerivations } from './derivations';
import { TreeDnd, treeTrace } from './dnd';
import { isParkedHeight, maxTreeHeight, navH, MIN_EDITOR_H, ROW_H } from './spine-projection';


/**
 * Gesture-outcome logging: ONE line at INFO, always on in
 * production (unlike treeTrace's DEBUG, which production filters out). This
 * is the direct answer to Finding 2 — a gesture whose failure left zero trace
 * in a full beta log. Format: `[tree] gesture=<name> outcome=<outcome> ...`.
 * Never throws and never surfaces an unhandled rejection: logging must not be
 * able to break a gesture, and in tests there is no Tauri IPC bridge behind
 * this call.
 */
function logGesture(msg: string): void {
  try {
    void logInfo(`[tree] ${msg}`).catch(() => { /* logging is best-effort */ });
  } catch { /* no IPC bridge (tests) — logging is a no-op */ }
}

/** Drag-fill slack (px, rule 3): half a row. Dragging down first REVEALS rows
 *  already open (height tracks the cursor 1:1, no state change) — only once
 *  the cursor pushes past content-fit by this much does the tree open the
 *  deepest CLOSED ancestor level of the active path. Half a row rather than
 *  zero avoids opening a new level on the exact pixel content-fit ends, which
 *  would fire on essentially every drag past a fully-revealed forest. */
const DRAG_FILL_SLACK_PX = ROW_H / 2;

/**
 * Best-effort "is this element on screen" check for the download-flash target
 * resolver (FIX 4). An element counts as on-screen when it is connected to the
 * document and is not clipped entirely out of the scrollable tree body.
 *
 * jsdom note: layout is not computed, so `getBoundingClientRect()` returns an
 * all-zero rect. A zero-area rect therefore can't be used to mean "hidden" —
 * we treat connected-with-zero-rect as ON screen (presence in `.nav-body`
 * already means the branch is expanded and rendered). In a real browser we
 * additionally drop rows whose vertical span lies entirely above/below the
 * nearest scroll container, so a row scrolled out of view falls through to the
 * breadcrumb fallback.
 */
function isElementOnScreen(el: HTMLElement): boolean {
  if (!el.isConnected) return false;
  const rect = el.getBoundingClientRect();
  // jsdom / not-yet-laid-out: zero-area rect → can't judge by geometry, and
  // presence in the rendered body already implies "expanded + rendered".
  if (rect.width === 0 && rect.height === 0) return true;
  // Live: clip against the nearest scrollable ancestor (the tree body). If the
  // row's band is fully outside that viewport it's scrolled off — not "on
  // screen" for flash purposes.
  const scroller = el.closest<HTMLElement>('.nav-body') ?? el.parentElement;
  if (!scroller) return true;
  const host = scroller.getBoundingClientRect();
  if (host.width === 0 && host.height === 0) return true;
  return rect.bottom > host.top && rect.top < host.bottom;
}

// ── FileTree Class ──────────────────────────────────────────────────────

/**
 * The editor-main.ts capabilities a rename/move/undo commit needs around the
 * open CM6 buffer — bundled into one object (rather than more positional
 * constructor args) since all three exist for the same reason and are
 * always wired together. Each defaults to a no-op so tests that don't care
 * can omit the constructor argument entirely.
 */
export interface EditorCommitHooks {
  /** Flush the editor's own pending (debounced) save to disk, same contract
   *  as `versions-mode.ts`'s `VersionsModeHost.flushPendingSave`. A rename/
   *  move plans against whatever is on disk right now; without this, an
   *  unsaved edit in the open buffer is invisible to the plan, and a
   *  debounced autosave landing AFTER the rename would overwrite its own
   *  file's just-rewritten links with the stale pre-flush bytes. */
  flushPendingSave(): Promise<unknown>;
  /** Freeze/unfreeze the open buffer for the commit's duration — closes the
   *  window AFTER flushPendingSave where new typing could re-dirty it and
   *  have a later autosave overwrite the commit's own rewrite. */
  setEditorCommitLock(locked: boolean): void;
  /** Re-parse the active file from disk once the commit (or its undo)
   *  completes — the backend may have rewritten the open file's own content
   *  (a rebased link) or another open page's incoming links, and the rename
   *  lifecycle event is deliberately "re-key, don't reload". */
  reloadActiveFileFromDisk(): Promise<void>;
}

const NOOP_EDITOR_COMMIT_HOOKS: EditorCommitHooks = {
  flushPendingSave: async () => {},
  setEditorCommitLock: () => {},
  reloadActiveFileFromDisk: async () => {},
};

export class FileTree {
  private readonly registry: EntryRegistry;
  private readonly fileEvents: FileEventsCoordinator | undefined;
  private readonly editorCommitHooks: EditorCommitHooks;

  constructor(registry: EntryRegistry, fileEvents?: FileEventsCoordinator, editorCommitHooks?: EditorCommitHooks) {
    this.registry = registry;
    this.fileEvents = fileEvents;
    this.editorCommitHooks = editorCommitHooks ?? NOOP_EDITOR_COMMIT_HOOKS;
  }

  private container: HTMLElement | null = null;
  private projectRoot = '';
  private selection = new FileSelection();

  /**
   * Reads from the canonical owner (EntryRegistry). selection.focus
   * mirrors this via the subscriber registered in mount().
   */
  private get activeEntryId(): EntryId | null {
    return this.registry.activeEntryId;
  }

  /**
   * Routes writes through registry. selection.setSingle/clear runs first
   * so the multi-select set is collapsed in the same tick. After
   * registry.setActive fires, the mount-time subscriber's !== guard
   * short-circuits (focus is already at the new value) so we get exactly
   * one notifySelectionChanged at the end.
   */
  private set activeEntryId(id: EntryId | null) {
    if (id === null) this.selection.clear();
    else this.selection.setSingle(id);
    this.registry.setActive(id);
    this.notifySelectionChanged();
  }

  /** Dev-only: snapshot of the current selection state. */
  public getSelectionSnapshot(): SelectionSnapshot {
    return this.selection.snapshot();
  }

  /** Clear the tree selection — used when the editor shows a non-file pane
   *  (the home / no-source state) so a stale file highlight doesn't linger and
   *  contradict the pane. Only re-renders + notifies; triggers no file load. */
  public clearSelection(): void {
    this.mutateSelection(sel => sel.clear());
  }

  /** The single active entry (focus), for the editor pane and __moss_debug.
   *  Reads from the canonical owner (EntryRegistry). */
  public getActiveEntryId(): EntryId | null {
    return this.registry.activeEntryId;
  }

  /**
   * Dispatch `editor:selection-changed` on the tree container with the
   * current snapshot. Call this AFTER any mutation to `this.selection`
   * (toggle/setSingle/setRange/addRange/clear/setFocus/reconcile). The
   * snapshot is a defensive copy — consumers can read it freely.
   */
  private notifySelectionChanged(): void {
    // Repaint selected visual state on every row first, so the DOM is in
    // sync before listeners observe the event. Cheap — selectorAll over
    // rendered rows + classList/attr toggle.
    this.render.renderSelectionVisualState();
    if (!this.container) return;
    this.container.dispatchEvent(
      new CustomEvent('editor:selection-changed', {
        bubbles: true,
        detail: this.selection.snapshot(),
      }),
    );
  }

  /** After a refresh: drop selected ids that no longer exist, notifying only
   *  if something went (per FileSelection.reconcile's contract). */
  private reconcileSelection(validIds: Set<EntryId>): void {
    const sizeBefore = this.selection.size;
    this.selection.reconcile(validIds);
    if (this.selection.size !== sizeBefore) {
      this.notifySelectionChanged();
    }
  }

  /**
   * Convenience wrapper: run a selection mutation then notify.
   * All keyboard handlers use this instead of manually calling
   * notifySelectionChanged() after each mutation.
   */
  private mutateSelection(fn: (sel: FileSelection) => void): void {
    fn(this.selection);
    this.notifySelectionChanged();
  }

  /**
   * Focus the tree row whose id matches `selection.focus` (or the first visible
   * row if focus is null). Called by Cmd+1 cross-pane shortcut.
   */
  public focus(): void {
    if (!this.render.body) return;
    const focusId = this.selection.focus;
    if (focusId !== null) {
      const el = this.render.rowFor(focusId);
      if (el && !el.classList.contains('bc')) { el.focus(); return; }
    }
    // Fall back to first visible tree row (bc segs aren't focusable —
    // they're display-only navigation, never selection targets).
    const firstRow = this.render.body.querySelector<HTMLElement>('[data-path]:not(.bc)');
    firstRow?.focus();
  }

  /**
   * The entry IDs of currently-visible rows, in DOM render order.
   * "Visible" means rendered in the tree BODY right now — rows inside collapsed
   * folders are excluded. Used by Shift+click range selection and by keyboard
   * focus movement — the two read the same list.
   *
   * Body-only (`:not(.bc)`): under the decoupled open-forest model the body
   * renders the active file and every open ancestor folder as real rows, so the
   * full selectable range lives in `.nav-body`. The breadcrumb is a path
   * indicator whose segments duplicate body rows — including them here would
   * double-count the active file / ancestors and inflate a Shift range, and
   * its segments are display-only, never focus targets.
   */
  public visibleEntryIds(): EntryId[] {
    const rows = this.render.body?.querySelectorAll<HTMLElement>('[data-path]:not(.bc)') ?? [];
    const out: EntryId[] = [];
    for (const row of rows) {
      const id = this.registry.getByPath(row.dataset.path!);
      if (id !== null) out.push(id);
    }
    return out;
  }

  /**
   * Derived: the path of the currently-active file in the tree, looked up
   * live from the registry. Returns null when no file is active OR the
   * registry doesn't (yet) know about the entry.
   */
  private get currentFilePath(): string | null {
    return this.registry.pathOf(this.activeEntryId);
  }

  // ── Public OS drag-in accessors ──────────────────────────────────

  /**
   * The project root path this tree is mounted on.
   * Used by os-drag-in.ts as the fallback drop target when no folder row is
   * under the cursor (drop on empty area → import into site root).
   */
  public get projectRootPath(): string {
    return this.projectRoot;
  }

  /**
   * Return the project-root-RELATIVE path of the entry with the given id,
   * or null if the id is not registered. (The registry stores relative paths
   * from the project root; see path-domain.ts.)
   *
   * Used by os-drag-in.ts to map a hit-tested EntryId to a path string; callers
   * that hand the result to a Tauri command MUST `toAbsolute(..., projectRootPath)`
   * at the FS boundary before calling e.g. `copyFilesToProject`.
   */
  public pathOf(id: EntryId): string | null {
    return this.registry.pathOf(id);
  }

  /**
   * The breadcrumb anchor (activePath leaf) as a READ-ONLY query for the
   * Untitled-first create flow (create-flow.ts): `resolveCreateTarget()`
   * derives the create destination from it. Never a write path into
   * `activePath`.
   */
  public get createAnchor(): { path: string; isDir: boolean } | null {
    const leaf = this.view.anchor[this.view.anchor.length - 1];
    if (leaf === undefined) return null;
    return { path: leaf, isDir: this.isDirectory(leaf) };
  }

  // ── Drag and drop — tree/dnd.ts ────────────────────────────────────────
  // The session, the hit-test, the drop and the hover feedback live there;
  // these are the entry points os-drag-in.ts, editor-main.ts and cm-drop.ts
  // reach them through.

  public resolveDropEntries(ids: number[]): string[] { return this.dnd.resolveDropEntries(ids); }
  public getFolderAtPosition(x: number, y: number): EntryId | null { return this.dnd.getFolderAtPosition(x, y); }
  public springLoadIfHoverIs(dirPath: string | null): void { this.dnd.springLoadIfHoverIs(dirPath); }
  public setDropHighlight(id: EntryId | null): void { this.dnd.setDropHighlight(id); }
  public updateEdgeScroll(clientY: number): void { this.dnd.updateEdgeScroll(clientY); }
  public stopEdgeScroll(): void { this.dnd.stopEdgeScroll(); }
  public hasActiveInternalDrag(): boolean { return this.dnd.hasActiveInternalDrag(); }
  public activeInternalDragEntryIds(): number[] { return this.dnd.activeInternalDragEntryIds(); }
  public clearInternalDragSession(): void { this.dnd.clearInternalDragSession(); }
  public handleInternalDragOver(x: number, y: number): void { this.dnd.handleInternalDragOver(x, y); }
  public handleInternalDrop(x: number, y: number): void { this.dnd.handleInternalDrop(x, y); }

  /** Re-scan the tree and repaint — THE refresh path (tree/refresh.ts): os-drag-in, CreateFlow and editor-main reach it here. */
  public refreshAndRender(): Promise<void> {
    return this.refresh.refreshAndRender();
  }

  /**
   * FIX 4 (download flash) — resolve the DEEPEST currently-visible container
   * that holds `dirPath`, for a viewport-only confirmation flash. Returns:
   *   1. a file-tree folder ROW (`.nav-body .seg[data-path]`) for `dirPath` or
   *      its deepest rendered ancestor, when the tree is expanded enough that
   *      that folder's row is on screen;
   *   2. ELSE the deepest breadcrumb SEGMENT (`.nav-bc-strip .seg.bc[data-path]`)
   *      whose path is an ancestor of (or equal to) `dirPath`;
   *   3. ELSE `null` (caller flashes the whole nav-surface).
   *
   * `dirPath` is a project-root-relative directory key (PROJECT_ROOT_REL '' for
   * a top-level file's parent). Pure DOM read — never mutates expansion
   * overrides, never expands the tree.
   *
   * Body rows render only for expanded branches (see render()/buildGroup), so a
   * row's mere PRESENCE in `.nav-body` means the tree is expanded to it — the
   * exact "tree is expanded and that row is on screen" condition. Visibility is
   * additionally gated by `isElementOnScreen` so a row scrolled out of the
   * scroll-clipped body falls through to the breadcrumb (and ultimately the
   * nav-surface), matching the spec's "on screen" requirement.
   */
  public deepestVisibleContainerFor(dirPath: string): HTMLElement | null {
    if (!this.container) return null;
    // Build the ancestor chain deepest → shallowest: dirPath, its parent, … up
    // to (and including) the project root. Separator-agnostic to match the
    // registry's native-separator keys.
    const chain: string[] = [];
    let cur = dirPath;
    // Guard against a pathological loop on malformed input.
    for (let i = 0; i < 256; i++) {
      chain.push(cur);
      if (cur === PROJECT_ROOT_REL) break;
      const lastSep = Math.max(cur.lastIndexOf('/'), cur.lastIndexOf('\\'));
      cur = lastSep >= 0 ? cur.slice(0, lastSep) : PROJECT_ROOT_REL;
    }

    // Deepest first: a body row counts only while on screen (the body
    // scrolls), a breadcrumb segment always does (the strip does not). Body
    // rows can only sit deeper in the chain than breadcrumb segments — a
    // crumb's ancestors are all crumbs — so one pass finds the deepest of
    // either kind.
    for (const path of chain) {
      const id = this.registry.getByPath(path);
      const el = id === null ? null : this.render.rowFor(id);
      if (el === null) continue;
      if (el.classList.contains('bc') || isElementOnScreen(el)) return el;
    }

    // No visible container — caller flashes the whole nav-surface.
    return null;
  }

  /** FIX 4 helper — the shared green confirmation pulse on `path`'s row (tree/render.ts). */
  public flashRow(path: string): void {
    this.render.flashRow(path);
  }

  private abortController: AbortController | null = null;
  // Unsubscribe handle for the registry.subscribeActive registered in mount.
  // Mirrors registry.activeEntryId into selection.focus for paths that
  // bypass the activeEntryId setter (e.g., editor-main's loadFile path,
  // future external setActive callers).
  private unsubscribeActive: (() => void) | null = null;
  private dragHandleEl: HTMLElement | null = null;

  // Directory index: folder id -> child ids in display order. Every fact
  // about an entry lives on its registry record; this is only the shape.
  private readonly dirIndex = new Map<EntryId, EntryId[]>();

  /**
   * What the tree shows — breadcrumb anchor, open forest, sash park — as ONE
   * record (tree/model.ts). Read `this.view.anchor` etc. freely; write only
   * through `update()`, the one write path.
   */
  private view: TreeView = initialView();

  /** The one writer of `view`. Pure transition in, new record out; callers
   *  re-render and re-fit height where the gesture demands it. */
  private update(fn: (view: TreeView) => TreeView): void {
    this.view = fn(this.view);
  }

  // Unsubscribe for the live-language-switch relabel (re-renders the tree chrome
  // under the new locale; render() defers while editing and preserves overrides).
  private localeUnsub: (() => void) | null = null;

  /**
   * THE tree's coordination state — the inline edit the user has open, the
   * `listTree` round trip the tree has in flight, and the work parked behind
   * either. One typed record with one admission question; see tree/edit-gate.ts
   * for what it replaced and why the derivations are getters, not fields.
   */
  private readonly gate: EditGate = new EditGate({
    hasLiveInput: () => this.editSession.liveEditInput() !== null,
    now: () => performance.now(),
    discard: (session) => { if (session.kind === 'create') this.editSession.discardTempEntry(session); },
    // The watchdog fired, so nothing else is about to repaint: undo the
    // bookkeeping AND drive the recovery the user sees.
    reap: (session) => {
      if (session.kind === 'create') this.editSession.discardTempEntry(session);
      this.render.render();
      this.editSession.flushPendingWork();
    },
    trace: (msg) => treeTrace(msg),
  });

  /**
   * The vault's folder derivations (tree/derivations.ts): what the hint rows
   * and the folder menu's "Show … as pages" read. Refreshed by the refresh
   * cycle below, written by the hint row's actions and that menu item.
   */
  private readonly derivations: TreeDerivations = new TreeDerivations({
    projectRoot: () => this.projectRoot,
    commands: engine,
    render: () => this.render.render(),
  });

  /**
   * The refresh cycle (tree/refresh.ts): the listing, the `.moss` toggle,
   * every write to `dirIndex`, and the watcher. A field because `forget` and
   * `adopt` are reached from the create flow and the delete family; the
   * watcher subscribes in mount() against its signal, whose abort is the one
   * teardown. It reaches the model only through the members named here
   * (`RefreshHost`), and `listTree` through this one import.
   */
  private readonly refresh: TreeRefresh = new TreeRefresh({
    projectRoot: () => this.projectRoot,
    commands: engine,
    registry: () => this.registry,
    dirIndex: this.dirIndex,
    update: (fn) => this.update(fn),
    reconcileSelection: (validIds) => this.reconcileSelection(validIds),
    gate: this.gate,
    render: () => this.render.render(),
    leaveInternal: () => this.leaveInternal(),
    flushPendingWork: () => this.editSession.flushPendingWork(),
    refreshDerivations: () => this.derivations.refresh(),
  });

  /** The open documents a move or rename keeps up with: the host's own, else moss's session. */
  private readonly documents: Documents = uiHost.documents ?? sessionDocuments({
    flush: () => this.editorCommitHooks.flushPendingSave(),
    events: () => this.container,
  });

  /**
   * The inline rename and create sessions (tree/edit-session.ts): the one
   * <input>, the rename commit with its prediction and rollback, the create
   * commit, and the drain of whatever the gate parked. A field because the
   * gate's deps and the refresh host reach it (lazily) before mount, and
   * declared before `ops`, whose host holds it by value. It reaches the
   * model only through the members named here (`EditSessionHost`).
   */
  private readonly editSession: TreeEditSession = new TreeEditSession({
    projectRoot: () => this.projectRoot,
    commands: engine,
    registry: () => this.registry,
    refresh: this.refresh,
    gate: this.gate,
    update: (fn) => this.update(fn),
    render: () => this.render.render(),
    rowFor: (id) => this.render.rowFor(id),
    container: () => this.container,
    carriesSelfNamedHome: (path) => this.carriesSelfNamedHome(path),
    documents: this.documents,
    setEditorCommitLock: (locked) => this.editorCommitHooks.setEditorCommitLock(locked),
    reloadActiveFileFromDisk: () => this.editorCommitHooks.reloadActiveFileFromDisk(),
  });

  /**
   * The vault operations (delete, duplicate, move, reveal, copy, URL import).
   * Stateless, so a field rather than a mount-time construction: there is
   * nothing to tear down. It reaches the model only through the members named
   * here (tree/ops.ts `OpsHost`).
   */
  private readonly ops = new TreeOps({
    projectRoot: () => this.projectRoot,
    commands: engine,
    registry: () => this.registry,
    children: (path) => this.children(path),
    gate: this.gate,
    refresh: this.refresh,
    editSession: this.editSession,
    render: () => this.render.render(),
    update: (fn) => this.update(fn),
    showFolder: (folder) => this.showFolder(folder),
    documents: this.documents,
    setEditorCommitLock: (locked) => this.editorCommitHooks.setEditorCommitLock(locked),
    reloadActiveFileFromDisk: () => this.editorCommitHooks.reloadActiveFileFromDisk(),
  });

  /**
   * Drag and drop (tree/dnd.ts): the session, the hit-test, the drop, the
   * OS drag-out and the hover feedback. Its listeners register in mount()
   * against the signal, whose abort is the one teardown. It reaches the
   * model only through the members named here (`DndHost`); the drop's move
   * is `ops.moveEntriesTo`; the drag-out is the host's.
   */
  private readonly dnd = new TreeDnd({
    projectRoot: () => this.projectRoot,
    registry: () => this.registry,
    selection: this.selection,
    isOpen: (path) => this.isOpen(path),
    toggleFolderOpen: (path) => this.toggleFolderOpen(path),
    ops: this.ops,
    flashRow: (path) => this.flashRow(path),
  });

  /**
   * The renderer (tree/render.ts): the breadcrumb strip, the tree body, the
   * spine that allocates their offsets, the row cache, and the nav surface's
   * height. Its containers are built in mount() and dropped in unmount(). It
   * reaches the model only through the members named here (`RenderHost`) and
   * writes nothing back except through the DOM.
   */
  private readonly preflightEvidence = new PreflightEvidence();

  private readonly render = new TreeRender({
    projectRoot: () => this.projectRoot,
    registry: () => this.registry,
    view: () => this.view,
    children: (path) => this.children(path),
    isOpen: (path) => this.isOpen(path),
    isDirectory: (path) => this.isDirectory(path),
    hiddenCount: (path) => this.hiddenCount(path),
    derivations: this.derivations,
    preflightEvidence: this.preflightEvidence,
    selection: this.selection,
    gate: this.gate,
    floor: () => this.computeFloor(),
  });

  /** The context menus (tree/menu.ts). Built in mount(), closed by its signal. */
  private menu: TreeMenu | null = null;

  // Chrome heights — updated at drag mousedown and on window resize
  private chromeDividerH = 0;

  /** Type-ahead (tree/keyboard.ts): the prefix the user is typing, reset on a pause. */
  private readonly typeAhead = new TypeAhead();

  // ── Public API ──────────────────────────────────────────────────────

  /**
   * Lock the file tree while a ship is in progress.
   *
   * Keyed on the `editor:seal` event (SSOT) via editor-main.ts. Toggles
   * `.moss-tree--sealed` on the tree container; CSS handles `pointer-events:none`
   * + `cursor:not-allowed`. No direct coupling to deploy-handler.
   *
   * No-op before mount (container is null) — safe to call at any time.
   */
  setSealed(sealed: boolean): void {
    this.container?.classList.toggle('moss-tree--sealed', sealed);
  }

  /** Replace, never merge, the completed-build evidence shown by tree rows. */
  setPublishPreflight(projection: import('@symbiosis-lab/moss-ui').PublishPreflightProjection | null): void {
    this.preflightEvidence.replace(projection?.missing_references.map(({ source_path }) => source_path) ?? []);
    this.render.render();
  }

  /** The title bar as a navigation stack (round six, tree/title-bar.ts) —
   *  `content` replaces the breadcrumb wholesale; `null` restores it. Also
   *  collapses `#nav-surface` to `navH(0)` while a title bar is up: hiding
   *  `.nav-body` doesn't shrink its JS-owned pixel height (found live:
   *  stranded ~90px of blank space under the breadcrumb). `applyWrapperHeight()`
   *  restores exactly, since nothing here ever mutates `view.park`/`overrides`
   *  — only hides what they describe. */
  setTitleBar(content: HTMLElement | null): void {
    setTitleBarImpl(this.render.strip, content);
    if (content) this.render.setWrapperHeight(navH(0));
    else this.render.applyWrapperHeight();
  }

  /** `site` is false for a loose document's folder (MenuHost.site). */
  async mount(container: HTMLElement, projectRoot: string, showInternal: boolean, site = true): Promise<void> {
    this.container = container;
    this.projectRoot = projectRoot;
    // The root is a directory before the first listTree answers, and stays one
    // if it never does: a boot whose refresh fails must still let the root open
    // and an override needs an id.
    this.registry.upsert(PROJECT_ROOT_REL, true);
    this.refresh.showInternal = showInternal;
    // Live language switch (no restart): re-render the tree chrome under the new
    // locale. render() defers if an inline rename/create input is mounted and
    // preserves expansion overrides. Paired with unmount().
    this.localeUnsub?.();
    this.localeUnsub = onLocaleChange(() => { if (this.container) this.render.render(); });
    this.abortController = new AbortController();
    const { signal } = this.abortController;
    const menu = new TreeMenu({
      ops: this.ops,
      commands: engine,
      site,
      projectRoot: () => this.projectRoot,
      container: () => this.container,
      createAnchor: () => this.createAnchor,
      recordAt: (path) => this.registry.recordAt(path),
      homeEntry: (path) => this.render.homeEntry(path),
      selectionSize: () => this.selection.size,
      selectionPaths: () => this.selectionPaths(),
      startCreate: (...args) => this.startCreate(...args),
      renameEntry: (entry) => this.renameEntry(entry),
      derivationAt: (path) => this.derivations.at(path),
      setDerivation: (path, choice) => { void this.derivations.set(path, choice); },
      // Versions mode lives in THIS webview (editor-main.ts listens); the main-window
      // panel route it replaced had no registrations here and did nothing.
      openVersions: (scope, path) => document.dispatchEvent(new CustomEvent('editor:open-versions', { detail: { scope, path, isRoot: this.isDirectory(path) && path === PROJECT_ROOT_REL } })),
    }, signal);
    this.menu = menu;

    // The breadcrumb strip and the tree body are the renderer's
    // (tree/render.ts); it builds them under `container` (= .nav-grid).
    this.render.mount(container);
    // The strip's bare area — the space to the right of the segments, freed
    // now that the leaf no longer absorbs trailing surplus — is a window drag
    // handle. Allowlist: the strip is the drag surface; its clickable segments
    // are their own mousedown targets (exact-target match) and never drag.
    // Registered here, not in the renderer: the shell reach-in stays the
    // controller's one edge.
    markDragRegion(this.render.strip!);

    // Right-click on panel whitespace (below the last row, between rows, the
    // breadcrumb strip's bare area): the surface owns its right-click — show
    // the Add-content menu targeting the browsed folder, exactly like the ＋▾
    // menu (design-vocabulary: the native webview menu never leaks). Row and
    // segment handlers stopPropagation, so anything arriving here IS empty
    // area. Text inputs (inline rename) keep the native editing menu.
    container.addEventListener('contextmenu', (e) => {
      // Inside an open rename box the native editing menu is what the user
      // wants, so bail before preventDefault, not after. Same boundary rowAt
      // enforces, asked separately because the order matters here.
      if (isTextEntry(e.target)) return;
      e.preventDefault();
      const tgt = e.target as Element | null;
      // ONE place decides which menu a right-click gets. Every row and
      // breadcrumb segment carries its own data-path (the project root's is
      // the empty string, hence the `undefined` test rather than falsiness);
      // anything else is bare surface, which the Add-content menu owns —
      // the native webview menu never leaks (design-vocabulary).
      const path = tgt?.closest<HTMLElement>('.seg[data-path]')?.dataset.path;
      if (path === undefined) menu.showEmptyAreaCtx(e);
      else {
        // Breadcrumb segments (`.seg.bc`) and tree rows share this one path
        // resolution but are different surfaces for the Send Logs breadcrumb —
        // a right-click on the strip vs. one on a row read very differently
        // when replaying "what did the user click" from a bundle.
        const surface = tgt?.closest('.seg.bc') ? 'breadcrumb-segment' : 'tree-row';
        this.openContextMenuAt(path, e, undefined, surface);
      }
    }, { signal });

    // A right-button press must not start a selection or drag gesture. Was
    // six identical per-row listeners; the rows have nothing to add to it.
    container.addEventListener('mousedown', (e) => {
      if (e.button === 2) e.preventDefault();
    }, { signal });

    // The drag listeners — source, target, the margin fallback — register
    // from tree/dnd.ts against the same signal.
    this.dnd.mount(container, this.render.body!, signal);

    // ── Row gestures, delegated ──────────────────────────────────────
    // Every row and breadcrumb segment shares one click handler (and, in
    // tree/dnd.ts, one drag-source handler), resolved from the event's row
    // at fire time. Per-row listeners were what made a row element
    // non-reusable: their closures captured a path that a rename
    // invalidates, so render() had to destroy and rebuild every row. With
    // the handlers here, a row element is inert markup and render() can
    // reuse it.
    this.container.addEventListener('click', (e) => {
      // A hint row's action (tree/derivations.ts): it is no row, and no bare surface either.
      const act = (e.target as Element | null)?.closest<HTMLElement>('[data-derive-action]');
      const hint = act?.closest<HTMLElement>('.nav-hint-row');
      if (act && hint) {
        this.runHintAction(act, hint.dataset.derivePath ?? '');
        return;
      }
      const row = rowAt(e.target);
      if (row === null) {
        // Empty area: clear the multi-selection, as Finder / VS Code /
        // Explorer do. Rows return early below, so anything reaching here
        // IS bare surface — no second listener and no `target === navBody`
        // test needed.
        this.selection.clear();
        this.notifySelectionChanged();
        return;
      }
      if (isChevron(e.target, row.el)) {
        // Chevron toggles expansion only; it never selects, opens or renames.
        this.toggleRow(row);
        return;
      }
      // The second press of a double-click never repeats the first: inert
      // on a folder (acting twice toggles shut what the first opened), the
      // inline rename on a file — Enter's edit; a double-click also
      // renames it, so both rest on
      // the row surviving the render the first click triggered: the browser
      // raises `detail` to 2 only when both presses land on the SAME element,
      // and a rebuilt row would arrive as a fresh detail-1 click.
      if (e.detail > 1) {
        if (e.detail === 2 && resolveClickAction(e) === 'plain' && !this.isDirectory(row.path)) {
          const record = this.registry.recordAt(row.path);
          if (record !== null) this.renameEntry(record);
        }
        return;
      }
      this.clickRow(row, e);
    }, { signal });

    // Keyboard navigation: single document-level listener, filtered by
    // tree focus (see handleKey). AbortSignal tears it down at unmount.
    document.addEventListener('keydown', this.handleKey, { signal });

    // Mirror registry.activeEntryId to selection.focus. When other code
    // calls registry.setActive (loadFile, file-changed handler, future
    // external mutators), the tree's visual focus follows. FileSelection
    // retains its own multi-select state; only the focus field is
    // downstream of the registry. The setSingle/clear path used by the
    // activeEntryId setter below ALSO updates selection.focus before
    // registry.setActive fires, so this subscriber is a no-op for
    // setter-driven changes (the !== guard short-circuits and notify is
    // skipped — the setter notifies once at the end on its own).
    this.unsubscribeActive = this.registry.subscribeActive((newId, _oldId) => {
      if (newId === this.selection.focus) return;
      this.selection.setFocus(newId);
      this.notifySelectionChanged();
    });

    // Load full tree upfront
    await this.refresh.refreshTree();

    // Don't guess which file to show — just render root breadcrumb.
    // highlight() will set the correct activePath.
    // activePath stores REGISTRY-DOMAIN paths (project-root-relative); the
    // root is PROJECT_ROOT_REL ('').
    this.update(() => initialView([PROJECT_ROOT_REL]));
    // Start with the window closed (breadcrumb only). It stays closed across
    // navigation until the user explicitly opens it; see navigateTo().
    this.render.render();

    // The post-mount re-measures — once fonts settle, and once after Tauri's
    // sizing reaches the web content process (WKWebView renders the first
    // pass at panelW=0). See TreeRender.armInitialSettle.
    this.render.armInitialSettle();

    // Start watching for FS changes
    this.refresh.startWatching(signal, this.fileEvents);
  }

  unmount(): void {
    this.abortController?.abort();
    this.abortController = null;

    // Tear down the spine's observer and timers (a stray timer would fire
    // reallocateSpine against a torn-down DOM) and drop the containers.
    this.render.unmount();

    if (this.unsubscribeActive) {
      this.unsubscribeActive();
      this.unsubscribeActive = null;
    }

    if (this.localeUnsub) {
      this.localeUnsub();
      this.localeUnsub = null;
    }

    if (this.container) {
      this.container.innerHTML = '';
    }

    this.container = null;
    this.dragHandleEl = null;
    this.activeEntryId = null;
    this.update(() => initialView());
    this.menu = null;
    closeActiveInputPopover();
    this.gate.reset();
    this.typeAhead.dispose();
  }

  /** Toggle `.moss/` visibility in the tree — the refresh cycle's; see tree/refresh.ts. */
  toggleInternal(showInternal: boolean): Promise<void> {
    return this.refresh.toggleInternal(showInternal);
  }

  /** `toggleInternal(false)`'s anchor policy: if the user was viewing a
   *  `.moss/` file (whether still in activePath or not — e.g. they navigated
   *  away first), re-home to root. */
  private leaveInternal(): void {
    const inActive = this.view.anchor.some(isMossInternalPath);
    const currentInMoss = this.currentFilePath != null && isMossInternalPath(this.currentFilePath);
    if (inActive || currentInMoss) {
      this.update(v => anchorAt(v, PROJECT_ROOT_REL));
      this.activeEntryId = null;
    }
  }

  /** Return the first .md file found depth-first, or null. Returns the
   *  project-root-relative path. */
  getFirstFile(): string | null {
    return this.findFirstFile(PROJECT_ROOT_REL);
  }

  private findFirstFile(dir: string): string | null {
    const entries = this.children(dir);
    if (!entries) return null;
    // Check files first (non-dirs), then recurse into dirs
    for (const e of entries) {
      if (!e.isDir && e.path.endsWith('.md')) return e.path;
    }
    for (const e of entries) {
      if (e.isDir) {
        const found = this.findFirstFile(e.path);
        if (found) return found;
      }
    }
    return null;
  }

  /**
   * Highlight a file by path. Converts to project-root-relative via
   * `toRelative()` (idempotent for inputs already in that domain) before
   * touching the registry. Callers from Tauri event boundaries may carry
   * absolute paths; callers from internal state always pass relative.
   *
   * FROZEN BREADCRUMB ANCHOR: the breadcrumb (`activePath`) is an
   * anchor that stays put while the tree is EXPANDED, so opening files never
   * reshuffles the layout (see the design doc). Setting `activeEntryId` (the
   * setter) already repaints the selection highlight onto the open file's BODY
   * row via notifySelectionChanged() — so an open file is visibly highlighted in
   * place. Only when the panel is AT MINIMUM HEIGHT (breadcrumb-only) does the
   * breadcrumb track the open file; that path re-anchors via navigateTo().
   *
   * The trigger is a GEOMETRY read (isAtMinHeight()), not the rendered row
   * count: the forest can be non-empty while the sash is dragged down to
   * minimum height, and the anchor follows the open file exactly when the user
   * can't see the forest at all, regardless of what's open underneath.
   *
   * FORCE-ANCHOR OVERRIDE: `opts.forceAnchor` re-anchors
   * unconditionally, even while the panel is taller than minimum. Reserved for
   * navigation that originates OUTSIDE the tree — the preview webview
   * navigating to a different page, synced back into the editor via
   * loadFile(path, {forceAnchor: true}) — which is a bigger shift in attention
   * than browsing a sibling inside the currently-open folder, so the
   * stable-layout rationale doesn't apply. In-tree callers (row clicks,
   * wikilink nav within the editor) must keep omitting `opts` so the
   * frozen-anchor behavior above is unaffected. navigateTo() has no reshape
   * side effect when called at non-minimum height — see its docstring — so
   * this override is safe.
   */
  highlight(filePath: string, opts?: { forceAnchor?: boolean }): void {
    const relPath = toRelative(filePath, this.projectRoot);
    this.activeEntryId = this.registry.upsert(relPath, false);
    // Above minimum height → anchor frozen (stable layout) UNLESS forceAnchor
    // overrides it. At minimum height → always re-point the breadcrumb at the
    // open file (stays at minimum height — navigateTo() never grows it).
    if (opts?.forceAnchor || this.isAtMinHeight()) {
      this.navigateTo(relPath);
    }
  }

  /**
   * "Is the sash showing nothing but the breadcrumb right now" — the re-keyed
   * predicate highlight()/navigateTo() use in place of the old
   * `renderedRowCount() === 0` inference (see highlight()'s docstring). A
   * geometry read of the height model, not of the DOM, so it holds with or
   * without a wrapper (unit tests construct a FileTree without
   * setupDragHandle) and can never disagree with the height applied.
   */
  private isAtMinHeight(): boolean {
    return this.render.surfaceHeight() <= navH(0);
  }

  /**
   * Reveal a JUST-CREATED file by making it the breadcrumb ANCHOR + selection —
   * WITHOUT expanding the tree body.
   *
   * This is the create-time counterpart to `highlight()`. `highlight()` honors
   * the FROZEN BREADCRUMB ANCHOR: while the window is open it repaints the
   * selection in place but deliberately does NOT move the anchor, so browsing/
   * opening an EXISTING file never reshuffles the layout. A fresh "Create folder
   * page" CTA is different — the newly created home IS the thing the user just
   * asked for, so it SHOULD become the breadcrumb anchor (its folder + home file
   * surface in the breadcrumb spine, and the editor opens the file). But creating
   * a folder page must NOT force the tree body open: we re-point the anchor and
   * select, and leave `overrides` (the expansion state) exactly as the user left
   * it . With the target folder collapsed the tree body
   * stays collapsed; the folder just becomes the anchor.
   *
   * `overrides` is untouched here — no `openAncestors`, no expansion.
   * Only `activePath` (the breadcrumb anchor) and the selection move.
   */
  public revealNewFile(filePath: string): void {
    const rel = toRelative(filePath, this.projectRoot);
    // Re-point the breadcrumb anchor at the new file and select it — but do NOT
    // expand the tree body (overrides stays as-is).
    this.update(v => anchorAt(v, rel));
    this.activeEntryId = this.registry.upsert(rel, false);
    this.render.render();
    this.render.applyWrapperHeight();
  }

  /**
   * Park the sash at its minimum height (breadcrumb-only) — WITHOUT touching
   * `overrides`. Two entry points: the divider double-click (which toggles —
   * revealFromPark() is the other) and Cmd+Shift+←. The chip-bar double-click and
   * Escape routed here ; both were removed as hidden one-way gestures.
   *
   * This replaces the old
   * collapse(), which cleared every open folder into a `lastOpenOverrides`
   * stash and re-anchored the breadcrumb. Collapsed is not a state — the open
   * forest survives untouched through every divider gesture, so there is
   * nothing to snapshot or restore.
   *
   * RE-ANCHORS THE BREADCRUMB (carried over from the old collapse()):
   * parking at minimum height is the one moment the breadcrumb should track
   * the currently-open file (see highlight()'s isAtMinHeight() re-key) — so
   * when a file is open, route through navigateTo() to re-point `activePath`
   * and re-render/re-fit. navigateTo() never touches `overrides` either.
   */
  parkAtMinHeight(): void {
    this.update(v => parkAt(v, navH(0)));
    const openFile = this.currentFilePath;
    if (openFile !== null) {
      this.navigateTo(openFile);
    } else {
      this.render.applyWrapperHeight();
    }
    logGesture(`gesture=park-min-height outcome=parked`);
  }

  /** Inverse of parkAtMinHeight(). REVEALING IS A CONTENT OPERATION: the park is
   *  a CAP, so restoring a remembered height reveals nothing when nothing is open
   *  (min(300, 48) is 48). Reveal = open the breadcrumb's deepest folder — what
   *  clicking that chevron does, so gesture and visible control cannot drift.
   *  Called only while parked, so handleToggleTop takes its OPEN branch. */
  revealFromPark(): void {
    this.update(v => parkAt(v, null));
    // -1 = releasing the cap already revealed the forest; open nothing extra.
    const idx = this.render.renderedRowCount() > 0
      ? -1 : deepestFolderIndex(this.view.anchor, (p) => this.isDirectory(p));
    if (idx >= 0) this.handleToggleTop(idx); else this.render.applyWrapperHeight();
  }

  /**
   * Reset tree state for a warm reopen (the panel stays mounted).
   *
   * When the editor webview is reopened (via __moss_editor_open eval from
   * Rust's open_editor), mount() is NOT called — the webview shell is alive
   * and persistent. Without this, the previous session's open-folder overrides
   * persist into the new session, so the tree reopens with stale expansion
   * rather than starting collapsed.
   *
   * Call this from openFromWarmReopen in editor-main.ts BEFORE the
   * applyResolvedNode → highlight() / navigateTo runs — that ensures the new
   * navigateTo call builds fresh state. (NOT from applyResolvedNode itself,
   * which is also the in-session 'editor:load-source' body.)
   *
   * This is a genuinely different purpose than parkAtMinHeight(): it resets
   * state for a NEW session (a different folder may now be open), not a
   * same-session divider gesture — so, unlike ordinary gestures, it is
   * correct for this one lifecycle boundary to wipe `overrides`.
   *
   * overrides mutated only via reassignment (never .set/.delete).
   * Also clears the park so a parked drag-height from the
   * previous session doesn't cap the freshly reopened (content-fit) tree.
   */
  public collapseForReopen(): void {
    this.update(collapseAll);
  }

  isEditing(): boolean {
    return this.gate.isEditing;
  }

  /** "Show as pages", "Keep as files" or "Why?" on the hint row of folder `path`. */
  private runHintAction(button: HTMLElement, path: string): void {
    const d = this.derivations.at(path);
    if (d === null) return;
    const action = button.dataset.deriveAction;
    if (action === 'show') void this.derivations.set(path, { show: d.derivation_id });
    else if (action === 'keep') void this.derivations.set(path, 'keep');
    else if (action === 'why') this.menu?.showDerivationWhy(button, d);
  }

  // ── Keyboard Navigation ───────────────────────────────────────────────

  /**
   * Central keyboard handler for the file tree.
   *
   * Mounted on `document` via AbortSignal so it's torn down at unmount.
   * Filters by: (1) tree or editor must be involved, (2) isEditing() guard.
   *
   * Architecture: routes through keyToAction(event), then dispatches to
   * the appropriate selection mutation or side effect. All selection changes
   * go through mutateSelection() → selection.X() → notifySelectionChanged()
   * to preserve the selection contract.
   *
   * Every write is a transition through update() — never mutate
   * this.view.overrides in place inside this handler.
   */
  private handleKey = (event: KeyboardEvent): void => {
    // Skip while rename/create input is focused (isEditing() gate)
    if (this.isEditing()) return;
    // A focused hint-row button owns its own Enter/Space; the tree's would act on the selection.
    if ((event.target as Element | null)?.closest?.('.nav-hint-row')) return;

    // Skip if focus is neither inside the tree container nor the document body
    // (i.e., some unrelated part of the page has focus). But always handle
    // cross-pane shortcuts (Cmd+1/Cmd+2) regardless of where focus is.
    const action = keyToAction(event);
    if (!action) return;

    // Cross-pane focus is handled even when tree is not focused
    if (action.kind === 'focusPane') {
      event.preventDefault();
      if (action.pane === 'tree') {
        // Dispatch focus-tree event so editor-main can route; also focus directly
        document.dispatchEvent(new CustomEvent('editor:focus-tree', { bubbles: false }));
        this.focus();
      } else {
        document.dispatchEvent(new CustomEvent('editor:focus-editor', { bubbles: false }));
      }
      return;
    }

    // For non-cross-pane actions: only handle a key pressed inside the tree or
    // with nothing focused, so the tree never takes keystrokes meant for the
    // editor. Judged by the event's target, the element focused when the key
    // went down: an earlier handler (the heading committing on Enter) can drop
    // focus to the body before the key reaches this listener.
    const origin = event.target instanceof Node ? event.target : null;
    const pressedInTree = origin === null || origin === document || origin === document.body
      || origin === document.documentElement || this.container?.contains(origin) === true;
    if (!pressedInTree) return;

    switch (action.kind) {
      case 'moveFocus': {
        event.preventDefault();
        // Body rows only (excludes breadcrumb segs) for keyboard focus movement.
        const navIds = this.visibleEntryIds();
        if (navIds.length === 0) return;
        const currentFocusId = this.selection.focus;
        const currentIdx = currentFocusId !== null ? navIds.indexOf(currentFocusId) : -1;
        // When focus is not on a nav row (e.g., it's on a breadcrumb, or null),
        // ArrowDown/Home goes to index 0; ArrowUp/End goes to last index.
        // This avoids "jumping over" the first row when focus starts on breadcrumb.
        let nextIdx: number;
        if (currentIdx === -1) {
          if (action.direction === 'up' || action.direction === 'end') {
            nextIdx = navIds.length - 1;
          } else {
            nextIdx = 0;
          }
        } else {
          nextIdx = computeNextFocusIndex(currentIdx, navIds.length, action.direction);
        }
        if (nextIdx === -1) return;
        const nextId = navIds[nextIdx];

        // Cmd+Down special case focusOnly+down = open + focus editor
        if (action.focusOnly && action.direction === 'down') {
          const record = this.registry.getById(nextId);
          if (record && !record.isDir) {
            // Dispatch file-selected to open the file
            this.container?.dispatchEvent(new CustomEvent('editor:file-selected', {
              bubbles: true,
              detail: { filePath: record.path },
            }));
            // Dispatch focus-editor to move focus to the editor
            document.dispatchEvent(new CustomEvent('editor:focus-editor', { bubbles: false }));
          }
          // Move focus to next row (but keep selection)
          this.mutateSelection(sel => sel.setFocus(nextId));
          this.render.updateTabIndex(nextId);
          return;
        }

        if (action.focusOnly) {
          // Cmd+Up/Down: move focus without touching selection
          this.mutateSelection(sel => sel.setFocus(nextId));
        } else if (action.extend) {
          // Shift+Arrow: extend range from anchor to new focus
          // setRange uses visibleEntryIds() for full visible order (incl. breadcrumbs)
          // but both anchor and target are in the nav rows, so result is correct.
          this.mutateSelection(sel => sel.setRange(nextId, this.visibleEntryIds()));
        } else {
          // Plain Arrow: set single selection + focus
          this.mutateSelection(sel => sel.setSingle(nextId));
        }
        this.render.updateTabIndex(nextId);
        break;
      }

      case 'expand': {
        event.preventDefault();
        this.handleKeyExpand();
        break;
      }

      case 'collapse': {
        event.preventDefault();
        this.handleKeyCollapse();
        break;
      }

      case 'collapseAll': {
        // Cmd/Ctrl+Shift+←: park the sash at breadcrumb-only height (rule 2 —
        // overrides untouched, nothing is wiped).
        event.preventDefault();
        this.parkAtMinHeight();
        break;
      }

      case 'expandAllSiblings': {
        // * (asterisk): WAI-ARIA APG tree pattern — expand all sibling closed
        // nodes at the same level as the currently focused treeitem.
        // https://www.w3.org/WAI/ARIA/apg/patterns/treeview/
        event.preventDefault();
        this.handleKeyExpandAllSiblings();
        break;
      }

      case 'selectAll': {
        event.preventDefault();
        const allNavIds = this.visibleEntryIds();
        if (allNavIds.length === 0) return;
        // Select all via setSingle + setRange from first to last
        this.mutateSelection(sel => {
          sel.setSingle(allNavIds[0]);
          sel.setRange(allNavIds[allNavIds.length - 1], allNavIds);
        });
        break;
      }

      case 'clear': {
        // Layered Esc per Finder/VS Code: first Esc clears selection (if
        // any); a second Esc lets the event bubble to editor-main, which
        // collapses the tree. If selection was already empty, let Esc fall
        // through immediately so it still collapses on first press.
        if (this.selection.size === 0 && this.selection.focus === null) return;
        event.preventDefault();
        event.stopPropagation();
        this.mutateSelection(sel => sel.clear());
        break;
      }

      case 'toggleSelection': {
        event.preventDefault();
        const focusId = this.selection.focus;
        if (focusId === null) return;
        this.mutateSelection(sel => sel.toggle(focusId));
        break;
      }

      case 'rangeToFocus': {
        // Shift+Space: extend selection from anchor to current focus (the WAI-ARIA APG multi-select tree pattern)
        event.preventDefault();
        const focusId = this.selection.focus;
        if (focusId === null) return;
        // setRange extends from anchor (most recently setSingle'd) to focusId
        this.mutateSelection(sel => sel.setRange(focusId, this.visibleEntryIds()));
        break;
      }

      case 'open': {
        // Space: open the focused entry. Files open in the editor; folders
        // select + expand in place (reveal children), mirroring a name-zone
        // click. Enter remains bound to rename.
        const focusId = this.selection.focus;
        if (focusId === null) return;
        const record = this.registry.getById(focusId);
        if (!record) return;
        event.preventDefault();
        if (record.isDir) {
          this.selectFolder(record.path, () => this.toggleFolderOpen(record.path));
        } else {
          this.container?.dispatchEvent(new CustomEvent('editor:file-selected', {
            bubbles: true,
            detail: { filePath: record.path },
          }));
        }
        // focus stays in tree (no editor:focus-editor dispatch)
        break;
      }

      case 'rename': {
        event.preventDefault();
        // Multi-row rename is out of scope per spec — only act on single focus
        if (this.selection.size !== 1) return;
        const focusId = this.selection.focus;
        if (focusId === null) return;
        const record = this.registry.getById(focusId);
        if (!record) return;
        this.renameEntry(record);
        break;
      }

      case 'delete': {
        event.preventDefault();
        // One or many: the question (child count for a folder, "N items" for
        // a batch) is derived from the selection inside deleteEntries.
        void this.ops.deleteEntries(this.selectionPaths());
        break;
      }

      case 'contextMenu': {
        // Shift+F10 / ContextMenu key: open the focused row's context menu at
        // the row (keyboard equivalent of right-click, WAI-ARIA), with roving
        // focus on the first item.
        event.preventDefault();
        const focusId = this.selection.focus;
        if (focusId === null) return;
        const record = this.registry.getById(focusId);
        if (!record) return;
        const r = this.render.rowFor(focusId)?.getBoundingClientRect();
        const pos = r
          ? { clientX: r.left + Math.min(24, r.width / 2), clientY: r.bottom }
          : { clientX: 0, clientY: 0 };
        this.openContextMenuAt(record.path, pos, { focusFirst: true });
        break;
      }

      case 'typeAhead': {
        this.handleTypeAhead(action.char);
        break;
      }
    }
  };

  /** Expand: → on collapsed folder expands it; → on expanded folder goes to first child; → on file = ↓. */
  private handleKeyExpand(): void {
    const focusId = this.selection.focus;
    if (focusId === null) return;
    const record = this.registry.getById(focusId);
    if (!record) return;

    if (record.isDir) {
      // Is it already expanded? Check overrides
      if (this.isOpen(record.path)) {
        // Already expanded: move focus to first child
        // Re-query nav rows AFTER checking state (render may not have run yet)
        const navIds = this.visibleEntryIds();
        const folderIdx = navIds.indexOf(focusId);
        // First child is the row immediately after the folder in nav order
        if (folderIdx !== -1 && folderIdx + 1 < navIds.length) {
          const firstChildId = navIds[folderIdx + 1];
          this.mutateSelection(sel => sel.setSingle(firstChildId));
          this.render.updateTabIndex(firstChildId);
        }
      } else {
        // Collapsed: expand it (same as click handler)
        this.toggleFolderOpen(record.path);
        // After expand, update focus to keep it on the folder
        this.mutateSelection(sel => sel.setFocus(focusId));
      }
    } else {
      // File: move focus to next row (like ↓)
      const navIds = this.visibleEntryIds();
      const currentIdx = navIds.indexOf(focusId);
      if (currentIdx === -1) return;
      const nextIdx = computeNextFocusIndex(currentIdx, navIds.length, 'down');
      if (nextIdx === -1 || nextIdx === currentIdx) return;
      this.mutateSelection(sel => sel.setSingle(navIds[nextIdx]));
      this.render.updateTabIndex(navIds[nextIdx]);
    }
  }

  /** Collapse: ← on expanded folder collapses it; ← on collapsed folder / file = move to parent. */
  private handleKeyCollapse(): void {
    const focusId = this.selection.focus;
    if (focusId === null) return;
    const record = this.registry.getById(focusId);
    if (!record) return;

    if (record.isDir && this.isOpen(record.path)) {
      // Expanded folder: collapse it
      this.toggleFolderOpen(record.path);
      this.mutateSelection(sel => sel.setSingle(focusId));
    } else {
      // Collapsed folder or file: move to parent folder. record.path is
      // registry-domain (relative); for an entry at the project root the
      // parent is PROJECT_ROOT_REL (''), which has no body row to focus.
      const parentPath = parentDirOf(record.path);
      if (!parentPath) return;
      const parentId = this.registry.getByPath(parentPath);
      if (parentId !== null) {
        this.mutateSelection(sel => sel.setSingle(parentId));
        this.render.updateTabIndex(parentId);
      }
    }
  }

  /**
   * * (asterisk): WAI-ARIA APG tree pattern — expand all sibling closed nodes
   * at the same level as the currently focused treeitem.
   *
   * Implementation: find the focused entry's parent directory, enumerate its
   * direct children that are directories and currently collapsed (not 'open'
   * in `overrides`), and open each via `openFolders`.
   *
   * Folders that are already expanded are left untouched (idempotent).
   * If no focus exists, the action is a no-op.
   */
  private handleKeyExpandAllSiblings(): void {
    const focusId = this.selection.focus;
    if (focusId === null) return;
    const record = this.registry.getById(focusId);
    if (!record) return;

    // Derive the parent path of the focused entry (root-level: no-op).
    const parentPath = parentDirOf(record.path);
    if (!parentPath) return;

    // Get all sibling entries (direct children of parent)
    const siblings = this.children(parentPath) ?? [];
    if (siblings.length === 0) return;

    // Open every collapsed sibling directory. openFolders returns the same
    // view when nothing needed opening: identity = "something opened".
    const before = this.view;
    this.update(v => openFolders(v, siblings.filter(sib => sib.isDir).map(sib => sib.id)));
    if (this.view !== before) {
      this.render.render();
      this.render.applyWrapperHeight();
    }
  }

  /** Type-ahead: extend the prefix (reset on a pause) and jump to the next row matching it. */
  private handleTypeAhead(char: string): void {
    const prefix = this.typeAhead.push(char);

    const navIds = this.visibleEntryIds();
    const rows = navIds.map(id => {
      const record = this.registry.getById(id);
      return { id, name: record?.path.split('/').pop() ?? '' };
    });

    const currentFocusId = this.selection.focus;
    const currentIdx = currentFocusId !== null ? navIds.indexOf(currentFocusId) : -1;
    const fromIdx = currentIdx === -1 ? 0 : currentIdx;

    const matchedId = matchTypeAhead(rows, prefix, fromIdx);
    if (matchedId !== null) {
      this.mutateSelection(sel => sel.setSingle(matchedId));
      this.render.updateTabIndex(matchedId);
    }
  }

  // ── Drag Handle Setup (called from editor-main) ─────────────────────

  setupDragHandle(handle: HTMLElement, wrapper: HTMLElement): void {
    this.dragHandleEl = handle;
    this.render.wrapper = wrapper;
    const signal = this.abortController?.signal;

// ── The divider is a SASH ──────────────────────────────────
    //
    // Height follows the cursor 1:1; `overrides` is
    // touched ONLY by drag-fill (rule 3, drag DOWN past content-fit), never by
    // drag UP (rule 6, clip-not-close) and never by release (rule 5).
    //
    //   pointermove (down) → REVEAL rows already open (live height tracks the
    //     cursor, no state change) then, once the cursor pushes past
    //     content-fit by DRAG_FILL_SLACK_PX, FILL: open the next CLOSED
    //     ancestor level nearest the active path first (fillOrder()). One
    //     level per crossing; a big jump can open several in one pointermove.
    //     Once every ancestor is open the ceiling is content-fit — dragging
    //     further rubber-bands (rule 4).
    //   pointermove (up)   → CLIP: live-resize down to minH; rows scroll/clip
    //     under the shorter panel. `overrides` is never touched, so this can
    //     never close a folder (rule 6).
    //   pointerup           → commits NOTHING but the height (rule 5): no
    //     threshold, no snap-back, no epilogue that can disagree with the
    //     gesture. Whatever height the drag landed at becomes the new park.
    //
    // Double-clicking the divider parks at minimum height instead (rule 2).
    let startY = 0;
    // The panel height at pointerdown — the live-resize baseline.
    let startHeight = 0;
    // The height the gesture has applied so far; release parks it (rule 5).
    // Tracked here, not read back out of `style.height`: the model is the
    // baseline and the drag is the only writer between down and up.
    let liveHeight = 0;
    const applyLive = (px: number): void => {
      liveHeight = px;
      this.render.setWrapperHeight(px);
    };
    // `dragging` (not hasPointerCapture) is the authoritative gate — it stops a
    // bare hover from acting if pointer capture ever leaks past a gesture. Set
    // on pointerdown, cleared in endDrag.
    let dragging = false;
    let activePointerId: number | null = null;

    const onResize = () => {
      this.readChromeHeights();
      // Keep the stored park honest after a window resize — clamp between
      // minH (navH(0)) and the new floor so the parked value can neither exceed
      // available space nor fall below the breadcrumb-only minimum.
      if (this.view.park !== null) {
        const park = Math.max(navH(0), Math.min(this.view.park, this.computeFloor()));
        this.update(v => parkAt(v, park));
      }
      this.render.applyWrapperHeight();
    };

    signal?.addEventListener('abort', () => {
      wrapper.classList.remove('no-transition');
      window.removeEventListener('resize', onResize);
    });

    window.addEventListener('resize', onResize, { signal });

    handle.addEventListener('pointerdown', (e: PointerEvent) => {
      startY = e.clientY;
      this.readChromeHeights();
      // Baseline for the live-resize: the height the model says the surface
      // has, which is what applyWrapperHeight last wrote.
      startHeight = liveHeight = this.render.surfaceHeight();
      dragging = true;
      activePointerId = e.pointerId;
      handle.classList.add('dragging');
      // Track the cursor 1:1 during the drag (no eased height transition).
      wrapper.classList.add('no-transition');
      handle.setPointerCapture(e.pointerId);
      e.preventDefault();
    }, { signal });

    // Double-click TOGGLES park/reveal. Was detected via
    // `e.detail >= 2` on the pointerdown handler above, but Chromium's
    // PointerEvent.detail is always 0 regardless of click count (WebKit's
    // does mirror MouseEvent's count) — dead on every Chromium-backed webview,
    // WebView2 (the real Windows app) included. `dblclick` reports the count
    // correctly in both engines. The pointerdown/pointerup pair each click of
    // a double-click still delivers just re-parks at the unchanged height
    // (no pointermove between them) — a harmless no-op ahead of the toggle.
    handle.addEventListener('dblclick', (e: MouseEvent) => {
      if (isParkedHeight(this.view.park)) this.revealFromPark();
      else this.parkAtMinHeight();
      e.preventDefault();
    }, { signal });

    handle.addEventListener('pointermove', (e: PointerEvent) => {
      // Only act while a genuine drag is in progress for THIS pointer.
      if (!dragging || e.pointerId !== activePointerId) return;

      const minH = navH(0);
      const dy = e.clientY - startY;
      const targetH = startHeight + dy;

      if (dy > 0) {
        // DRAG DOWN = reveal, then fill (rule 3). Open the next CLOSED
        // ancestor level (nearest the active path first) each time the target
        // height pushes past the current content-fit by DRAG_FILL_SLACK_PX.
        // Loops so one big jump can open several levels in a single move.
        let contentHeight = navH(this.render.renderedRowCount());
        while (targetH > contentHeight + DRAG_FILL_SLACK_PX) {
          const order = this.fillOrder();
          const next = order.find((p) => !this.isOpen(p));
          if (next === undefined) {
            // Every gesture leaves a trace, even a no-op one. A non-empty
            // fillOrder() with everything already open is the ordinary
            // terminal state (rule 4 — rubber-band, nothing degenerate to
            // report). An EMPTY fillOrder() means there was never anything
            // openable to begin with — the structural sink Finding 2 flagged
            // (this happens less often but is not eliminated,
            // e.g. before any file is active) — so that case gets its own line.
            if (order.length === 0) {
              logGesture(`gesture=drag-fill outcome=none cause=empty-dirIndex`);
            }
            break; // rule 4: nothing left to open — rubber-band
          }
          this.update(v => openFolders(v, this.folderIds([next])));
          this.render.render();
          const rows = this.render.renderedRowCount();
          contentHeight = navH(rows);
          const cause = this.render.noRowsCause(next);
          if (cause !== null) {
            logGesture(`gesture=drag-fill outcome=no-rows path=${next} cause=${cause}`);
          } else {
            logGesture(`gesture=drag-fill outcome=opened-level path=${next} rows=${rows}`);
          }
        }
        applyLive(Math.max(minH, Math.min(targetH, contentHeight)));
        return;
      }

      // DRAG UP = clip, never close (rule 6). Live-resize down toward minH;
      // rows scroll/clip under the shorter panel. `overrides` untouched.
      const contentHeight = navH(this.render.renderedRowCount());
      applyLive(Math.max(minH, Math.min(targetH, contentHeight)));
    }, { signal });

    // End-of-drag cleanup. ONLY pointerup + pointercancel end the drag — NOT
    // lostpointercapture: WebKit can fire that mid-gesture, which used to clear
    // `dragging` after the first move and silently kill the drag.
    const endDrag = (): void => {
      if (!dragging) return; // ignore stray ends (e.g. after a double-click park)
      dragging = false;
      if (activePointerId !== null && handle.hasPointerCapture?.(activePointerId)) {
        handle.releasePointerCapture(activePointerId);
      }
      activePointerId = null;
      handle.classList.remove('dragging');
      // Re-enable the transition AFTER the park height write below settles —
      // release commits nothing but the height (rule 5): no threshold, no
      // snap-back, no epilogue that can disagree with the gesture.
      wrapper.classList.remove('no-transition');
      this.update(v => parkAt(v, liveHeight));
      logGesture(`gesture=drag-release outcome=parked height=${liveHeight}`);
    };
    handle.addEventListener('pointerup', endDrag, { signal });
    handle.addEventListener('pointercancel', endDrag, { signal });

    this.readChromeHeights();
    this.render.applyWrapperHeight();
  }

  /**
   * Openable ancestor levels of the active path, NEAREST FIRST (parent, then
   * upward, root last) — the fill order rule 3 opens in. Mirrors spineBody's
   * `levels` construction in tree/spine-projection.ts exactly (folder-valued leaf first,
   * then ancestors length-2..0) so drag-fill can never disagree with what the
   * body would render once a level opens. Kept in sync with spineBody by
   * comment, not by shared code, because spineBody is a pure function over
   * caller-supplied predicates and has no FileTree instance to call back into.
   */
  private fillOrder(): string[] {
    const order: string[] = [];
    const leaf = this.view.anchor[this.view.anchor.length - 1];
    if (this.view.anchor.length > 0 && this.isDirectory(leaf)) order.push(leaf);
    for (let i = this.view.anchor.length - 2; i >= 0; i--) {
      const p = this.view.anchor[i];
      if (this.isDirectory(p)) order.push(p);
    }
    return order;
  }

  // ── Navigation ──────────────────────────────────────────────────────

  /** `filePath` is registry-domain (project-root-relative). The activePath
   *  array carries the same domain.
   *
   *  Re-point the breadcrumb at `filePath`. Its caller, highlight(), reaches
   *  here in two cases: `isAtMinHeight()` (sash parked at breadcrumb-only) or
   *  `opts.forceAnchor` (an external navigation overriding the frozen anchor
   *  above minimum height). See highlight().
   *
   *  This function ONLY ever touches `activePath` (breadcrumb) — it never
   *  mutates the LIVE `overrides` map (the open-folder set the body renders).
   *  That is deliberate and safety-relevant: reshaping the live `overrides` to
   *  pop the new file's ancestors open is exactly the layout thrash the
   *  frozen-anchor design eliminates, so calling this at any height only moves
   *  the breadcrumb + re-renders against the UNCHANGED `overrides`. render() is idempotent and
   *  scroll-position-preserving regardless of height, so there is no
   *  height-specific render side effect to guard against either.
   *
   *  The forest is never wiped, so there is nothing to remember here; the
   *  sash's own drag-fill opens ancestors on demand instead
   *  (setupDragHandle → fillOrder()). */
  private navigateTo(filePath: string): void {
    this.update(v => anchorAt(v, filePath));
    this.render.render();
    this.render.applyWrapperHeight();
  }

  /** The registry's `isDir`, the root (upserted at mount, before the first
   *  listing, so a failed boot refresh still finds it) and the create flow's
   *  temp row included. */
  private isDirectory(path: string): boolean {
    return this.registry.recordAt(path)?.isDir ?? false;
  }

  /** A folder's own `hidden` count (`EntryRecord.hidden`) — how many of its
   *  immediate children moss's curated tree filtered out. `0` for an unknown
   *  path, matching `isDirectory`'s default. Feeds `stateRowKind` so a
   *  filtered-to-nothing folder reads differently from a genuinely empty one. */
  private hiddenCount(path: string): number {
    return this.registry.recordAt(path)?.hidden ?? 0;
  }

  /**
   * Display name for a path segment. Paths are project-root-relative;
   * the project root is PROJECT_ROOT_REL ('') — fall back to
   * the folder name from `this.projectRoot` so breadcrumbs still render
   * 'my-blog' instead of empty.
   */
  private getName(path: string): string {
    return displayName(path, this.projectRoot);
  }

  // ── Nav-Surface Height ──────────────────────────────────────────────

  /** Read live chrome heights from the DOM. Called at drag-start and on resize. */
  private readChromeHeights(): void {
    this.chromeDividerH = document.getElementById('divider')?.offsetHeight ?? 0;
  }

  /** Maximum nav-surface height that leaves at least MIN_EDITOR_H for the editor. */
  private computeFloor(): number {
    return maxTreeHeight(
      window.innerHeight,
      this.chromeDividerH,
      MIN_EDITOR_H,
    );
  }

  // ── Event Handlers ────────────────────────────────────────────────────

  /**
   * Toggle a BREADCRUMB folder at `activePath[index]` (root chevron = index 0 =
   * PROJECT_ROOT_REL → reveals root's children). Opening a breadcrumb folder
   * also opens every ancestor up to it (activePath[0..index]) so the open
   * forest actually reaches it — a breadcrumb ancestor is reachable by
   * definition (it's on the active file's path), and the forest only renders a
   * folder's children when the chain from the root is open. Closing collapses
   * just that folder.
   */
  private handleToggleTop(index: number): void {
    const path = this.view.anchor[index];
    // A breadcrumb chevron click, closed or open, is
    // an ordinary click-territory toggle — no restore special case. The forest
    // is never wiped (rule 2), so opening the root here is the SAME whether or
    // not it was ever "closed" before; there is nothing to restore.
    const gesture = index === 0 ? 'open-root' : 'breadcrumb-toggle';
    // PARITY WITH THE PAINTED POSE: parked → the chevron paints
    // CLOSED, so the click must OPEN. This is what makes the chevron the
    // one-click way back from every park gesture.
    if (!isParkedHeight(this.view.park) && this.isOpen(path)) {
      // CLOSE a breadcrumb folder: keep any parked drag-height intact so the
      // panel shrinks only as far as min(park, contentH).
      this.setFolderAt(path, 'default');
      this.render.render();
      this.render.applyWrapperHeight();
      logGesture(`gesture=${gesture} outcome=closed path=${path}`);
      return;
    }
    // OPEN a breadcrumb folder: this is a breadcrumb expand,
    // which always returns to content-fit GROW — clear the park. Tree-body
    // opens (toggleFolderOpen) instead FREEZE the park to cap+scroll; the
    // two open paths are intentionally opposite.
    const chain = this.folderIds(this.view.anchor.slice(0, index + 1));
    this.update(v => openFolders(parkAt(v, null), chain));
    this.render.render();
    this.render.applyWrapperHeight();
    const rows = this.render.renderedRowCount();
    const cause = this.render.noRowsCause(path);
    if (cause !== null) {
      logGesture(`gesture=${gesture} outcome=no-rows path=${path} cause=${cause}`);
    } else {
      logGesture(`gesture=${gesture} outcome=opened path=${path} rows=${rows}`);
    }
  }

  /**
   * Toggle a TREE-BODY folder row: flip its open/closed state in `overrides`
   * and re-render. Open when absent or closed; close when open. The row is
   * already reachable (it only rendered because its parent chain is open), so
   * a plain flip suffices — the breadcrumb counterpart is handleToggleTop. The
   * body re-renders the open forest and the panel height follows the new row
   * count. The single expansion mutation (a transition through
   * update(), never .set/.delete).
   *
   * In-tree expand freezes the panel at the current content height
   * BEFORE the expand so the panel does not grow (new rows scroll under). This
   * makes in-tree expand cap+scroll at the current size even before any drag.
   * Freeze only on OPEN — closing a folder (intent='default') lets the panel
   * shrink naturally via the min() in applyWrapperHeight (Rule 4).
   */
  private toggleFolderOpen(path: string): void {
    const intent = this.isOpen(path) ? 'default' : 'open';
    if (intent === 'open' && this.view.park === null) {
      // Freeze at the current rendered content height so the in-tree expand
      // scrolls rather than growing. Compute BEFORE applying the override so
      // the row count reflects the pre-expand state.
      const park = navH(this.render.renderedRowCount());
      this.update(v => parkAt(v, park));
    }
    treeTrace(`toggleFolderOpen path=${path} intent=${intent}`);
    this.setFolderAt(path, intent);
    this.render.render();
    this.render.applyWrapperHeight();
    if (intent === 'default') {
      logGesture(`gesture=click-toggle outcome=closed path=${path}`);
      return;
    }
    const rows = this.render.renderedRowCount();
    const cause = this.render.noRowsCause(path);
    if (cause !== null) {
      logGesture(`gesture=click-toggle outcome=no-rows path=${path} cause=${cause}`);
    } else {
      logGesture(`gesture=click-toggle outcome=opened path=${path} rows=${rows}`);
    }
  }

  private handleFileClick(filePath: string, event?: MouseEvent): void {
    const id = this.registry.getByPath(filePath);
    if (id === null) {
      // Path not in registry yet — fall through to legacy dispatch path.
      this.container?.dispatchEvent(
        new CustomEvent('editor:file-selected', {
          bubbles: true,
          detail: { filePath },
        }),
      );
      return;
    }

    const action = resolveClickAction(event);
    if (action !== 'plain') {
      this.selectByClick(id, action);
      return;
    }
    // Plain click: existing behavior — select + open in editor.
    this.selection.setSingle(id);
    this.notifySelectionChanged();
    this.container?.dispatchEvent(
      new CustomEvent('editor:file-selected', {
        bubbles: true,
        detail: { filePath },
      }),
    );
  }

  /** A modifier-click's selection change, for file and folder rows alike. */
  private selectByClick(id: EntryId, action: Exclude<ClickAction, 'plain'>): void {
    if (action === 'toggle') this.selection.toggle(id);
    else if (action === 'setRange') this.selection.setRange(id, this.visibleEntryIds());
    else this.selection.addRange(id, this.visibleEntryIds());
    this.notifySelectionChanged();
  }

  /**
   * Select a folder (name-zone click / Space). With a home file, open it —
   * the editor loads it and PreviewFollower resolves /folder/. Without one,
   * highlight the folder and ask the editor for a "create folder page" CTA:
   * resolve_url_for_file returns null for a home-less folder and the preview
   * server 404s, so there is no URL to navigate to. Callers handle
   * modifier-clicks (multi-select) BEFORE calling this.
   *
   * Note: opening a home file routes through handleFileClick -> editor's
   * loadFile -> fileTree.highlight -> navigateTo, which only
   * updates `activePath` and PRESERVES `overrides` — the tree stays open across
   * a file open. selectFolder itself never mutates `overrides`, so this respects
   * the expansion rule.
   */
  /**
   * Handle a click on a folder's NAME zone.
   *
   * A name-click EXPANDS THE FOLDER IN PLACE (reveals its children below it,
   * like VS Code / Finder) and selects it — it does NOT navigate into the
   * folder, which would reset `overrides`, leave the body EMPTY and relocate
   * the folder up into the breadcrumb. Opening a folder's home page is a separate, deliberate action (its inline home-file
   * row), not a side effect of clicking the folder.
   *
   * `toggle` is the same expansion toggle the row's chevron uses (handleToggleTop
   * for breadcrumb ancestors, toggleFolderOpen for tree rows) so name and chevron
   * are interchangeable channels into the one expansion model.
   */
  private selectFolder(folderPath: string, toggle?: () => void): void {
    const folderId = this.registry.getByPath(folderPath);
    if (folderId !== null) {
      this.selection.setSingle(folderId);
      this.notifySelectionChanged();
    }
    // Expand in place (reveal children). If no toggle is supplied (e.g. the
    // keyboard 'open' path), fall back to selecting only.
    toggle?.();
    // A plain tree folder click (name zone or chevron) ONLY toggles expand/
    // collapse and updates the selection. It does NOT change the editor surface
    // — the "create a folder page" CTA (editor:folder-page-missing) must come
    // exclusively from preview/URL navigation (applyResolvedNode → decideEditorSurface).
    // Dispatching here caused the CTA to appear on every plain tree folder click,
    // even when the user simply wanted to expand the folder.
  }

  /**
   * Show `folderRel` as the place the user is now: breadcrumb, editor surface
   * (the gated `dispatchFolderSurface`) and selection — the one path for both
   * forced navigations, the delete fallback (tree/ops.ts) and folder creation.
   *
   * The breadcrumb lands where a click on the folder's home would put it —
   * the home as the leaf, else the folder — and is rendered HERE
   * Editor-main answers the dispatch by loading a file, which
   * under the frozen anchor never moves the breadcrumb, so a delete used to
   * re-point the anchor unrendered and leave the deleted file's crumb up.
   */
  private showFolder(folderRel: string, opts?: { justCreated?: boolean }): void {
    this.navigateTo(this.render.homeEntry(folderRel)?.path ?? folderRel);
    this.dispatchFolderSurface(folderRel, opts);
    this.selectFolder(folderRel);
  }

  /**
   * Dispatch the editor surface for `folderRel`, GATED by `homeEntry()`:
   *   • folder WITH a home file → `editor:file-selected` (opens the home; the
   *     editor loads its source and PreviewFollower resolves the folder URL).
   *   • folder WITHOUT a home → `editor:folder-navigate` (editor-main shows the
   *     "create folder page" CTA and clears any stale surface).
   * This is the SINGLE gated surface path — shared by the delete-fallback
   * (tree/ops.ts applyDeleteLocally) and folder creation. It is NOT wired to plain tree/
   * breadcrumb folder clicks or the keyboard 'open' key: those only expand/
   * collapse + select (selectFolder), so browsing the tree never swaps the
   * editor pane. Dispatch only: it does NOT touch selection, `activePath`, or
   * `overrides`.
   */
  private dispatchFolderSurface(folderRel: string, opts?: { justCreated?: boolean }): void {
    const homeFile = this.render.homeEntry(folderRel)?.path ?? null;
    if (homeFile !== null) {
      this.container?.dispatchEvent(new CustomEvent('editor:file-selected', {
        bubbles: true,
        detail: { filePath: homeFile },
      }));
    } else {
      const folderName = this.getName(folderRel);
      const folderPath = toAbsolute(folderRel, this.projectRoot);
      // `justCreated` tells the shell the folder's page CANNOT be in the build
      // output yet, so it covers the 404 the preview is about to land on
      // instead of showing it. The delete-fallback caller leaves it unset —
      // that folder's page already exists.
      this.container?.dispatchEvent(new CustomEvent('editor:folder-navigate', {
        bubbles: true,
        detail: { folderPath, folderName, justCreated: opts?.justCreated === true },
      }));
    }
  }

  /** Expand/collapse the folder `row` stands for. */
  private toggleRow(row: TreeRow): void {
    const bcIndex = row.el.dataset.bcIndex;
    if (bcIndex !== undefined) this.handleToggleTop(Number(bcIndex));
    else this.toggleFolderOpen(row.path);
  }

  /**
   * What a plain click on a row does. One policy for four row kinds that
   * previously each carried their own copy of it.
   */
  private clickRow(row: TreeRow, e: MouseEvent): void {
    const { el, path } = row;
    if (this.isDirectory(path)) {
      const folderId = this.registry.getByPath(path);
      const action = resolveClickAction(e);
      if (folderId !== null && action !== 'plain') {
        // A modifier-click changes the selection without expanding or navigating.
        this.selectByClick(folderId, action);
        return;
      }
      // Plain click on the name zone: select + toggle in place, the same
      // toggle the chevron does. Does NOT navigate.
      this.selectFolder(path, () => this.toggleRow(row));
      return;
    }
    if (el.dataset.bcIndex !== undefined) {
      // The breadcrumb's file leaf. Under the frozen anchor it
      // can DIVERGE from the active file; clicking it re-opens the anchor
      // only then, compared by EntryId because path drifts on rename.
      const leafId = this.registry.getByPath(path);
      if (leafId !== null && leafId === this.activeEntryId) return;
    }
    this.handleFileClick(path, e);
  }

  /** A folder is open iff its id maps to 'open' in `overrides` A
   *  path the registry does not know is closed. */
  private isOpen(path: string): boolean {
    const id = this.registry.getByPath(path);
    return id !== null && this.view.overrides.get(id) === 'open';
  }

  /** The path→id seam for writes. Gesture paths come from dirIndex, which flattenTree pairs with the registry: no id is a bug worth a trace. */
  private folderIds(paths: string[]): EntryId[] {
    const ids: EntryId[] = [];
    for (const path of paths) {
      const id = this.registry.getByPath(path);
      if (id === null) treeTrace(`folderIds: no registry id for path=${path}`);
      else ids.push(id);
    }
    return ids;
  }

  /** One folder's intent, by path ('default' drops the override — a folder with no override is closed). */
  private setFolderAt(path: string, intent: 'open' | 'default'): void {
    const [id] = this.folderIds([path]);
    if (id !== undefined) this.update(v => setFolder(v, id, intent));
  }

  /** Finder-style free placeholder (`untitled`, `untitled 2`, …): a temp row sharing a real entry's path would take its id, and teardown would retire it. */
  private freePlaceholder(folderPath: string, type: 'file' | 'folder'): string {
    const ext = type === 'file' ? '.md' : '';
    for (let n = 1; ; n++) {
      const name = n === 1 ? `untitled${ext}` : `untitled ${n}${ext}`;
      if (this.registry.getByPath(childPath(folderPath, name)) === null) return name;
    }
  }

  /**
   * A folder's children in display order, or null when dirIndex has no
   * entry for it at all — the distinction tree/spine-projection.ts's spineBody() needs
   * for the empty and unavailable state rows.
   */
  private children(folderPath: string): EntryRecord[] | null {
    const id = this.registry.getByPath(folderPath);
    const ids = id === null ? undefined : this.dirIndex.get(id);
    if (ids === undefined) return null;
    const out: EntryRecord[] = [];
    for (const c of ids) {
      const r = this.registry.getById(c);
      if (r) out.push(r);
    }
    return out;
  }

  /**
   * Would renaming this folder also rename a file INSIDE it? A folder whose
   * home file is named after the folder has that file carried along by the
   * backend (`posts/posts.md` → `writing/writing.md`; see
   * `rename_self_named_home` in the host editor commands). A
   * path-keyed prediction cannot know that — it keeps each descendant's own
   * basename, so it would put `writing/posts.md` on screen, a path that
   * exists nowhere, for the whole rewrite window, and then swap it silently.
   *
   * So we decline to predict this one rename rather than mirror the backend's
   * stem / lang-suffix / index-stem policy on this side of the seam, where
   * the two copies would be free to drift. The user gets the pre-prediction
   * behaviour: correct, just not instant.
   *
   * Deliberately over-eager: any `<folder>.*.md` counts, not only the exact
   * stems the backend carries. Declining to predict costs a wait; predicting
   * wrong shows a lie.
   */
  private carriesSelfNamedHome(path: string): boolean {
    // Names compared in one Unicode form: the folder and its child may have
    // been listed in different spellings, and a miss here answers "no
    // self-named home", i.e. predict anyway. A guard whose job is to DECLINE
    // must not fail open on a spelling difference.
    const key = (raw: string) => normalizePath(raw).toLowerCase();
    const dir = this.registry.recordAt(path);
    if (dir === null || !dir.isDir) return false;
    const base = key(baseName(dir.path));
    return (this.children(dir.path) ?? []).some(e => {
      if (e.isDir) return false;
      const name = key(baseName(e.path));
      return name.startsWith(`${base}.`) && name.endsWith('.md');
    });
  }

  /** The canonical rename flow (tree/edit-session.ts): editor-main's CM6
   *  heading rename and the tests reach it here; the inline gesture reaches
   *  it inside the session. Absolute or relative paths. */
  public renameThroughTree(oldPath: string, newPath: string): Promise<void> {
    return this.editSession.renameThroughTree(oldPath, newPath);
  }

  // ── Rename ────────────────────────────────────────────────────────

  /** Start the inline rename on `entry`'s rendered row, if it has one. */
  private renameEntry(entry: EntryRecord): void {
    const seg = this.render.rowFor(entry.id);
    if (seg) this.editSession.startRename(seg, entry);
  }

  // ── Context menu entry + selection paths ─────────────────────────

  /**
   * The ONE way a row's context menu opens — right-click and Shift+F10 both
   * land here. Finder convention: a target outside the current selection
   * collapses the selection to itself before the menu shows, so the menu
   * describes what the user pointed at; tree/menu.ts then reads the
   * selection's size to pick the single or the batch menu.
   */
  private openContextMenuAt(
    path: string,
    pos: { clientX: number; clientY: number },
    opts?: { focusFirst?: boolean },
    surface = 'tree-row',
  ): void {
    const targetId = this.registry.getByPath(path);
    if (targetId !== null && !this.selection.has(targetId)) {
      this.mutateSelection(sel => sel.setSingle(targetId));
    }
    if (this.isDirectory(path)) this.menu?.showFolderCtx(pos, path, opts, surface);
    else this.menu?.showFileCtx(pos, path, opts, surface);
  }

  /**
   * Return the filesystem paths for all entries currently in the selection,
   * resolved via EntryRegistry. Paths for which the registry has no record
   * are silently omitted (shouldn't happen; guards against stale IDs).
   */
  private selectionPaths(): string[] {
    const snap = this.selection.snapshot();
    const paths: string[] = [];
    for (const id of snap.selected) {
      const record = this.registry.getById(id);
      if (record) paths.push(record.path);
    }
    return paths;
  }

  // ── Create New Item ─────────────────────────────────────────────

  // Public: the ＋ menu's "New folder" (create-flow.ts) shares the inline-name
  // input with the context-menu path — for folders the name IS the content.
  // The anchor policy — where the breadcrumb points while the name is typed
  // and what opens once the entry lands — is here; the input, the IPC and the
  // session's lifetime are the edit session's (tree/edit-session.ts).
  public async startCreate(
    folderPath: string,
    type: 'file' | 'folder',
    opts?: { templateId?: string },
  ): Promise<void> {
    if (!this.gate.canStartGesture) return;

    // Point the breadcrumb at the folder: its ancestor chain, whether it was
    // already on the active path (this trims to it) or is a sibling or
    // descendant (this builds the path to it).
    this.update(v => anchorAt(v, folderPath));

    // The temp row is a registry entry for the length of its session, and a
    // child of folderPath in dirIndex so the open forest renders it.
    // discardTempEntry retires it, by this id.
    const tempName = this.freePlaceholder(folderPath, type);
    // childPath, not string concat: at the root that minted '/untitled.md'.
    const tempPath = childPath(folderPath, tempName);
    const tempId = this.registry.upsert(tempPath, type === 'folder');
    this.refresh.adopt(folderPath, tempId);

    // Append temp path so folderPath is no longer the terminal breadcrumb
    // element (the breadcrumb points one level into the create location), and
    // open the target folder AND every ancestor folder so the temp row is
    // actually rendered by the open forest — a create in a collapsed/deep dir
    // would otherwise be invisible. buildActivePath(folderPath) is exactly the
    // ancestor chain (each entry is a directory; root = PROJECT_ROOT_REL).
    const chain = this.folderIds(buildActivePath(folderPath));
    this.update(v => openFolders(withAnchor(v, [...v.anchor, tempPath]), chain));

    // Render NOW (before opening the session) so the temp entry materializes
    // in the DOM. After this initial render the gate defers subsequent
    // renders so the input the session mounts survives async work.
    this.render.render();
    this.editSession.startCreate(
      { kind: 'create', folderPath, type, name: tempName, tempId, templateId: opts?.templateId },
      (createdPath: string) => this.revealCreated(createdPath, type === 'file' || opts?.templateId !== undefined),
    );
  }

  /**
   * Where the tree points once a create has landed: the anchor moves onto the
   * new entry, and it opens — a file (or a template's seeded home) in the
   * editor, a folder as its folder page. `createdPath` is registry-domain.
   */
  private revealCreated(createdPath: string, opensFile: boolean): void {
    if (opensFile) {
      // The new file is the leaf (createdPath is registry-domain, as
      // buildActivePath expects). A folder gets the same from showFolder.
      this.update(v => anchorAt(v, createdPath));
      // Open the new file in the editor (preview follows). isNewFile tells
      // the editor:file-selected handler this is a fresh creation, not a
      // tree click on an existing file, so it focuses the CM6 body once
      // loaded: the temp rename <input>'s focus is destroyed synchronously
      // (discardTempEntry + render(), before loadFile's awaited RPC chain
      // progresses), leaving a focus vacuum otherwise. Scoped to creation
      // so ordinary opens never have their focus silently redirected.
      this.container?.dispatchEvent(
        new CustomEvent('editor:file-selected', {
          bubbles: true,
          detail: { filePath: createdPath, isNewFile: true },
        }),
      );
    } else {
      // The SAME gated path the delete fallback uses: a fresh folder is
      // empty → no home → the folder-page CTA. No expansion: nothing to
      // open, and toggling would leave a spurious 'open' override.
      this.showFolder(createdPath, { justCreated: true });
    }
  }
}
