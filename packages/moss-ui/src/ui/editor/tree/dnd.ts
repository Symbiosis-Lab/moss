/**
 * Drag and drop in the tree: the one in-app drag session, the hit-test that
 * turns a pointer into a drop folder, the drop itself (a batch move through
 * the host's commands), the OS drag-out, and the hover feedback — highlight,
 * spring-load dwell, edge auto-scroll — that both an in-app and an OS drag
 * produce. The delegated drag listeners register here, in `mount`, against
 * the controller's AbortSignal.
 *
 * The Tauri seam is one edge, in the controller: the drag-out plugin arrives through
 * `DndHost.commands`, and the move itself is tree/ops.ts `moveEntriesTo`
 * (the one owner, shared with Move to…); this module never imports bindings.
 *
 * THE INTERCEPTION (why every phase has two entry points): with drag_drop_handler_enabled — on by default, required for OS
 * drag-in (os-drag-in.ts) — Tauri intercepts the ENTIRE native drag session
 * at the webview level, our own draggable rows included. The DOM
 * dragover/drop listeners registered here do NOT fire for a real in-app
 * drag: dragstart, then dragend seconds later, zero dragover between
 * (confirmed with tauri-plugin-drag's drag-out disabled too). The live path
 * is editor-main.ts's `onDragDropEvent` calling `handleInternalDragOver` /
 * `handleInternalDrop` with Tauri's already-logical-pixel coordinates, and
 * `hasActiveInternalDrag` is how it tells one of our rows from a Finder file
 * — otherwise os-drag-in.ts alone would see these drags, and it no-ops on a
 * drop with no paths. The DOM listeners stay as a same-logic fallback for a
 * webview that does not intercept the same way.
 */

import { host as uiHost } from "../../host";
import { debug as logDebug } from "../../log";
import { t } from "../../i18n/index";
import { showToast } from "../../shared/ui/toast/toast-manager";
import { toAbsolute, PROJECT_ROOT_REL, parentDirOf, baseName } from '../path-domain';
import type { EntryRegistry, EntryId, EntryRecord } from '../entry-registry';
import { rowAt, rowFor, type TreeRow } from './render';
import type { FileSelection } from './selection';
import type { TreeOps } from './ops';

/**
 * Opt-in dev trace for the file-tree / drag-and-drop flow. OFF by default —
 * zero noise in production, and it never crosses the IPC boundary when off.
 * Enable for a manual-test session with
 *   localStorage.setItem('moss.tree.trace', '1')   // then reload the editor panel
 * and the lines reach the dev terminal / moss.log through the plugin-log
 * debug channel (the same kill-switch convention as moss.preview.morph).
 * Only LOW-FREQUENCY, high-signal events are traced (drag start/end, drop
 * target + result) — never per-mousemove dragover/highlight. Genuine
 * failures never rely on it: they go through console.error + a toast
 * (performDrop), which install-log-bridge always forwards. Lives here
 * because the drag is where nearly every line is traced; the controller
 * imports it for its few remaining call sites.
 */
export function treeTrace(msg: string): void {
  try {
    if (typeof localStorage !== 'undefined' && localStorage.getItem('moss.tree.trace') === '1') {
      void logDebug(`[tree] ${msg}`);
    }
  } catch { /* localStorage unavailable (SSR/test) — trace is a no-op */ }
}

// ── Drop feedback ──────────────────────────────────────────────────────
//
// The visual feedback a drag hovering the tree produces: the drop-target
// highlight, the spring-load dwell timer, and edge auto-scroll.
//
// They are one record with one `clear()`, because they share exactly one
// lifetime: a drag is over the tree, or it is not. They are NOT folded into
// DragSession, because an OS drag-in from Finder produces all three with no
// in-app session at all.
//
// The highlight deliberately has no mirror field. The DOM class IS the state
// and every paint is a clear-then-set over the whole surface; two independent
// "what is highlighted" bookkeepings let two folders highlight at once.
//
// The trade that buys, named so nobody "optimises" it back: every paint runs a
// `querySelectorAll` over the surface, and a drag paints on every dragover
// (~60/s) even when the target has not moved. At the tree's row counts this is
// not measurable,
// and the obvious fix — a `lastHighlighted` field to skip the repaint — is
// exactly the mirror this deleted, so it must not come back.

