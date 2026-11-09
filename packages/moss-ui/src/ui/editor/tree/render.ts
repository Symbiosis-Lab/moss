/**
 * The renderer: the breadcrumb strip and the tree body, rebuilt from the
 * controller's view record on every `render()`, and the nav surface's height
 * written from the same pass. Owns the two containers, the breadcrumb `Spine`
 * that allocates their shared offsets, and the row cache that keeps a row's
 * element across a rebuild (rows are reconciled by EntryId and carry
 * no listeners of their own — the delegated handlers live in the controller
 * and resolve a row from the event through `rowAt` below).
 *
 * Reads the model
 * through `RenderHost` and writes nothing back except through the DOM; the
 * height is a pure function of what it rendered (`desiredHeight` in
 * tree/spine-projection.ts), so the controller's sash machine asks this
 * module for the height rather than the other way round.
 *
 * Gesture resolution (`rowAt`, `rowFor`, `isChevron`, `isTextEntry`) lives
 * here because row resolution belongs with the rows: every row and breadcrumb
 * segment carries its own `data-path` and `data-entry-id`, so a delegated
 * handler on the container can answer "which row?" at fire time — pure DOM
 * reads with no tree state.
 */

import { t } from "../../i18n/index";
import type { EntryRegistry, EntryId, EntryRecord } from '../entry-registry';
import { PROJECT_ROOT_REL, baseName, displayName, fileDisplayParts } from '../path-domain';
import type { EditGate } from './edit-gate';
import type { PreflightEvidence, TreeView } from './model';
import type { FileSelection } from './selection';
import type { FolderDerivation } from '@symbiosis-lab/moss-ui';
import { mkHintRow } from './derivations';
import { Spine, readPxVar } from './spine';
import { spineBody, ROW_H, desiredHeight, isParkedHeight, stateRowKind, type StateRowKind } from './spine-projection';

// ── Gesture resolution ─────────────────────────────────────────────────

/**
 * The element rendering entry `id` under `root`, or null when it is not on
 * screen. Keyed by the `data-entry-id` every seg carries, so a rename between
 * render and lookup cannot miss; under the integrated-spine invariant a node
 * renders in exactly one container, so there is one answer. `data-path` stays
 * on rows for the event → row direction (`rowAt`) and the engine gates.
 */
export function rowFor(root: ParentNode | null, id: EntryId): HTMLElement | null {
  return root?.querySelector<HTMLElement>(`.seg[data-entry-id="${id}"]`) ?? null;
}

/** A row (or breadcrumb segment) resolved from an event, with its path. */
export interface TreeRow {
  readonly el: HTMLElement;
  readonly path: string;
}

/**
 * Anything the user types into. An open inline rename box lives INSIDE its
 * row, so without this boundary a click in it resolves to the row underneath
 * and acts on the file being renamed.
 *
 * One selector with two callers, deliberately: `rowAt` returns null here, and
 * the context-menu handler tests it separately because it must bail BEFORE
 * `preventDefault` — a right-click inside the box gets the native editing
 * menu, not moss's. Two copies of the string is what this exists to prevent.
 */
const TEXT_ENTRY = 'input, textarea, [contenteditable="true"]';

export function isTextEntry(target: EventTarget | null): boolean {
  return (target as Element | null)?.closest(TEXT_ENTRY) != null;
}

/** The row an event landed on, or `null` for bare surface and text entry. */
export function rowAt(target: EventTarget | null): TreeRow | null {
  if (isTextEntry(target)) return null;
  const el = (target as Element | null)?.closest<HTMLElement>('.seg[data-path]');
  const path = el?.dataset.path;
  return el == null || path === undefined ? null : { el, path };
}

/** True when the event landed on `row`'s own chevron hit-zone. */
export function isChevron(target: EventTarget | null, row: HTMLElement): boolean {
  const chev = (target as Element | null)?.closest('.chev');
  return chev != null && chev.closest('.seg') === row;
}

// ── Row construction ───────────────────────────────────────────────────

export const CHEV_SVG = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>';

/** Create a segment element (folder or file). */
export function mkSeg(name: string, isFolder: boolean, ...classes: string[]): HTMLElement {
  return fillSeg(document.createElement('div'), name, isFolder, classes);
}

/**
 * Render a segment INTO `el`, which may be a row already on screen. Every
 * attribute and child is dropped first, so a reused row is indistinguishable
 * from a fresh one apart from its identity — and identity is the whole point:
 * a row that survives keeps its scroll offset, its ResizeObserver
 * registration and its JS-set custom properties, and the browser will pair
 * two clicks only when both land on the SAME element.
 */
export function fillSeg(el: HTMLElement, name: string, isFolder: boolean, classes: string[]): HTMLElement {
  while (el.attributes.length > 0) el.removeAttribute(el.attributes[0].name);
  el.replaceChildren();
  el.className = ['seg', ...(isFolder ? [] : ['leaf']), ...classes.filter(Boolean)].join(' ');
  // ARIA tree pattern (WAI-ARIA APG): every row is a treeitem; selection
  // state lands here, expand state set by callers for folders, level set
  // by callers based on depth. aria-selected defaults to "false" so
  // assistive tech reads consistently before any user gesture.
  el.setAttribute('role', 'treeitem');
  el.setAttribute('aria-selected', 'false');
  if (isFolder) {
    // 'open' class on folder segments means expanded; otherwise collapsed.
    el.setAttribute('aria-expanded', classes.includes('open') ? 'true' : 'false');
    const chev = document.createElement('span');
    chev.className = 'chev';
    chev.innerHTML = CHEV_SVG;
    el.appendChild(chev);
  } else {
    const dot = document.createElement('span');
    dot.className = 'dot';
    el.appendChild(dot);
  }
  const textSpan = document.createElement('span');
  textSpan.className = 'seg-name';
  textSpan.textContent = name;
  el.appendChild(textSpan);
  return el;
}

/** i18n keys for a state row — see tree/spine-projection.ts's `StateRowKind`/
 *  `ForestRow.kind` and the mkStateRow doc below. Resolved via t() at render
 *  time (not module load) so the existing onLocaleChange → render() re-render
 *  picks up a locale switch, same as every other tree string. */
const STATE_ROW_KEY: Record<StateRowKind, 'tree.state_empty' | 'tree.state_unavailable' | 'tree.state_hidden'> = {
  empty: 'tree.state_empty',
  unavailable: 'tree.state_unavailable',
  hidden: 'tree.state_hidden',
};

/**
 * Non-interactive placeholder row for an open folder level that has nothing
 * to show (every gesture ends in a visible
 * consequence").
 * `kind: 'empty'` means the folder genuinely has no
 * children; `kind: 'unavailable'` means dirIndex has no entry for it at all
 * (a degenerate refresh — Finding 2's "silent no-op sink"); `kind: 'hidden'`
 * means the folder has no children to show but its own listing filtered
 * something out (moss's own `.moss/` housekeeping — see `TreeNode.hidden`).
 *
 * Deliberately NOT a `.seg`: it carries no path identity, is never a drag
 * source/target/click target, and is excluded from aria-posinset/setsize
 * (those are computed over rows the user can act on).
 */
function mkStateRow(kind: StateRowKind, depth: number, offsetPx: number): HTMLElement {
  const el = document.createElement('div');
  el.className = `nav-state-row nav-state-row--${kind}`;
  el.style.setProperty('--ft-offset', `${offsetPx}px`);
  el.setAttribute('data-depth', String(depth));
  el.setAttribute('data-state-row', kind);
  el.textContent = t(STATE_ROW_KEY[kind]);
  return el;
}

/** The muted extension suffix after a non-markdown file's name (`name.canvas`). */
function appendExtHint(el: HTMLElement, extHint: string | null | undefined): void {
  if (!extHint) return;
  const hint = document.createElement('span');
  hint.className = 'ext-hint';
  hint.textContent = extHint;
  el.appendChild(hint);
}

/**
 * Place a tree-body row at `depth`, indenting by `offsetPx` — the rounded
 * prefix-offset O[depth] from the spine allocator (the SAME array the
 * breadcrumb consumes, so the spine aligns by construction).
 * `.nav-body .seg`'s padding-inline-start reads --ft-offset. `data-depth` lets
 * the ResizeObserver re-apply offsets to existing rows without a full rebuild.
 * Rows are appended in visual order, so no grid row/column.
 */
export function place(el: HTMLElement, depth: number, offsetPx: number): HTMLElement {
  el.style.setProperty('--ft-offset', `${offsetPx}px`);
  el.setAttribute('data-depth', String(depth));
  return el;
}

/**
 * Create a WAI-ARIA `role="group"` wrapper for one sibling group.
 *
 * The wrapper is a containment boundary for assistive tech AND a layout-neutral
 * flex column (see `.nav-body [role="group"]` in file-tree.css) that HOSTS the
 * sibling connector rail (a `::before` at `--ft-rail-offset`). It is NOT
 * `display:contents` — the rail needs a real box to position against.
 *
 * `--ft-rail-offset` is the children's content-left offset (the same value
 * `place()` writes to the child rows). This is a correct default for any
 * non-home-pair group and for jsdom; `syncRailOffsets()` later refines it from
 * the live rows so the rail tracks a home-pair's measured shift.
 */
export function mkGroup(childOffsetPx: number): HTMLElement {
  const g = document.createElement('div');
  g.setAttribute('role', 'group');
  g.style.setProperty('--ft-rail-offset', `${childOffsetPx}px`);
  return g;
}

/** A fresh `.nav-bc-strip__mode` layer, child of `strip` (title-bar.ts). */
function mkTitleBarLayer(strip: HTMLElement): HTMLElement {
  const layer = document.createElement('div');
  layer.className = 'nav-bc-strip__mode';
  strip.appendChild(layer);
  return layer;
}

// ── The renderer ───────────────────────────────────────────────────────

/** What the renderer reads from the controller. It writes nothing back. */
export interface RenderHost {
  /** The vault root, for the root crumb's display name. */
  projectRoot(): string;
  /** The tree model: every fact about an entry, and the active file. */
  registry(): EntryRegistry;
  /** The view record — anchor, open forest, park — read fresh on every pass. */
  view(): TreeView;
  /** A folder's children in display order, or null when dirIndex has no entry. */
  children(folderPath: string): EntryRecord[] | null;
  /** The one path→id read seam for the open forest. */
  isOpen(path: string): boolean;
  isDirectory(path: string): boolean;
  /** A folder's own `hidden` count (`EntryRecord.hidden`/`TreeNode.hidden`) —
   *  how many of its immediate children moss's curated tree filtered out.
   *  `0` for a folder nothing was filtered from. Feeds `stateRowKind`. */
  hiddenCount(path: string): number;
  /** The folder derivations behind the hint rows (tree/derivations.ts). */
  readonly derivations: { at(path: string): FolderDerivation | null; hasHint(path: string): boolean };
  /** The selection the rows paint; `focus` carries the roving tabindex. */
  readonly selection: Pick<FileSelection, 'has' | 'focus'>;
  /** The admission gate: no rebuild while an inline input is mounted. */
  readonly gate: Pick<EditGate, 'admit' | 'isEditing'>;
  preflightEvidence: Pick<PreflightEvidence, 'count'>;
  /** The sash's ceiling — the controller reads the chrome, the renderer clamps to it. */
  floor(): number;
}

export class TreeRender {
  /** The tree's root, its breadcrumb strip and its scrolling body — created in
   *  `mount`, held until `unmount`; `render()` is the only writer of their children. */
  private container: HTMLElement | null = null;
  private navBcStrip: HTMLElement | null = null;
  private navBody: HTMLElement | null = null;
  /** The nav surface whose height follows the rendered forest; height writes are no-ops until `setupDragHandle` sets this. */
  wrapper: HTMLElement | null = null;
  /** The breadcrumb spine: measures, allocates, and owns the offsets tree-row
   *  placement reads. Everything it schedules is torn down by its own `dispose()`. */
  readonly spine = new Spine(() => this.reallocateSpine());

  constructor(private readonly host: RenderHost) {}

  /** Build the two containers under `container` (= .nav-grid, a flex column):
   *  a non-scrolling breadcrumb strip (.nav-bc-strip, plus its title-bar
   *  layer) followed by the scrolling tree body (.nav-body); rows indent via
   *  --ft-offset (fixed-indent model). See file-tree.css. */
  mount(container: HTMLElement): void {
    this.container = container;
    this.navBcStrip = document.createElement('div');
    this.navBcStrip.className = 'nav-bc-strip';
    container.appendChild(this.navBcStrip);
    mkTitleBarLayer(this.navBcStrip);
    this.navBody = document.createElement('div');
    this.navBody.className = 'nav-body';
    // ARIA tree pattern (WAI-ARIA APG): role=tree, multi-selectable; each
    // rendered row (mkSeg) is a treeitem.
    this.navBody.setAttribute('role', 'tree');
    this.navBody.setAttribute('aria-label', t('tree.file_tree_aria'));
    this.navBody.setAttribute('aria-multiselectable', 'true');
    container.appendChild(this.navBody);

    // Adaptive-spine width reactivity: re-allocate on nav-grid resize (custom
    // properties write to DEEPER nodes, never the observed one — no feedback loop).
    this.spine.observe(container);
  }

  /**
   * Arm the post-mount re-measures. Once fonts settle: the first render may
   * have measured against fallback metrics if a web font hadn't loaded yet
   * — cheap, idempotent, ≥1px-gated, guarded for jsdom. And one
   * guaranteed settle after Tauri's sizing: WKWebView's first render runs
   * before `set_size()` reaches the web content process (panelW=0), and
   * ResizeObserver's own notification can race the debounce and be
   * coalesced away. See Spine.armInitialSettle.
   */
  armInitialSettle(): void {
    if (typeof document !== 'undefined' && document.fonts?.ready) {
      document.fonts.ready.then(() => {
        if (this.navBcStrip) this.spine.schedule();
      }).catch(() => { /* fonts API unavailable — system metrics stand */ });
    }
    this.spine.armInitialSettle();
  }

  /** Tear down the spine's observer and timers and drop the containers (the
   *  controller empties the root). */
  unmount(): void {
    this.spine.dispose();
    this.container = null;
    this.navBcStrip = null;
    this.navBody = null;
    this.wrapper = null;
  }

  /** The breadcrumb strip, for the controller's window-drag region. */
  get strip(): HTMLElement | null { return this.navBcStrip; }
  /** The scrolling body: the drag listeners register on it, and the
   *  selection reads its rows in DOM order. */
  get body(): HTMLElement | null { return this.navBody; }