export const DROP_TARGET_CLASS = 'moss-tree-row--drop-target';

/** Dwell before a hovered collapsed folder springs open. */
const SPRING_LOAD_MS = 600;

/**
 * Debounce on clearing the highlight. Crossing between two adjacent rows
 * fires dragleave before the next dragover, and the transparent per-row
 * indent margin (file-tree.css:74-89) belongs to no row's hit box, so an
 * immediate clear makes the highlight flicker over the gap. VS Code's
 * listView.ts uses the identical pattern (onDragLeaveTimeout).
 */
const HIGHLIGHT_CLEAR_DEBOUNCE_MS = 100;

/** Edge auto-scroll geometry. */
const EDGE_ZONE = 48;       // px from the top/bottom edge that scrolls
const SCROLL_STEP = 8;      // px per tick
const SCROLL_INTERVAL = 16; // ms (~60fps; setTimeout, not rAF — rAF is
                            // throttled in the idle child WKWebView)

/** What DropFeedback needs from the tree. Nothing else. */
export interface DropFeedbackHost {
  /** The scrolling tree body. Null while unmounted. */
  body(): HTMLElement | null;
  /** The subtree containing every highlightable row. Null while unmounted. */
  surface(): HTMLElement | null;
  /** True when `path` is a folder currently collapsed in this tree. */
  canSpringLoad(path: string): boolean;
  /** Expand `path` through the tree's ordinary toggle path. */
  springOpen(path: string): void;
}

export class DropFeedback {
  private clearTimer: ReturnType<typeof setTimeout> | null = null;
  private springTimer: ReturnType<typeof setTimeout> | null = null;
  private springPath: string | null = null;
  private scrollTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollDir: -1 | 0 | 1 = 0;

  constructor(private readonly host: DropFeedbackHost) {}

  /**
   * Paint `el` as the one drop target, or clear the highlight entirely when
   * given null. Clear-then-set over the whole surface, so it is the only
   * answer to "what is highlighted" no matter which drag path asked.
   */
  highlight(el: HTMLElement | null): void {
    this.cancelClear();
    for (const prev of this.host.surface()?.querySelectorAll<HTMLElement>(`.${DROP_TARGET_CLASS}`) ?? []) {
      prev.classList.remove(DROP_TARGET_CLASS);
    }
    el?.classList.add(DROP_TARGET_CLASS);
  }

  /** Clear the highlight after the debounce, unless a new paint intervenes. */
  scheduleClear(): void {
    this.cancelClear();
    this.clearTimer = setTimeout(() => {
      this.clearTimer = null;
      this.highlight(null);
    }, HIGHLIGHT_CLEAR_DEBOUNCE_MS);
  }

  private cancelClear(): void {
    if (this.clearTimer !== null) {
      clearTimeout(this.clearTimer);
      this.clearTimer = null;
    }
  }

  /**
   * Arm the spring-load dwell for `dirPath` so a nested drop target needs no
   * pre-expanding. Re-arming for the same path is a no-op, so the user's
   * pause keeps counting rather than restarting under a jittery pointer.
   */
  armSpringLoad(dirPath: string): void {
    if (this.springPath === dirPath && this.springTimer !== null) return;
    this.cancelSpringLoad();
    if (!this.host.canSpringLoad(dirPath)) return;
    this.springPath = dirPath;
    this.springTimer = setTimeout(() => {
      this.springTimer = null;
      this.springPath = null;
      this.host.springOpen(dirPath);
    }, SPRING_LOAD_MS);
  }

  cancelSpringLoad(): void {
    if (this.springTimer !== null) clearTimeout(this.springTimer);
    this.springTimer = null;
    this.springPath = null;
  }

  /**
   * Set the edge-scroll direction from a pointer position. Driven by DOM
   * dragover for in-app drags and by Tauri's drag 'over' payload for OS
   * drags, which never produce a DOM dragover.
   */
  updateEdgeScroll(clientY: number): void {
    const body = this.host.body();
    if (body === null) { this.stopEdgeScroll(); return; }
    const rect = body.getBoundingClientRect();
    const dir: -1 | 0 | 1 =
      clientY < rect.top + EDGE_ZONE ? -1
      : clientY > rect.bottom - EDGE_ZONE ? 1
      : 0;
    if (dir === this.scrollDir) return;
    this.scrollDir = dir;
    if (this.scrollTimer !== null) clearTimeout(this.scrollTimer);
    this.scrollTimer = null;
    if (dir !== 0) this.step();
  }