  /**
   * Apply `.selected` class and `aria-selected` to every visible row based on
   * the current selection. The class drives the accent background in CSS;
   * aria-selected is for assistive tech (WAI-ARIA tree pattern). Both go on
   * every row regardless of state ('false' when not selected) so a screen
   * reader reads the row count consistently.
   */
  renderSelectionVisualState(): void {
    if (!this.navBody) return;
    // Breadcrumb segs carry data-path for navigation but are display-only:
    // they must never carry .selected/aria-selected="true", or the active
    // file's bc seg paints with --moss-accent-soft (regression of fea219b14).
    const rows = this.navBody.querySelectorAll<HTMLElement>('[data-path]:not(.bc)');
    const focusId = this.host.selection.focus;
    let anyRowGotTabIndex0 = false;
    for (const row of rows) {
      const path = row.dataset.path;
      if (!path) continue;
      const id = this.host.registry().getByPath(path);
      const selected = id !== null && this.host.selection.has(id);
      const isFocus = id !== null && id === focusId;
      row.classList.toggle('selected', selected);
      row.setAttribute('aria-selected', selected ? 'true' : 'false');
      // Roving tabIndex: focused row = 0, all others = -1 (WAI-ARIA tree pattern)
      row.setAttribute('tabindex', isFocus ? '0' : '-1');
      if (isFocus) anyRowGotTabIndex0 = true;
    }
    // Initial-focus: when no selection exists, make the first visible nav row
    // (non-breadcrumb) keyboard-reachable so a keyboard user can Tab into the
    // tree without clicking first. WAI-ARIA APG tree pattern requires at least
    // one treeitem to have tabindex=0.
    if (!anyRowGotTabIndex0) {
      const firstNavRow = this.navBody.querySelector<HTMLElement>('[data-path]:not(.bc)');
      if (firstNavRow) firstNavRow.setAttribute('tabindex', '0');
    }
  }

  /** Update tabIndex=0 on the focused row and -1 on all others.
   *
   * Also moves actual DOM focus to the target row and scrolls it into view.
   * Per WAI-ARIA APG tree pattern, screen readers only announce focus changes
   * when DOM focus moves — setting tabindex alone is insufficient.
   */
  updateTabIndex(focusId: EntryId): void {
    if (!this.navBody) return;
    const record = this.host.registry().getById(focusId);
    if (!record) return;
    const rows = this.navBody.querySelectorAll<HTMLElement>('[data-path]');
    let targetRow: HTMLElement | null = null;
    for (const row of rows) {
      const rowPath = row.dataset.path;
      const isFocus = rowPath === record.path;
      row.setAttribute('tabindex', isFocus ? '0' : '-1');
      if (isFocus) targetRow = row;
    }
    if (targetRow) {
      // WAI-ARIA APG tree pattern: DOM focus must move so screen readers
      // announce the newly focused item. preventScroll:false lets the browser
      // scroll natively; the explicit scrollIntoView is belt-and-suspenders
      // for rows that are partially clipped but technically "in viewport".
      targetRow.focus({ preventScroll: false });
      // scrollIntoView is not implemented in jsdom; guard for test environments.
      targetRow.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
    }
  }

  /** The element rendering entry `id` (the module-level `rowFor` above, over
   *  this tree's root), or `null` when it is not on screen. */
  rowFor(id: EntryId): HTMLElement | null {
    return rowFor(this.container, id);
  }

  /**
   * Flash a tree row (or breadcrumb segment) by path with the shared green
   * `.moss-just-committed` confirmation pulse. Restart-safe; no-op when no
   * element matches; viewport-only (no expansion change). The controller
   * delegates here so editor-main can flash a known target without reaching
   * into the tree's DOM.
   */
  flashRow(path: string): void {
    const id = this.host.registry().getByPath(path);
    const el = id === null ? null : this.rowFor(id);
    if (!el) return;
    el.classList.remove('moss-just-committed');
    void el.offsetWidth; // force reflow so the next add restarts the animation
    el.classList.add('moss-just-committed');
  }

  /**
   * Child paths of a folder, or null when dirIndex has no entry for it at
   * all — the distinction tree/spine-projection.ts's spineBody() needs for state
   * rows (empty vs unavailable).
   */
  private getChildrenOrNull(parentPath: string): string[] | null {
    return this.host.children(parentPath)?.map(e => e.path) ?? null;
  }

  /**
   * Why an opened level rendered zero rows, for the `cause=` field on a
   * `outcome=no-rows` gesture log line. `null` when the level actually has
   * rows — callers only call this after confirming renderedRowCount() is
   * unchanged/zero for the level just opened.
   */
  noRowsCause(path: string): 'missing-dirIndex' | 'empty-dirIndex' | null {
    const children = this.getChildrenOrNull(path);
    if (children === null) return 'missing-dirIndex';
    if (children.length === 0) return 'empty-dirIndex';
    return null;
  }

  /**
   * The open-folder set the body renders: every path the user has opened
   * (overrides === 'open'). buildGroup recurses wherever this map `.has(path)`;
   * only key presence matters. The single "what's visible" input, paired with
   * spineBody() for the row count.
   */
  private openPathsMap(): Map<string, string> {
    const open = new Map<string, string>();
    for (const [id, intent] of this.host.view().overrides) {
      const path = this.host.registry().getById(id)?.path;
      if (intent === 'open' && path !== undefined) open.set(path, '');
    }
    return open;
  }

  /**
   * The number of tree-body ROWS the spine body renders. Drives the panel
   * height (height = clamp(navH(count), minH, floor)). A folder + its
   * self-named home file share ONE visual row (the `.seg-pair`), so each open
   * folder with an elected home file collapses two body entries into
   * one — subtract those. Pure arithmetic over spineBody + dirIndex; no DOM,
   * so it is correct even before/independent of a render.
   */
  renderedRowCount(): number {
    // The spine body excludes EVERY active-path node (leaf AND ancestors) — they
    // render in the breadcrumb — so the count IS the body's row count, in
    // deepest-first order (see tree/spine-projection.ts / the integrated-spine invariant).
    const rows = spineBody(
      this.host.view().anchor,
      (p) => this.host.isOpen(p),
      (p) => this.getChildrenOrNull(p),
      (p) => this.host.isDirectory(p),
      (p) => this.host.hiddenCount(p),
      (p) => this.host.derivations.hasHint(p),
    );
    // A folder with a same-row home file renders folder+home as ONE visual row
    // (the `.seg-pair`), but the forest lists both → subtract one per such
    // folder. `rendersAsHomePair()` is the SINGLE source of "this folder shows
    // an inline home file" — render()'s buildGroup uses the SAME predicate to
    // build the pair, so the count can't drift from what's drawn (review #4).
    let count = rows.length;
    for (const row of rows) {
      if (row.isDir && this.rendersAsHomePair(row.path)) count -= 1;
    }
    return count;
  }

  /**
   * True iff folder `path` renders its home file INLINE on its own row (a
   * `.seg-pair`). The ONE definition of that condition: the folder is open,
   * it has a backend-elected home child (`is_home`), and that home file is
   * itself a rendered body row (not an excluded active-path node). buildGroup
   * (which builds the pair) and renderedRowCount (which merges its row count)
   * BOTH call this, so the height can never diverge from the rendered body
   * (review #4).
   */
  private rendersAsHomePair(path: string): boolean {
    if (!this.host.isOpen(path)) return false;
    const home = this.homeEntry(path);
    // The home file is collapsed onto the folder's row only when it itself still
    // renders (an active-path home file is in the breadcrumb, not a body row).
    const registry = this.host.registry();
    return home !== null && home.path !== registry.pathOf(registry.activeEntryId);
  }