  private step(): void {
    const body = this.host.body();
    if (this.scrollDir === 0 || body === null) return;
    body.scrollTop += this.scrollDir * SCROLL_STEP;
    this.scrollTimer = setTimeout(() => {
      this.scrollTimer = null;
      this.step();
    }, SCROLL_INTERVAL);
  }

  stopEdgeScroll(): void {
    if (this.scrollTimer !== null) clearTimeout(this.scrollTimer);
    this.scrollTimer = null;
    this.scrollDir = 0;
  }

  /**
   * The drag is over. One teardown for all three, so no caller has to
   * remember which subset its exit path owes — the omission that put a
   * dangling highlight-clear past an aborted drag.
   */
  clear(): void {
    this.highlight(null);
    this.cancelSpringLoad();
    this.stopEdgeScroll();
  }
}

// ── The drag session ───────────────────────────────────────────────────

/**
 * How long a retired drag session stays readable after dragend. Tauri's
 * onDragDropEvent 'drop' payload arrives via async IPC after the DOM's
 * native dragend, so a session ended at dragend would make the in-flight
 * drop handler discard a real drop. See retireDrag.
 */
const DRAG_DROP_IPC_GRACE_MS = 500;

/**
 * One in-app drag gesture: opened at dragstart, consumed by the drop,
 * retired at dragend. Replaces a selection snapshot and a path-keyed source
 * that had two clearing disciplines and a window where they could disagree.
 */
interface DragSession {
  readonly source: EntryRecord;
  readonly ids: ReadonlySet<EntryId>;
  ghost: HTMLElement | null;
}

/** What the drag needs from the controller. Seven members; the count is the review artefact. */
export interface DndHost {
  projectRoot(): string;
  /** The tree model: what is under the pointer, what is being dragged. */
  registry(): EntryRegistry;
  /** The persistent selection a drag snapshots, never mutates. */
  readonly selection: Pick<FileSelection, 'has' | 'snapshot'>;
  /** A folder is open iff its override says so; the spring-load asks before arming. */
  isOpen(path: string): boolean;
  /** The ordinary toggle a spring-load fires, so it logs and freezes the park like a click. */
  toggleFolderOpen(path: string): void;
  /** The move a drop performs — with the refresh, the re-anchor and the active-file rename lifecycle. */
  readonly ops: Pick<TreeOps, 'moveEntriesTo'>;
  /** The green arrival pulse on the folder a drop landed in. */
  flashRow(path: string): void;
}

export class TreeDnd {
  /** The tree's root element and its scrolling body, held from `mount` until the signal aborts. */
  private container: HTMLElement | null = null;
  private navBody: HTMLElement | null = null;

  /** The one in-app drag in progress, or `null`. `ids` is the set being
   *  dragged (see startMultiRowDrag); `ghost` is the stack image for N>1,
   *  appended to document.body and owned by the session so it cannot leak. */
  private drag: DragSession | null = null;

  /** The hover feedback — highlight, spring-load dwell, edge auto-scroll —
   *  one record with one `clear()` (the section above says why). */
  private readonly feedback = new DropFeedback({
    body: () => this.navBody,
    surface: () => this.container,
    canSpringLoad: (path) => this.isDirectory(path) && !this.host.isOpen(path),
    springOpen: (path) => this.host.toggleFolderOpen(path),
  });

  constructor(private readonly host: DndHost) {}