  /**
   * Resolve a folder's ELECTED home file: its registry record, or null. The
   * ONE owner of the election read — buildGroup, the height count and the
   * surface/menu gates all go through it, so what counts as a home child can
   * never differ between the row that renders the pair and the count that
   * sizes it. dirIndex holds the full scan, so this works for collapsed
   * folders too.
   *
   * The decision comes from the BACKEND (`TreeNode.is_home`): one election per
   * folder — `home: true` marker › index stems › self-named note › first doc
   * alphabetically (`moss_core::home::detect_home_file_in_folder_marked`,
   * real root basename) — the same election the build runs, so the tree can
   * never disagree with what the built site serves at the folder URL. Never
   * re-derive it from filenames on this side.
   */
  homeEntry(folderPath: string): EntryRecord | null {
    return this.host.children(folderPath)?.find(e => !e.isDir && e.isHome) ?? null;
  }

  /**
   * The nav surface's height as the model has it: `desiredHeight` (E4, in
   * tree/spine-projection.ts) over the rendered forest, the park and the
   * floor. Every reader of "how tall is the surface" — the wrapper write,
   * isAtMinHeight, the sash's pointerdown baseline — derives from this rather
   * than parsing `style.height` back out of the DOM.
   */
  surfaceHeight(): number {
    return desiredHeight(this.renderedRowCount(), this.host.view().park, this.host.floor());
  }

  /** Size the nav surface to the rendered content (see desiredHeight). */
  applyWrapperHeight(): void {
    if (!this.wrapper) return;
    this.setWrapperHeight(this.surfaceHeight());
    // .nav-body has overflow-y: auto unconditionally; the breadcrumb moved into
    // .nav-bc-strip (a sibling outside the scrollport), so scrollHeight equals
    // tree-row content and a scrollbar appears only when content exceeds floor.
  }

  /**
   * Set the nav-surface wrapper height. Sets BOTH `height` and `max-height`:
   * in WKWebView the surface is a `flex` item whose `flex: 0 0 auto` + explicit
   * `height` is supposed to be authoritative, but the empty `.nav-body` (flex:1)
   * keeps a ~20px min-content that makes the flex container size to content and
   * IGNORE the shorter `height` — so a "collapsed to 48px" surface renders 68px,
   * leaving a residual gap below the breadcrumb. `max-height` is honored where
   * `height` alone is not (verified live), so it pins the box to the intended
   * height. (Same class of WebKit-only flex quirk as the subgrid-scroller bug.)
   */
  setWrapperHeight(px: number): void {
    if (!this.wrapper) return;
    this.wrapper.style.height = `${px}px`;
    this.wrapper.style.maxHeight = `${px}px`;
  }

  /**
   * Re-allocate widths/offsets against the already-rendered DOM on panel
   * resize. Does NOT rebuild the tree. Writes only when an offset changes by
   * >= 1px: output quantization stops a 1px drag from reflowing every row.
   */
  private reallocateSpine(): void {
    // Never re-run the allocator while an inline create/rename input is mounted:
    // the allocator rewrites each seg's inline `flex` to '0 0 Wpx', which
    // would clobber the '1 1 auto' widen beginSegEdit() applied to the edited
    // breadcrumb seg (and pin the input back to the narrow measured width). The
    // render() path is already isEditing-gated at its top; this is the only OTHER
    // caller (ResizeObserver → Spine.schedule), so it needs the same gate.
    // The edit teardown triggers a fresh render that re-allocates correctly.
    if (this.host.gate.isEditing) return;
    const strip = this.navBcStrip;
    if (!strip) return;
    const segEls = [...strip.querySelectorAll<HTMLElement>('.seg.bc')];
    if (!segEls.length) return;

    const prev = this.spine.snapshot();
    this.spine.apply(strip, segEls);
    // Re-measure truncation and add/remove data-tooltip: a panel/divider resize
    // changes each seg's allocated width, so which segments ellipsize changes too
    // (BUG 16). Runs regardless of the offset-quantization bail below — the
    // quantization guard is about tree-row RE-PLACEMENT, not the tooltip state.
    this.spine.syncTooltips(segEls);
    const next = this.spine.snapshot();
    if (!next) return;

    // Output quantization: bail if no offset moved >= 1px.
    const changed = !prev || next.length !== prev.length ||
      next.some((o, i) => Math.abs(o - prev[i]) >= 1);
    if (!changed) return;

    // Re-apply offsets to existing tree rows (keyed by data-depth). MUST use
    // the same offsetAt() the render path uses — NOT a private formula — or the
    // two drift: offsetAt indents sibling subtrees structurally (FT_STRUCTURAL_
    // INDENT past the deepest meaningful spine column), while the old inline
    // formula here reused next[depth] (the surplus-folded leaf offset) + the
    // wide 64px step, silently reverting the bug #1 indent fix on every resize.
    const rows = this.navBody?.querySelectorAll<HTMLElement>('.seg') ?? [];
    rows.forEach((row) => {
      const depth = parseInt(row.getAttribute('data-depth') ?? '', 10);
      if (!Number.isFinite(depth)) return;
      row.style.setProperty('--ft-offset', `${this.spine.offsetAt(depth)}px`);
    });

    // After the structural offsets are re-applied above, re-run the
    // home-pair child alignment so a folder-note pair's children keep
    // tracking the inline home file across a panel resize (it shifts the
    // home file's content-dependent x). See alignHomePairChildren().
    this.alignHomePairChildren();
    // Rows just moved (spine re-alloc), so re-point the connector rails at
    // their new columns — without this, deep-nesting rails shear on resize.
    this.syncRailOffsets();
  }

  /**
   * Align the children of every folder-note pair under the pair's INLINE home
   * file (the "docs · index" row). This fixes the content-dependent
   * misalignment: the home file `index` flows inline after the folder name, so
   * its icon x = folder-icon-x + folder-content-width + gap — it varies with
   * the folder-name length and cannot be derived from a fixed per-depth
   * formula. The folder's OTHER children are normally placed at
   * offsetAt(depth+1) (a fixed indent), so their icons land LEFT of the home
   * file. We MEASURE the home file's rendered icon position and re-offset the
   * direct children to match, so `index`, `getting-started`, `editors`, … share
   * one icon column.
   *
   * Approach (measure-and-align, post-layout):
   *  1. Read --moss-traffic-light-inset (the concrete source --ft-tl aliases) +
   *     --ft-base off .nav-grid — the same origin the row margin's calc() uses.
   *  2. For each `.seg-pair` with a `.home-inline` child:
   *     a. Measure the home file's icon (`.dot`/`.chev`) left relative to the
   *        .nav-body content origin → homeIconLeft.
   *     b. targetOffset = homeIconLeft - (ftTl + ftBase) — the --ft-offset that
   *        places a normal row's icon at homeIconLeft (row icon left =
   *        navBodyOrigin + ftTl + ftBase + --ft-offset; see file-tree.css).
   *     c. Find ALL descendant rows: the rows AFTER the pair's folder, in
   *        flat DOM order, whose data-depth > folderDepth, stopping at the
   *        first row whose data-depth <= folderDepth (no longer a
   *        descendant). Re-offset EVERY descendant — not just direct
   *        children — by the delta between targetOffset and the plain
   *        structural offsetAt(folderDepth+1), so a non-pair intermediate
   *        folder's own children stay correctly nested under its
   *        ad-hoc-shifted position instead of falling back to a
   *        breadcrumb-relative formula that doesn't know the ancestor moved.
   *
   * Recursion is handled implicitly: we process EVERY pair in the pass, so a
   * grandchild pair's own direct children are re-offset AGAIN, more
   * accurately, when that pair is processed. (A grandchild pair's home file
   * flows inline after ITS folder, which has already been pushed right by
   * the delta above, so the grandchild's targetOffset is measured AFTER its
   * parent moved — process order doesn't matter because each pair
   * re-measures its own home file's live position, and querySelectorAll's
   * document order guarantees a pair is always processed before its own
   * descendant pairs.)
   *
   * jsdom guard: getBoundingClientRect() returns all-zeros under jsdom (no
   * layout), so homeIconLeft would be 0 and we'd write a bogus negative offset.
   * We no-op the whole pass when measurements are flat (home icon and nav-body
   * both report left=0). The real alignment is verified on-device, not in jsdom.
   */
  private alignHomePairChildren(): void {
    const body = this.navBody;
    if (!body) return;
    const pairs = body.querySelectorAll<HTMLElement>('.seg-pair');
    if (pairs.length === 0) return;

    const grid = body.closest('.nav-grid') as HTMLElement | null;
    const cs = grid ? getComputedStyle(grid) : null;
    const ftTl = readPxVar(cs, '--moss-traffic-light-inset', 76);
    const ftBase = readPxVar(cs, '--ft-base', 8);

    const bodyRect = body.getBoundingClientRect();
    // .nav-body content origin: no padding-inline-start (the depth indent lives
    // in each seg's margin, not on the body), and overflow-x is hidden so there
    // is no horizontal scroll to subtract. So the content origin is bodyRect.left.
    const bodyLeft = bodyRect.left;

    // Flat DOM-order list of all placed rows (data-depth carriers). querySelectorAll
    // returns document order, flattening the display:contents group wrappers, so a
    // pair's folder and its descendants appear here in render order. State rows
    // (a hint, "empty") are a folder's lines too and move with its files.
    const allRows = [...body.querySelectorAll<HTMLElement>('.seg[data-depth], [data-state-row][data-depth]')];

    for (const pair of pairs) {
      const homeEl = pair.querySelector<HTMLElement>('.home-inline');
      const fSeg = pair.querySelector<HTMLElement>('.seg[data-depth]');
      if (!homeEl || !fSeg) continue;

      const folderDepth = parseInt(fSeg.getAttribute('data-depth') ?? '', 10);
      if (!Number.isFinite(folderDepth)) continue;

      const homeIcon = homeEl.querySelector<HTMLElement>('.dot, .chev');
      if (!homeIcon) continue;
      const iconRect = homeIcon.getBoundingClientRect();

      // jsdom guard: with no layout every rect is zero, so iconRect.left and
      // bodyLeft are both 0 → homeIconLeft 0 → a bogus negative targetOffset.
      // Skip when measurements are flat (verified on-device, not in jsdom).
      if (iconRect.left === 0 && iconRect.right === 0 && bodyLeft === 0) {
        continue;
      }

      const homeIconLeft = iconRect.left - bodyLeft;
      const targetOffset = homeIconLeft - (ftTl + ftBase);
      if (!Number.isFinite(targetOffset)) continue;

      // Every descendant of this pair, not just its direct children (step c
      // of the doc above): shift each one by the same delta, and stop at the
      // first row that is no longer inside the folder's subtree.
      const start = allRows.indexOf(fSeg);
      if (start === -1) continue;
      const delta = targetOffset - this.spine.offsetAt(folderDepth + 1);
      for (let i = start + 1; i < allRows.length; i++) {
        const row = allRows[i];
        const d = parseInt(row.getAttribute('data-depth') ?? '', 10);
        if (!Number.isFinite(d)) continue;
        if (d <= folderDepth) break; // left this folder's subtree
        row.style.setProperty('--ft-offset', `${this.spine.offsetAt(d) + delta}px`);
      }
    }
  }

  /**
   * Point every sibling-group's connector rail at its children's icon column.
   *
   * The rail is a CSS `::before` on each `role="group"` wrapper positioned at
   * `--ft-rail-offset` (file-tree.css). This copies each group's FIRST direct
   * child row's live `--ft-offset` onto the wrapper, so the rail sits exactly
   * under the children's icons — including after `alignHomePairChildren()` has
   * shifted a folder-note pair's children right by a measured delta.
   *
   * Reads an inline CSS var we set in `place()`/`alignHomePairChildren()`, NOT a
   * `getBoundingClientRect()` — so it is measurement-free and safe to run
   * synchronously: a measured version produces a non-deterministically
   * missing/stale connector. MUST be called after every
   * mutation of row `--ft-offset` (post-render, post-spine-realloc).
   *
   * The DIRECT-child selector (a direct `.seg` row, or a direct `.seg-pair`'s
   * folder `fSeg`) can never descend into a nested `[role="group"]` and read a
   * deeper row's column. The FIRST match (document order) is this group's own
   * first row → its `--ft-offset` drives the rail's horizontal column.
   *
   * VERTICAL CAP (--ft-rail-rows): the rail must span from the wrapper
   * top down to the group's LAST DIRECT-CHILD row's icon-center and STOP there.
   * The wrapper CONTAINS each child folder's nested `[role="group"]`, so its
   * full height runs PAST the last direct child whenever that child is an
   * EXPANDED folder — a CSS offset measured off the wrapper bottom would
   * dangle the rail below the folder's chevron, through the folder's own
   * subtree. Every visual row is exactly one `.seg[data-depth]` (a
   * home-pair's inline `.home-inline` seg carries no data-depth), and rows lay
   * out in document order at a uniform pitch, so the last direct child's 0-based
   * DOCUMENT INDEX among ALL rows in the wrapper IS its visual row offset from
   * the top. We write that index to `--ft-rail-rows`; the CSS turns it into a
   * height. Count-based, so it stays measurement-free (no getBoundingClientRect)
   * and correct under jsdom. Middle expanded children are untouched — the rail
   * still passes through them to reach later direct children.
   *
   * TOP per group kind (--ft-rail-extend-top, 0/1): an OUTERMOST group (directly
   * under `.nav-body`) has its parent folder in the BREADCRUMB spine, not as an
   * adjacent row — keep top:0 so the rail tucks under the breadcrumb hairline
   * (short first segment). A NESTED group's parent IS the tree row right above
   * it — extend the top up to that row's icon-center so the first child→parent
   * segment equals the inter-child pitch (the CSS raises top by half a row + the
   * 1px group gap). See file-tree.css.
   */
  private syncRailOffsets(): void {
    const body = this.navBody;
    if (!body) return;
    for (const wrapper of body.querySelectorAll<HTMLElement>('[role="group"]')) {
      // Direct child ROWS of this wrapper (a bare `.seg` row, or a `.seg-pair`'s
      // folder row) — NOT its nested `[role="group"]` subtrees. Walk
      // `wrapper.children` directly rather than a `:scope > …, :scope > …`
      // selector: jsdom mis-scopes the comma form and leaks descendant rows in,
      // which would push the cap into the nested subtree (the very bug below).
      let firstRow: HTMLElement | null = null;
      let lastRow: HTMLElement | null = null;
      for (const child of wrapper.children) {
        let row: HTMLElement | null = null;
        if (child.matches('.seg[data-depth]')) row = child as HTMLElement;
        else if (child.classList.contains('seg-pair')) {
          row = child.querySelector<HTMLElement>('.seg[data-depth]');
        }
        if (!row) continue;
        if (!firstRow) firstRow = row;
        lastRow = row;
      }

      const off = firstRow?.style.getPropertyValue('--ft-offset');
      if (off) wrapper.style.setProperty('--ft-rail-offset', off);

      // Vertical cap: the LAST direct child's visual row index within this
      // wrapper (its 0-based DOCUMENT INDEX among ALL rows here, nested included,
      // since rows lay out in document order at a uniform pitch).
      //
      // A hint row is one row of that pitch, so it counts. A hint that wraps
      // (a narrow panel) is taller than one pitch: its excess height above the
      // last row is the one measured term, written as --ft-rail-extra.
      if (lastRow) {
        const allRows = [...wrapper.querySelectorAll<HTMLElement>('.seg[data-depth], .nav-hint-row')];
        const idx = allRows.indexOf(lastRow);
        if (idx >= 0) wrapper.style.setProperty('--ft-rail-rows', String(idx));
        let extra = 0;
        for (const row of allRows.slice(0, Math.max(idx, 0))) {
          if (row.classList.contains('nav-hint-row')) extra += Math.max(0, row.offsetHeight - ROW_H);
        }
        wrapper.style.setProperty('--ft-rail-extra', `${extra}px`);
      }

      // Rail top per group kind (see the doc above): a group is nested iff it
      // sits inside another [role="group"].
      const nested = wrapper.parentElement?.closest('[role="group"]') != null;
      wrapper.style.setProperty('--ft-rail-extend-top', nested ? '1' : '0');
    }
  }