  /**
   * Register the delegated drag listeners on the tree's root and body: one
   * drag-source and one drop-target handler for every row and breadcrumb
   * segment, resolved from the event's row at fire time. The
   * abort tears them down and ends whatever drag was in flight — the ghost
   * on document.body, the feedback's timers — so an unmount mid-drag leaks
   * nothing.
   */
  mount(container: HTMLElement, navBody: HTMLElement, signal: AbortSignal): void {
    this.container = container;
    this.navBody = navBody;
    signal.addEventListener('abort', () => {
      this.feedback.clear();
      this.endDrag();
      this.container = null;
      this.navBody = null;
    });

    // Edge auto-scroll stops when the drag leaves the body entirely (not
    // just crossing a child row). It is STARTED from the container's
    // dragover below, which sees the same events — a second dragover here
    // would call updateEdgeScroll twice per move — and its end is the
    // container's dragend, which fires for every drag source.
    navBody.addEventListener('dragleave', (e) => {
      if (!this.navBody?.contains(e.relatedTarget as Node | null)) {
        this.stopEdgeScroll();
      }
    }, { signal, passive: true });

    // A folder row or crumb under the pointer is the target; otherwise the
    // transparent per-row indent margin (file-tree.css:74-89), which belongs
    // to no .seg's hit box, and only the coordinate path sees a drop there
    // (bug #3/#4). Spring-load arms only over a row, not the margin — a
    // deliberate, deferred gap, not a correctness bug. See THE INTERCEPTION
    // in the module doc for why these are the fallback, not the live path.
    container.addEventListener('dragover', (e) => {
      const dir = this.dropTargetAt(e.target);
      if (dir !== null) {
        e.preventDefault();
        this.updateEdgeScroll(e.clientY); // the one edge-scroll driver for in-app drags
        if (!this.canDropInto(dir.path)) return;
        if (this.drag !== null && e.dataTransfer) e.dataTransfer.dropEffect = 'move';
        this.feedback.highlight(dir.el);
        this.feedback.armSpringLoad(dir.path);
        return;
      }
      this.handleInternalDragOver(e.clientX, e.clientY);
      if (this.hasActiveInternalDrag()) {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
      }
    }, { signal });
    container.addEventListener('dragleave', (e) => {
      const dir = this.dropTargetAt(e.target);
      if (dir === null) return;
      this.feedback.cancelSpringLoad();
      this.feedback.scheduleClear();
    }, { signal });
    container.addEventListener('drop', (e) => {
      const dir = this.dropTargetAt(e.target);
      if (dir !== null) {
        e.preventDefault();
        treeTrace(`row drop dirPath=${dir.path}`);
        this.feedback.clear();
        this.performDrop(dir.path);
        return;
      }
      if (this.hasActiveInternalDrag()) e.preventDefault();
      this.handleInternalDrop(e.clientX, e.clientY);
    }, { signal });
    container.addEventListener('dragstart', (e) => {
      const row = rowAt(e.target);
      const entry = row === null ? null : this.host.registry().recordAt(row.path);
      // The project root is a drop target, never a drag source.
      if (entry === null || entry.path === PROJECT_ROOT_REL) return;
      this.startMultiRowDrag(entry, e);
      // OS drag-out runs alongside the browser's own DnD: a drop inside moss
      // is handled by the drop listener above, a drop on Finder/Explorer by
      // the plugin. The two do not compete for the same event.
      void this.startOsDragOut(entry);
    }, { signal });
    container.addEventListener('dragend', () => this.retireDrag(), { signal });
  }

  private isDirectory(path: string): boolean {
    return this.host.registry().recordAt(path)?.isDir ?? false;
  }

  /**
   * Open the session at dragstart. A selected source drags the whole
   * selection; an unselected one drags just itself, and the persistent
   * selection is untouched either way (Finder) — it must be, because
   * selection.focus drives the open editor file through the activeEntryId
   * facade, and a drag must not switch what is open.
   */
  private startMultiRowDrag(sourceEntry: EntryRecord, e: DragEvent): void {
    const draggedIds: Set<EntryId> = this.host.selection.has(sourceEntry.id)
      ? new Set(this.host.selection.snapshot().selected)
      : new Set([sourceEntry.id]);
    this.endDrag(); // a new gesture supersedes any session still in its grace window
    this.drag = { source: sourceEntry, ids: draggedIds, ghost: null };
    treeTrace(`dragstart source=${sourceEntry.path} selectionSize=${draggedIds.size}`);
    if (e.dataTransfer) {
      e.dataTransfer.setData('text/plain', sourceEntry.path);
      e.dataTransfer.effectAllowed = 'move';
      // The EntryIds as a payload: the tree's own drop reads the typed
      // session; cm-drop.ts's editor drop reads this.
      e.dataTransfer.setData(
        'application/x-moss-entries',
        JSON.stringify({ entryIds: [...draggedIds] }),
      );
    }
    // Stack ghost with count badge for N>1
    if (draggedIds.size > 1) {
      const ghost = document.createElement('div');
      ghost.className = 'drag-ghost';
      ghost.dataset.count = String(draggedIds.size);
      ghost.textContent = t('tree.drag_n_items', { n: draggedIds.size });
      const badge = document.createElement('span');
      badge.className = 'drag-ghost__badge';
      badge.textContent = String(draggedIds.size);
      ghost.appendChild(badge);
      document.body.appendChild(ghost);
      this.drag.ghost = ghost;
      // Browser snapshots the ghost element at this instant — offset 16,16
      // puts cursor near top-left of the stack icon.
      e.dataTransfer?.setDragImage(ghost, 16, 16);
    }
  }