  render(): void {
    // Defer destructive re-renders while an inline rename/create input is
    // mounted — the `replaceChildren()` below would wipe the input the user is
    // typing into — or while a listTree round trip is rewriting the indexes
    // this reads. The controller's `flushPendingWork` re-runs whatever was parked.
    if (!this.host.gate.admit('render')) return;
    const strip = this.navBcStrip;
    const body = this.navBody;
    const container = this.container;
    if (!strip || !body || !container) return;

    // Detaching every row would reset the body's scroll offset. Restored at
    // the end, after applyWrapperHeight has settled the scrollable range.
    const savedScrollTop = body.scrollTop;

    // Row reconciliation: the strip and body are emptied, but their
    // ROWS are not destroyed — they are parked in rowCache by EntryId, and
    // seg() takes each one back for the entry it belongs to. A row that
    // still exists after the render is the same DOM object it was before,
    // which is what keeps a JS-set --ft-offset alive across a refresh and
    // what lets the browser pair two clicks on it, so the second click of a
    // double-click arrives as detail=2 and stays inert instead of toggling
    // a folder shut again. Rows the render
    // does not claim are left in the cache and dropped below, which is how
    // a deleted entry's row leaves the tree.
    //
    // The wrappers (role="group", .seg-pair) are rebuilt wholesale: they
    // carry no identity and no state worth preserving.
    this.rowCache.clear();
    for (const el of container.querySelectorAll<HTMLElement>('.seg[data-entry-id]')) {
      this.rowCache.set(Number(el.dataset.entryId) as EntryId, el);
    }
    strip.replaceChildren();
    mkTitleBarLayer(strip);
    body.replaceChildren();

    if (this.host.view().anchor.length === 0) return;

    // The body renders the open-folder forest (tree/spine-projection.ts):
    // EVERY open folder, not just the active file's ancestors. The breadcrumb
    // chevron reflects the same per-path open state.
    const openPaths = this.openPathsMap();

    // ── Breadcrumb segments ──────────────────────────────────────────
    // Segments are appended in depth order into .nav-bc-strip; the strip's
    // flex layout positions them (each seg: flex 0 0 --ft-indent, last:
    // flex 1 1 auto). No grid placement needed.
    //
    // For files (last segment), the .md extension is stripped via
    // fileDisplayParts; other extensions render as a dimmed suffix
    // matching tree-row treatment.
    const segEls: HTMLElement[] = [];
    for (let i = 0; i < this.host.view().anchor.length; i++) {
      const path = this.host.view().anchor[i];
      const name = displayName(path, this.host.projectRoot());
      const isFolder = this.host.isDirectory(path);
      // Parked → paint closed, or the next click hits the CLOSE branch blind.
      const isOpen = this.host.isOpen(path) && !isParkedHeight(this.host.view().park);
      const isLast = i === this.host.view().anchor.length - 1;

      // Apply extension hiding to the last segment (file). Folders render verbatim.
      const parts = !isFolder ? fileDisplayParts(name) : { stem: name, extHint: '' };
      const el = this.seg(path, parts.stem, isFolder, isOpen ? 'open' : '', 'bc', isLast ? 'bc-current' : '');
      // WAI-ARIA tree: aria-level is 1-indexed; breadcrumb segments use the
      // activePath index + 1 (root = 1, first child = 2, etc.)
      el.setAttribute('aria-level', String(i + 1));
      // "You are here" — the current/leaf segment is the active page (design P5,
      // NN/g). The full (untruncated) name is already on aria-label below, so a
      // CSS-truncated label stays WCAG 2.5.3-compliant.
      if (isLast) el.setAttribute('aria-current', 'page');
      appendExtHint(el, parts.extHint);
      el.dataset.fullName = name;
      el.setAttribute('aria-label', name);
      // Portal tooltip (moss convention: `data-tooltip=`, never native `title=`;
      // see .claude/CLAUDE.md → UI Conventions). The `.nav-bc-strip` header strip
      // is NOT a floating overlay (unlike `.dropdown`/`.ctx-menu`), so a
      // `data-tooltip=` is correct here rather than `aria-label`-only.
      //
      // BUG 16 refinement: the tooltip is set ONLY when the segment is actually
      // truncated (its label overflows into an ellipsis) — a fully-visible segment
      // echoing its own visible name is noise. Truncation is a LAYOUT fact known
      // only AFTER the spine allocator pins each seg's width, so it is NOT set here
      // at build time; Spine.syncTooltips sets/removes it post-allocation
      // (see the spine.apply call below, and reallocateSpine on resize).
      // The untruncated full name stays on data-fullName above as the tooltip source.

      // Breadcrumb segments carry the same data-path/data-bc-index the
      // delegated handlers read on body rows, so gesture behaviour is shared
      // by construction rather than by a parallel implementation (bug #2).
      // Draggable is an attribute, not a listener, so it survives row reuse.
      // The project root is a valid drop TARGET but not a drag source — you
      // cannot "drag the project root".
      el.draggable = path !== PROJECT_ROOT_REL;
      el.dataset.bcIndex = String(i);

      // Append in depth order; strip flex layout positions each seg.
      strip.appendChild(el);
      segEls.push(el);
      // No explicit separator span — each folder seg's leading .chev
      // already draws `›` (and rotates when open). See ecb77095.
    }

    // ── Adaptive spine allocation ──────────────────────────────────────
    // Measure natural content, allocate a single rounded offset array, apply
    // it to the breadcrumb segments here and (via offsetAt) the tree rows
    // below — one shared array, so the spine aligns by construction.
    // In jsdom / a detached or hidden panel, panelW is 0 → skip live
    // allocation (offsetAt falls back to depth*--ft-indent, the prior
    // fixed-indent behavior) so unit tests and collapsed panels are safe.
    this.spine.apply(strip, segEls);
    // BUG 16: after the segments are sized, set data-tooltip ONLY on the ones
    // whose label actually truncates (ellipsis). Post-allocation because
    // truncation is a layout fact, not knowable at build time (above).
    this.spine.syncTooltips(segEls);

    // ── Tree body: the deepest-first spine body ─────────────
    // The body renders the active path's ancestors' children DEEPEST-FIRST, so
    // the active file's SIBLINGS sit directly under the breadcrumb, then the
    // level above, etc. (See tree/spine-projection.ts / spineBody for the pure model — the
    // DOM order here MUST equal spineBody's row order.)
    //
    // For each ancestor folder of the active file, from the deepest (the active
    // file's own parent) down to the project root, IF that ancestor is open we
    // render its children at the ancestor's child depth via buildGroup. buildGroup
    // EXCLUDES every active-path node (the active child at this level is a
    // breadcrumb crumb, not a body row) and recurses normally into non-active
    // open subfolders. With nothing open the body is empty (breadcrumb-only
    // default). Active ancestors are breadcrumb crumbs, so they never render a
    // body folder row or a home-inline pair.
    // Folder-valued leaf: when the active
    // path's own leaf is itself a directory (a generated home `['']`, or dir
    // mode's `['', 'docs']`), it counts as the DEEPEST openable level too —
    // see tree/spine-projection.ts's FOLDER-VALUED LEAF note. Rendered first (deepest-
    // first order), so it sits directly under the breadcrumb exactly like any
    // other open ancestor's children. The folder itself never gets a body
    // row here — only in the breadcrumb (integrated-spine invariant).
    const leafPath = this.host.view().anchor[this.host.view().anchor.length - 1];
    if (this.host.view().anchor.length > 0 && this.host.isDirectory(leafPath) && openPaths.has(leafPath)) {
      const leafWrapper = mkGroup(this.spine.offsetAt(this.host.view().anchor.length));
      body.appendChild(leafWrapper);
      this.renderLevelBody(leafWrapper, leafPath, this.host.view().anchor.length, openPaths);
    }

    // Ancestor folders are activePath indices 0..length-2; the last entry is the
    // active file (leaf). Walk DEEPEST first.
    for (let i = this.host.view().anchor.length - 2; i >= 0; i--) {
      const folder = this.host.view().anchor[i];
      if (!openPaths.has(folder)) continue; // that level shows iff its folder is open
      // WAI-ARIA APG tree pattern: each ancestor-level sibling group is a
      // role="group" wrapper (mkGroup) — a containment boundary for assistive
      // tech AND a layout-neutral flex box that hosts the sibling connector rail
      // (::before at --ft-rail-offset = the children's column i+1).
      // Ref: https://www.w3.org/WAI/ARIA/apg/patterns/treeview/
      const levelWrapper = mkGroup(this.spine.offsetAt(i + 1));
      body.appendChild(levelWrapper);

      this.renderLevelBody(levelWrapper, folder, i + 1, openPaths);
    }

    // Align home-pair children SYNCHRONOUSLY, then sync the connector rails to
    // the (now final) row columns — BOTH synchronous, no rAF. rAF is
    // throttled-to-dead in the idle action-panel WKWebView (see Spine's timer
    // comment), which is exactly why the old connector line (drawn in a rAF)
    // went missing/stale non-deterministically. alignHomePairChildren() forces
    // the pending layout so its measurements are valid; syncRailOffsets() then
    // reads each group's live child column into its rail var (no measurement).
    this.alignHomePairChildren();
    this.syncRailOffsets();

    // The height is written from the same pass, so every caller of render()
    // (file-watcher refreshes included) keeps the surface in sync. Cheap: pure
    // arithmetic. No-op until `wrapper` is set.
    this.applyWrapperHeight();

    // Re-apply .selected and aria-selected on the freshly rendered DOM so a
    // multi-row selection survives any render() triggered between selection
    // changes (e.g., folder toggle, file-watcher rebuild). render() builds
    // fresh DOM without the visual state; this restores it.
    this.renderSelectionVisualState();

    // Rows nothing claimed this pass are gone from the tree; drop the
    // references so they can be collected.
    this.rowCache.clear();

    // Restore the scroll position saved before the rebuild above.
    // Placed after applyWrapperHeight() (which can change the scrollable
    // range) so scrollTop isn't clamped against a stale, pre-resize
    // scrollHeight. A raw pixel restore is sufficient here: toggling a
    // folder only adds/removes rows AFTER that folder's own row, so content
    // above the previous scroll position is unchanged.
    body.scrollTop = savedScrollTop;
  }

  /**
   * Render one open forest level into `wrapper`: `folderPath`'s children at
   * `depth` via buildGroup, or a placeholder state row when the level has
   * nothing to show — 'unavailable' when dirIndex has no entry for the
   * folder at all, 'empty' when it does, the entry is `[]`, and nothing was
   * filtered, 'hidden' when it does, the entry is `[]`, and the folder's own
   * `hidden` count is nonzero. `stateRowKind` (tree/spine-projection.ts) is
   * the one place that decides among the three — spineBody's `renderLevel`
   * calls the same function, so the DOM can never disagree with the row
   * count `renderedRowCount()` derives from that pure model. Shared by
   * render()'s top-level ancestor/folder-valued-leaf loop and buildGroup's
   * own recursion into a nested expanded folder with no home-pair (the
   * home-pair branch calls buildGroup directly — see its comment for why
   * `remaining = []` there is NOT a state row).
   */
  private renderLevelBody(
    wrapper: HTMLElement,
    folderPath: string,
    depth: number,
    openPaths: Map<string, string>,
  ): { firstEl: HTMLElement | null; lastEl: HTMLElement | null } {
    this.appendHint(wrapper, folderPath, depth);
    const entries = this.host.children(folderPath);
    if (entries === null || entries.length === 0) {
      const children = entries === null ? null : entries.map(e => e.path);
      const kind = stateRowKind(children, this.host.hiddenCount(folderPath));
      if (kind === 'empty') return { firstEl: null, lastEl: null };
      const row = mkStateRow(kind, depth, this.spine.offsetAt(depth));
      wrapper.appendChild(row);
      return { firstEl: row, lastEl: row };
    }
    return this.buildGroup(wrapper, entries, depth, openPaths);
  }

  /** The folder's hint row, as the first line of its children's group. The
   *  height count (`spineBody`'s `hasHint`) reads the same predicate. */
  private appendHint(wrapper: HTMLElement, folderPath: string, depth: number): void {
    const d = this.host.derivations.hasHint(folderPath) ? this.host.derivations.at(folderPath) : null;
    if (d !== null) wrapper.appendChild(mkHintRow(d, folderPath, depth, this.spine.offsetAt(depth)));
  }