  /**
   * dragend does NOT end the session, it retires it on a grace timer
   * Tauri's onDragDropEvent 'drop' payload
   * arrives via async IPC AFTER the DOM's native dragend, so ending here
   * races that handler into discarding a genuine drop. The window covers
   * the observed IPC latency, and a following dragstart ends the session
   * outright anyway, so the timer only has to survive the gap.
   */
  private retireDrag(): void {
    const session = this.drag;
    treeTrace(`dragend stillArmed=${session !== null}`);
    if (session !== null) {
      session.ghost?.remove();
      session.ghost = null;
      setTimeout(() => {
        if (this.drag === session) this.drag = null;
      }, DRAG_DROP_IPC_GRACE_MS);
    }
    // An aborted drag (dropped outside any valid target, or Escape) leaves no
    // feedback behind; this fires for EVERY drag source, row or crumb.
    this.feedback.clear();
  }

  /**
   * End the session NOW, the counterpart to retireDrag's grace window: the
   * drop has been consumed (or a new gesture supersedes it), so leaving it
   * armed would misroute what follows.
   */
  private endDrag(): void {
    this.drag?.ghost?.remove();
    this.drag = null;
  }

  // ── Public OS drag-in accessors ──────────────────────────────────

  /**
   * Resolve drag-payload entry IDs to project-relative paths for the
   * tree→editor drop (cm-drop.ts, Flow 3), a folder's with a trailing `/` so
   * it embeds as `![[folder/]]`. The full path, not the name: a media embed
   * is written as the path from the page. Unresolvable ids (stale / not
   * registered) are skipped.
   */
  public resolveDropEntries(ids: number[]): string[] {
    return ids.flatMap((id) => {
      const record = this.host.registry().getById(id as EntryId);
      if (!record) return [];
      if (!baseName(record.path)) return []; // root or empty path — skip
      return [record.isDir ? `${record.path}/` : record.path];
    });
  }

  /**
   * Hit-test a viewport point (CSS pixels, `getBoundingClientRect` space)
   * against the rendered rows: the EntryId of the folder under (x, y), a
   * file's parent for a file row, null when unmounted or nothing is there.
   * Tauri's `onDragDropEvent` position is in physical pixels; the caller
   * divides by `window.devicePixelRatio` first (os-drag-in.ts, which has a
   * position rather than a DOM target and so must hit-test).
   *
   * `elementsFromPoint` first, walking each hit up to its nearest
   * [data-path] ancestor, so the transparent left-margin beside an indented
   * pill (margin-inline-start under align-items:flex-start) and a hit on a
   * child (chevron, text span) both resolve. jsdom has no
   * `elementsFromPoint`, so the y-range scan below is the sole path there.
   */
  public getFolderAtPosition(x: number, y: number): EntryId | null {
    if (!this.container) return null;

    if (typeof document.elementsFromPoint === 'function') {
      const hits = document.elementsFromPoint(x, y);
      for (const el of hits) {
        let cur: Element | null = el;
        while (cur && cur !== this.container) {
          const path = (cur as HTMLElement).dataset?.path;
          if (path !== undefined) {
            const record = this.host.registry().recordAt(path);
            if (record === null) { cur = cur.parentElement; continue; }
            const target = this.dropFolderFor(record);
            if (target !== null) return target;
            break; // a [data-path] that resolves to no folder — stop walking
          }
          cur = cur.parentElement;
        }
        if (cur === this.container) break; // hits past the tree are not ours
      }
    }

    // Fallback: y-range scan over all rows. The indent-margin case, where
    // elementsFromPoint lands on the group wrapper and only the y-scan
    // resolves the row by its vertical position because the tree has one column.
    const rows = this.container.querySelectorAll<HTMLElement>('[data-path]');
    for (const row of rows) {
      const rect = row.getBoundingClientRect();
      if (y >= rect.top && y < rect.bottom) {
        const path = row.dataset.path;
        if (!path) continue;
        const record = this.host.registry().recordAt(path);
        if (record === null) continue;
        const target = this.dropFolderFor(record);
        if (target !== null) return target;
      }
    }

    return null;
  }