  /**
   * Build sibling rows for one group, recursing into every open folder.
   * `openPaths` is the open-folder set (key present ⇒ that folder is open and
   * its children render below it). Returns { firstEl, lastEl }.
   */
  private buildGroup(
    nav: HTMLElement,
    siblings: EntryRecord[],
    depth: number,
    openPaths: Map<string, string>,
  ): { firstEl: HTMLElement | null; lastEl: HTMLElement | null } {
    let firstEl: HTMLElement | null = null;
    let lastEl: HTMLElement | null = null;
    // THE INTEGRATED-SPINE INVARIANT: every node on the active path
    // lives in the BREADCRUMB (the tree's horizontal spine), so it must NOT also
    // render as a body row — that double-shows it (the reported duplication: the
    // active file AND its ancestor folders appearing in both breadcrumb and body).
    // We therefore EXCLUDE every active-path node here. Active-path ANCESTOR
    // traversal is owned by render()'s deepest-first per-level loop (it calls
    // buildGroup once per open ancestor at the ancestor's child depth), so
    // buildGroup itself just SKIPS active nodes — exactly the spineBody walkNested
    // contract (tree/spine-projection.ts). The active child at THIS level is the next
    // breadcrumb crumb; its OWN children, if open, are rendered by a deeper
    // iteration of render()'s loop, not by recursing here.
    const active = new Set(this.host.view().anchor);

    // Keep only NON-active siblings as render rows, in original order so the body
    // row sequence (and the connector lines / home-pair alignment that depend on
    // it) matches spineBody exactly.
    const rowEntries = siblings.filter(e => !active.has(e.path));

    // aria-setsize counts only the rows that actually render at this level
    // (WAI-ARIA APG tree pattern).
    const setSize = rowEntries.length;

    let sibIdx = -1; // 0-based index among the rendered rows at THIS level
    for (const entry of rowEntries) {
      sibIdx++;
      const name = baseName(entry.path);
      if (!entry.isDir) {
        // File row
        const { stem, extHint } = fileDisplayParts(name);
        const el = this.seg(entry.path, stem, false);
        // WAI-ARIA: tree-body rows are nested under their depth (root sibs = level 2)
        el.setAttribute('aria-level', String(depth + 1));
        el.setAttribute('aria-posinset', String(sibIdx + 1));
        el.setAttribute('aria-setsize', String(setSize));
        el.dataset.fullName = name;
        appendExtHint(el, extHint);
        el.setAttribute('aria-label', name);
        // Drag source: carries the WHOLE multi-selection (or just this row
        // if it is not selected). draggable=true is required — without it
        // the browser never fires dragstart in production even though a
        // manual dispatchEvent works in tests (B5, draggability plan).
        el.draggable = true;

        nav.appendChild(place(el, depth, this.spine.offsetAt(depth)));
        if (!firstEl) firstEl = el;
        lastEl = el;

      } else if (!openPaths.has(entry.path)) {
        // Collapsed folder
        const el = this.seg(entry.path, name, true);
        el.setAttribute('aria-label', name);
        el.setAttribute('aria-level', String(depth + 1));
        el.setAttribute('aria-posinset', String(sibIdx + 1));
        el.setAttribute('aria-setsize', String(setSize));
        el.draggable = true;

        nav.appendChild(place(el, depth, this.spine.offsetAt(depth)));
        if (!firstEl) firstEl = el;
        lastEl = el;

      } else {
        // Expanded folder: show folder, then ALL direct children sorted home-first
        const fSeg = this.seg(entry.path, name, true, 'open');
        fSeg.setAttribute('aria-label', name);
        fSeg.setAttribute('aria-level', String(depth + 1));
        fSeg.setAttribute('aria-posinset', String(sibIdx + 1));
        fSeg.setAttribute('aria-setsize', String(setSize));
        fSeg.draggable = true;

        const childEntries = this.host.children(entry.path) ?? [];

        // If the folder renders an inline home-pair (the SAME predicate the
        // height count uses — review #4), put its ELECTED home file (backend
        // is_home flag; the backend also sorts it first) on the folder's row.
        // rendersAsHomePair() also excludes an ACTIVE home file (it's in the
        // breadcrumb), so it never double-renders here.
        const homeEntry = this.rendersAsHomePair(entry.path) ? this.homeEntry(entry.path) : null;

        if (homeEntry) {
          // Folder + its self-named home file share ONE visual row ("docs · index").
          // The folder sits at its depth column and sizes to its content; the home
          // file flows inline right after it (CSS .seg-pair). We do NOT column-lock
          // the home file to the child column — that squeezed the folder name to
          // zero width and made the folder vanish when expanded.
          const pair = document.createElement('div');
          pair.className = 'seg-pair';
          place(fSeg, depth, this.spine.offsetAt(depth)); // fSeg gets --ft-offset so .nav-body .seg padding resolves
          pair.appendChild(fSeg);
          if (!firstEl) firstEl = fSeg;

          const homeName = baseName(homeEntry.path);
          const { stem: homeStem, extHint: homeExtHint } = fileDisplayParts(homeName);
          const homeEl = this.seg(homeEntry.path, homeStem, false, 'home-inline');
          homeEl.dataset.fullName = homeName;
          homeEl.setAttribute('aria-label', homeName);
          // Home file is rendered as a child of its folder, so its ARIA level
          // is one deeper than the parent folder.
          homeEl.setAttribute('aria-level', String(depth + 2));
          appendExtHint(homeEl, homeExtHint);
          homeEl.draggable = true;
          pair.appendChild(homeEl);
          nav.appendChild(pair);

          // Render remaining children (skip home file) — each child group is
          // its own role="group" wrapper (WAI-ARIA APG) hosting the sibling rail
          // at the children's column (depth+1).
          const remaining = childEntries.filter(e => e !== homeEntry);
          const childWrapper = mkGroup(this.spine.offsetAt(depth + 1));
          nav.appendChild(childWrapper);
          this.appendHint(childWrapper, entry.path, depth + 1);
          const sub = this.buildGroup(childWrapper, remaining, depth + 1, openPaths);
          lastEl = sub.lastEl || fSeg;
        } else {
          nav.appendChild(place(fSeg, depth, this.spine.offsetAt(depth)));
          if (!firstEl) firstEl = fSeg;

          // Each child group is its own role="group" wrapper (WAI-ARIA APG)
          // hosting the sibling rail at the children's column (depth+1).
          const childWrapper2 = mkGroup(this.spine.offsetAt(depth + 1));
          nav.appendChild(childWrapper2);
          const sub = this.renderLevelBody(childWrapper2, entry.path, depth + 1, openPaths);
          lastEl = sub.lastEl || fSeg;
        }
      }
    }

    return { firstEl, lastEl };
  }

  /**
   * Row elements from the render in progress's PREDECESSOR, keyed by
   * EntryId. Populated at the top of render() and consumed by `seg()`;
   * anything left in it when render() ends was removed from the tree.
   */
  private readonly rowCache = new Map<EntryId, HTMLElement>();

  /**
   * The element to render `path`'s row into — the one that already
   * represents it, reset in place, or a fresh one. Identity is the EntryId,
   * never the path, so a rename keeps the row the user's second click has to
   * land on. `data-entry-id` is what the next render reads it back by.
   */
  private seg(path: string, name: string, isFolder: boolean, ...classes: string[]): HTMLElement {
    const id = this.host.registry().getByPath(path);
    const cached = id === null ? undefined : this.rowCache.get(id);
    const el = cached === undefined
      ? mkSeg(name, isFolder, ...classes)
      : fillSeg(cached, name, isFolder, classes);
    if (id !== null) {
      this.rowCache.delete(id);
      el.dataset.entryId = String(id);
    }
    el.dataset.path = path;
    const count = this.host.preflightEvidence.count(path);
    if (count > 0) {
      el.dataset.preflightCount = String(count);
      const badge = document.createElement('span');
      badge.className = 'tree-preflight-count';
      badge.textContent = String(count);
      badge.setAttribute('aria-label', t(count === 1 ? 'tree.preflight_count_one' : 'tree.preflight_count_many', { count }));
      el.appendChild(badge);
    }
    return el;
  }
}