  /**
   * The folder a drop over `record`'s row lands in: the folder itself, or a
   * file's parent (Finder-style drop-to-parent). A root-level file resolves
   * to PROJECT_ROOT_REL, whose registry id is the project root — so hovering
   * a root file targets the root folder, not null. Null only when the parent
   * is not registered.
   */
  private dropFolderFor(record: EntryRecord): EntryId | null {
    if (record.isDir) return record.id;
    return this.host.registry().getByPath(parentDirOf(record.path));
  }

  // The OS-drag entry points (os-drag-in.ts, editor-main.ts): an OS drag
  // produces no DOM dragover, so its 'over'/'leave' payloads drive the same
  // feedback by hand. Spring-load arming is idempotent for the same path
  // (the hover pause keeps counting); null cancels it. A breadcrumb folder
  // highlights its crumb, a body folder its row — the one element that
  // renders it.

  public springLoadIfHoverIs(dirPath: string | null): void {
    if (dirPath === null) this.feedback.cancelSpringLoad();
    else this.feedback.armSpringLoad(dirPath);
  }

  public setDropHighlight(id: EntryId | null): void {
    this.feedback.highlight(id === null ? null : rowFor(this.container, id));
  }

  /** @param clientY CSS-pixel Y coordinate of the pointer. */
  public updateEdgeScroll(clientY: number): void {
    this.feedback.updateEdgeScroll(clientY);
  }

  public stopEdgeScroll(): void {
    this.feedback.stopEdgeScroll();
  }

  /**
   * A folder row usable as a drop target, or `null`. Breadcrumb segments
   * qualify as well as body rows — the same unification the per-row wiring
   * had.
   */
  private dropTargetAt(target: EventTarget | null): TreeRow | null {
    const row = rowAt(target);
    return row !== null && this.isDirectory(row.path) ? row : null;
  }

  /**
   * OS drag-out (drag to Finder / Explorer), where the host offers one. Runs
   * from the same `dragstart` as the in-app session, so a plain drag on any
   * row does both: HTML5 DnD carries on for a drop inside moss, the host
   * hands the OS the file list for a drop outside, and the two never compete
   * for an event. The session's multi-select paths are forwarded, so dragging
   * three selected files out copies all three.
   */
  private async startOsDragOut(entry: EntryRecord): Promise<void> {
    const dragOut = uiHost.dragOut;
    if (!dragOut) return;
    // The session is already open — the caller starts it first. Fall back to
    // this row alone if the session carries nothing resolvable.
    const paths: string[] = [];
    for (const id of this.drag?.ids ?? []) {
      const e = this.host.registry().getById(id);
      if (e) paths.push(e.path);
    }
    if (paths.length === 0) paths.push(entry.path);
    try {
      // The OS takes absolute paths; the registry stores relative ones.
      await dragOut(paths.map((p) => toAbsolute(p, this.host.projectRoot())));
    } catch { /* ignore — user can still use Finder/Explorer to copy files */ }
  }

  /** The folder under (x, y) while an in-app drag is active, else null. */
  private resolveDragTargetId(x: number, y: number): EntryId | null {
    if (this.drag === null) return null;
    return this.getFolderAtPosition(x, y);
  }

  /** True while an in-app row/breadcrumb drag is active: how editor-main's
   *  `onDragDropEvent` tells our row from a Finder file (THE INTERCEPTION). */
  public hasActiveInternalDrag(): boolean {
    return this.drag !== null;
  }

  /**
   * The EntryIds carried by the active internal drag session; empty when no
   * in-app drag is active. Read-only accessor so editor-main's tree→editor
   * embed routing (Flow 3) never touches the private session record.
   */
  public activeInternalDragEntryIds(): number[] {
    return this.drag === null ? [] : [...this.drag.ids];
  }

  /**
   * Consume the session after a drop the TREE does not handle itself —
   * editor-main's tree→editor embed path (Flow 3). endDrag, not retireDrag's
   * grace: the drop has been consumed, and a session left armed would
   * misroute the next gesture as an internal drag. Clears the hover feedback
   * too, as handleInternalDrop → performDrop does for a tree target.
   */
  public clearInternalDragSession(): void {
    this.endDrag();
    this.feedback.clear();
  }

  /** The live dragover path: editor-main's onDragDropEvent ('over') with
   *  Tauri's logical-pixel coordinates (THE INTERCEPTION, module doc). */
  public handleInternalDragOver(x: number, y: number): void {
    const id = this.resolveDragTargetId(x, y);
    if (id === null) {
      this.setDropHighlight(null);
      return;
    }
    const path = this.host.registry().pathOf(id);
    const canDrop = path !== null && this.canDropInto(path);
    this.setDropHighlight(path !== null && canDrop ? id : null);
  }

  /** The live drop path, the same way. */
  public handleInternalDrop(x: number, y: number): void {
    const id = this.resolveDragTargetId(x, y);
    if (id === null) {
      treeTrace(`internal drop: no target at (${x},${y}) — no-op`);
      return;
    }
    const path = this.host.registry().pathOf(id);
    treeTrace(`internal drop pos=(${x},${y}) resolvedId=${id} resolvedPath=${path}`);
    if (path === null) return;
    this.feedback.clear();
    this.performDrop(path);
  }

  /**
   * Execute a drop: move the dragged entry/selection into `targetDirPath`.
   * Shared by the delegated drop handler and the in-app-drag path
   * (handleInternalDrop) — one move-and-feedback implementation, not two.
   * The move itself — one move_entries call, one refreshTree, the re-anchor
   * and the active-file rename lifecycle — is ops.moveEntriesTo, the same
   * owner Move to… uses; this keeps only what is the drag's: which of the
   * dragged paths may land here, the toast, and the arrival flash.
   */
  private performDrop(targetDirPath: string): void {
    const { host } = this;
    const session = this.drag;
    treeTrace(`performDrop target=${targetDirPath} selectionSize=${session?.ids.size ?? 0} source=${session?.source.path ?? 'none'}`);
    if (session !== null) {
      const validPaths: string[] = [];
      for (const id of session.ids) {
        const entry = host.registry().getById(id);
        if (entry && this.canDropOneInto(entry.path, targetDirPath)) {
          validPaths.push(entry.path);
        }
      }
      if (validPaths.length > 0) {
        void host.ops.moveEntriesTo(validPaths, targetDirPath).then((result) => {
          treeTrace(`performDrop moveEntries status=${result.status}${result.status !== 'ok' ? ' error=' + String(result.error) : ''}`);
          if (result.status !== 'ok') {
            console.error('[file-tree] batch drop move failed:', result.error);
            showToast({ what: t('tree.move_failed').replace('{error}', String(result.error)), severity: 'Blocking' });
            return;
          }
          // Green arrival flash on the target folder, the same pulse a
          // download gives it. AFTER refreshTree()+render(): a pre-render
          // element is one render() destroys, and the pulse never showed.
          // Resolved by path, root's '' included.
          host.flashRow(targetDirPath);
        });
      }
    }
    this.endDrag();
  }

  /** Returns true if ANY dragged entry can be moved into the target dir. */
  private canDropInto(targetDirPath: string): boolean {
    if (this.drag === null) return false;
    for (const id of this.drag.ids) {
      const entry = this.host.registry().getById(id);
      if (entry && this.canDropOneInto(entry.path, targetDirPath)) return true;
    }
    return false;
  }

  /** Returns true if `sourcePath` can be moved into `targetDirPath`. */
  private canDropOneInto(sourcePath: string, targetDirPath: string): boolean {
    // 1. No-op: drop on current parent
    if (parentDirOf(sourcePath) === targetDirPath) return false;
    // 2. Cycle: drop a folder on itself or its descendant
    if (sourcePath === targetDirPath) return false;
    if (targetDirPath.startsWith(`${sourcePath}/`)) return false;
    return true;
  }
}
