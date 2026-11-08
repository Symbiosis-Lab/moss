/**
 * Shared context-menu primitive.
 *
 * Design-vocabulary rule: "Every surface owns its right-click; the native
 * webview menu never leaks." A surface shows a moss menu built on THIS module
 * (or deliberately suppresses).
 *
 * One implementation for every moss context menu:
 *   - positioning at a point or anchored under an element, with viewport
 *     clamping (shared `clampIntoViewport`, also used by other overlays)
 *   - dismissal on outside-click, webview blur, Escape, and — for a menu the
 *     preview opened — a press or Escape back inside the preview iframe
 *   - `role="menu"` / `role="menuitem"`, roving focus, ArrowUp/Down +
 *     Home/End + Enter/Space activation, focus returned to the invoker
 *   - `.ctx-kbd` shortcut chips and `.ctx-hint` notes (muted, non-interactive
 *     text anywhere in the list)
 *   - drill-in levels: a `submenu` entry replaces the rows in place under a
 *     back row; ArrowLeft / Escape / the back row return one level. Menus
 *     drill in, they never cascade — one element, one position, one
 *     dismissal for every depth
 *   - checklist rows (`checkbox` entry, `role="menuitemcheckbox"`): Space,
 *     Enter or a click flips the row and the menu stays open
 *   - a drill-in level may be filterable (`submenu.filter`): typing narrows its
 *     checkbox rows. A long level gets a real text field (`field: true`) that
 *     holds focus — the combobox pattern: ArrowUp/Down move an active row named
 *     by `aria-activedescendant`, Enter/Space toggle it, Escape clears the
 *     text first. A short level has no field and the keys are taken directly
 *   - Shift+F10 / ContextMenu-key opening for surfaces that register for it
 *     (see `isCtxMenuKey`)
 *
 * Styles live in `styles/shared/ctx-menu.css`, imported by BOTH the shell's
 * and the editor's stylesheets so a menu renders identically in either
 * webview.
 *
 * Focus policy: a mouse-opened menu does NOT steal focus (no blur
 * side effects in the editor); the
 * first ArrowDown/ArrowUp moves real DOM focus into the items. A
 * keyboard-opened menu (`focusFirst: true`) focuses the first item
 * immediately, per the WAI-ARIA menu pattern. On close, focus returns to the
 * invoker only if the menu had taken it.
 */

import { t } from '../../i18n';
import { uiEvent } from './toast/toast-runtime';
import { armOutsideDismiss } from './dismiss-outside';
import {
  buildEmptyHint, buildFilterField, buildQueryLine, createComboboxCursor, filterMatch, firstMatch, nextQuery,
  type Cursor, type FilterSpec, type FilterState,
} from './ctx-menu-filter';

/** Margin from the viewport edge when clamping into view. */
const VIEWPORT_EDGE_PADDING = 8;

/** Pixel gap between an anchored menu and the bottom edge of its anchor. */
const MENU_ANCHOR_GAP = 4;

export interface CtxMenuItem {
  kind?: 'item';
  label: string;
  /**
   * Stable, locale-independent handle for this row, rendered as `data-action`
   * on the row element (the app's existing convention for this, such as a
   * settings row's `data-action="add"`). `label` is
   * translated text and a row's position shifts as sibling entries come and
   * go, so neither is a handle a script (or a future automated check) can
   * target reliably; this is. A static row's own short handle reads well
   * here — an English slug matching its i18n key, not the dotted key itself
   * (e.g. `'new-page'` for `t('tree.new_page')`); a per-data row (one saved
   * template, one schema field) wants a computed id instead (e.g.
   * `template:${meta.id}`).
   * Optional — a row with no caller-assigned id simply carries no attribute.
   */
  id?: string;
  /** Shortcut chip text (e.g. '⌘N'), rendered right-aligned in `.ctx-kbd`. */
  kbd?: string;
  /** Muted text right-aligned on the row (a count). */
  detail?: string;
  /** Destructive action — rendered in the danger color. */
  danger?: boolean;
  /** Run `action` but leave the menu open and repaint the level (a "Select all"
   *  inside a checklist). Without it an item closes the menu first. */
  keepOpen?: boolean;
  /** Rendered but inert: skipped by roving focus, click does nothing. */
  disabled?: boolean;
  /** Marks the row as the current choice (`aria-current`), as on a submenu row. */
  current?: boolean;
  /**
   * Runs after the menu closes. Receives the activating MouseEvent (keyboard
   * activation synthesizes one carrying the item's on-screen coordinates, so
   * actions that open a follow-up surface at the click point keep working).
   */
  action(ev: MouseEvent): void;
}

/**
 * An entry that owns a child level. Activating it (click, Enter, Space,
 * ArrowRight) replaces the visible rows with `entries` under a back row
 * carrying `label`; ArrowLeft, Escape and the back row return. Rendered with
 * `aria-haspopup="menu"` and a CSS chevron; no `aria-expanded`, because the
 * row is detached while its child shows, so the attribute could never be
 * observed in either state.
 */
export interface CtxMenuSubmenu {
  kind: 'submenu';
  label: string;
  /** A function is re-read every time the level paints, so a level that shows
   *  live state (counts, ticks) stays true after a toggle or a Back. */
  entries: CtxMenuEntry[] | (() => CtxMenuEntry[]);
  /** Same `data-action` handle as `CtxMenuItem.id` — see its doc. */
  id?: string;
  /** Muted text right-aligned on the row and on the back row of its level
   *  ("127 of 129"). */
  detail?: string | (() => string);
  /**
   * Makes the row two targets: the row itself runs `action` and closes the menu
   * (Enter, Space, click), and only the chevron — or ArrowRight — drills in.
   * Without it the whole row drills in.
   */
  action?(ev: MouseEvent): void;
  /** Marks the row as the current choice (`aria-current`). */
  current?: boolean;
  /** Type-to-filter for this level's checkbox rows. `hint` is the field's
   *  placeholder and accessible name, `empty` shows when nothing matches. With
   *  `field` the level has a focused text input; without it typing filters
   *  anyway and the query shows on a line only while it is non-empty. */
  filter?: FilterSpec;
}

/**
 * A row with a tick box: `role="menuitemcheckbox"` and `aria-checked`. Activating
 * it flips `checked`, calls `onToggle` and repaints the level; the menu stays
 * open (WAI-ARIA: a checkbox item changes state without closing the menu).
 */
export interface CtxMenuCheckbox {
  kind: 'checkbox';
  label: string;
  checked: boolean;
  /** Muted text right-aligned on the row (an address's language tag). */
  detail?: string;
  /** Same `data-action` handle as `CtxMenuItem.id` — see its doc. */
  id?: string;
  onToggle(checked: boolean): void;
}

/**
 * Non-interactive muted text anywhere in the list: a group label ("From
 * template"), or the trailing
 * destination line ("Creates in <b>posts</b>"). `render` fills a `.ctx-hint`
 * host. Rendered `role="presentation"`, skipped by roving focus.
 */
export interface CtxMenuNote {
  kind: 'note';
  render(host: HTMLElement): void;
}

export type CtxMenuEntry = CtxMenuItem | CtxMenuSubmenu | CtxMenuCheckbox | { kind: 'separator' } | CtxMenuNote;

export interface CtxMenuHandle {
  /** The menu's root element. */
  readonly el: HTMLElement;
  /** Close the menu. Idempotent. Fires `onClose` once. */
  close(): void;
  /** Re-read `entries` and repaint the level on screen, for a list whose rows
   *  change while it is open (search results arriving). Keyboard position and
   *  scroll offset survive, as after a toggle. No-op once closed. */
  refresh(): void;
}

export interface OpenCtxMenuOptions {
  entries: CtxMenuEntry[] | (() => CtxMenuEntry[]);
  /** Extra class(es) on the menu root, space-separated (a wider list than the 160px default). */
  className?: string;
  /**
   * - `point`: open at viewport coordinates (right-click position).
   * - `below`: open under an anchor element; `align: 'right'` lines the
   *   menu's right edge up with the anchor's (create-flow's ▾ caret).
   */
  position:
    | { kind: 'point'; x: number; y: number }
    | { kind: 'below'; anchor: HTMLElement; align?: 'left' | 'right' };
  /**
   * Element treated as "inside" for outside-click — typically the element
   * that spawned the menu, so clicking it again doesn't fight with its own
   * re-open handler (chip-bar convention).
   */
  outsideAnchor?: HTMLElement;
  /** Accessible name for the `role="menu"` container at the root level; a
   *  nested level is named after the submenu entry that opened it. */
  ariaLabel?: string;
  /** Fired exactly once when the menu closes (any path). */
  onClose?(): void;
  /** Keyboard invocation: focus the first enabled item on open. */
  focusFirst?: boolean;
  /**
   * Surface name for the Send Logs UI trail (`uiEvent('menu.open'/'menu.pick',
   * …)`, log.ts). Omit to skip logging — every real surface should
   * pass one; there is no surface today that deliberately opts out.
   */
  logSurface?: string;
  /** `kind:name` the menu targets — a file/folder basename or an item count.
   *  Omitted for a surface with no single target (empty area, the preview). */
  logTarget?: string;
}

/**
 * True for the two conventional open-a-context-menu keystrokes: Shift+F10
 * and the dedicated ContextMenu key. Surfaces that register for keyboard
 * opening route these to their own open call (positioned at the focused
 * row/element, with `focusFirst: true`).
 */
export function isCtxMenuKey(e: KeyboardEvent): boolean {
  return e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10');
}

/**
 * Clamp a fixed-position element fully into the viewport (8px padding).
 * The one shared implementation — the host's other overlays and the
 * URL-import popover use it too.
 */
export function clampIntoViewport(el: HTMLElement): void {
  const r = el.getBoundingClientRect();
  if (r.right > window.innerWidth - VIEWPORT_EDGE_PADDING) {
    el.style.left = `${window.innerWidth - r.width - VIEWPORT_EDGE_PADDING}px`;
  }
  if (r.bottom > window.innerHeight - VIEWPORT_EDGE_PADDING) {
    el.style.top = `${window.innerHeight - r.height - VIEWPORT_EDGE_PADDING}px`;
  }
}

/**
 * Join item groups with a single separator between any two groups that BOTH
 * actually rendered something — never unconditionally around a block that
 * might be entirely skipped. Empty groups vanish, so the double-separator
 * bug (two adjacent rules once conditional items are omitted) cannot occur.
 */
export function joinCtxGroups(groups: CtxMenuEntry[][]): CtxMenuEntry[] {
  const out: CtxMenuEntry[] = [];
  for (const group of groups) {
    if (group.length === 0) continue;
    if (out.length > 0) out.push({ kind: 'separator' });
    out.push(...group);
  }
  return out;
}

/** The one open menu — opening a new one closes it (a context menu is
 *  singular by nature; this also guarantees listener cleanup). */
let activeMenu: CtxMenuHandle | null = null;
let menuSeq = 0;

export function openCtxMenu(opts: OpenCtxMenuOptions): CtxMenuHandle {
  activeMenu?.close();

  const menu = document.createElement('div');
  menu.className = 'ctx-menu';
  menu.setAttribute('role', 'menu');
  if (opts.className) menu.classList.add(...opts.className.split(/\s+/));

  // Clicks inside the menu must not reach document-level handlers (outside-
  // click dismissers, the tree's selection clearing, …) — the same rule every
  // overlay follows. The press is also kept from moving
  // focus: a row is not focusable, so the default action blurred whatever
  // the user was in (the editor) to `body`, and a mouse-driven drill or
  // pop then closed with focus nowhere.
  menu.addEventListener('mousedown', (e) => {
    // A text field must take the press (caret placement); everything else must not.
    if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
    e.stopPropagation();
  });
  menu.addEventListener('click', (e) => e.stopPropagation());
  // Hover moves the roving focus once the keyboard has engaged, so mixed
  // mouse/keyboard use never highlights two items at once — but only a
  // pointer that MOVED counts. The engine also fires mouse events when the
  // rows move under a resting pointer (a keyboard scroll in a menu taller
  // than the panel, a level swap), and taking those as hover stole focus
  // from the row the keyboard had just reached; with a scrolling focus() it
  // looped, row under pointer → focus → scroll → new row (seen in Chromium).
  // The first event only seeds the
  // position; a real move is many events, a synthetic one is one.
  let pointer: { x: number; y: number } | null = null;
  menu.addEventListener('mousemove', (e) => {
    const moved = pointer !== null && (pointer.x !== e.clientX || pointer.y !== e.clientY);
    pointer = { x: e.clientX, y: e.clientY };
    if (!moved || !menu.contains(document.activeElement) || !cursor().followsPointer) return;
    const item = (e.target as Element).closest<HTMLElement>('.ctx-item');
    if (item && actions.has(item)) item.focus({ preventScroll: true });
  });

  // Drill-in levels:
  // one element, a stack of levels, one render function. A level holds the
  // submenu ENTRY that opened it, not its element — renderLevel rebuilds the
  // DOM, so an element captured at push time is detached by the time a pop
  // wants to focus it.
  type Source = CtxMenuEntry[] | (() => CtxMenuEntry[]);
  interface Level extends FilterState {
    source: Source; parent: CtxMenuSubmenu | null;
    /** Position of the submenu row that opened this level among its parent's entries. */
    parentIndex: number;
    /** `combobox` when the level has a text field, else the keyboard roves DOM focus. */
    mode: 'roving' | 'combobox';
  }
  const levels: Level[] = [{ source: opts.entries, parent: null, parentIndex: -1, query: '', mode: 'roving' }];
  const top = (): Level => levels[levels.length - 1];
  const menuUid = `ctx-menu-${++menuSeq}`;
  let rowSeq = 0;
  /** Where addRow appends: the menu, or the sticky header / list wrapper while one is built. */
  let host: HTMLElement = menu;
  const resolve = (src: Source): CtxMenuEntry[] => (typeof src === 'function' ? src() : src);
  const detailOf = (d: string | (() => string) | undefined): string => (typeof d === 'function' ? d() : d ?? '');

  /** Enabled menuitem elements, in visual order — the roving-focus ring. */
  let focusables: HTMLElement[] = [];
  /** Each row's own activation policy: an item closes then acts, a submenu
   *  pushes, the back row pops. `activate` invokes whatever is registered. */
  let actions = new Map<HTMLElement, (ev: MouseEvent) => void>();
  /** Entry position → its freshly rendered row, so a pop can refocus the parent. A
   *  position, not the label or the entry object: two rows can share a label, and a
   *  live level builds new entry objects every time it paints. */
  let rows = new Map<number, HTMLElement>();
  /** ArrowRight / chevron target: only a submenu row has one. */
  let drills = new Map<HTMLElement, () => void>();
  let tookFocus = false;

  const activate = (item: HTMLElement, ev: MouseEvent): void => {
    actions.get(item)?.(ev);
  };

  /** A menuitem row. `run: null` renders it inert: no action, outside the
   *  roving ring. `id`, when given, is the row's `data-action` handle. */
  const addRow = (
    label: string,
    run: ((ev: MouseEvent) => void) | null,
    id?: string,
    role: 'menuitem' | 'menuitemcheckbox' = 'menuitem',
  ): HTMLElement => {
    const item = document.createElement('div');
    item.className = 'ctx-item';
    item.setAttribute('role', role);
    item.id = `${menuUid}-r${++rowSeq}`;
    if (id) item.dataset.action = id;
    const span = document.createElement('span');
    span.className = 'ctx-label';
    span.textContent = label;
    item.appendChild(span);
    if (run === null) {
      item.classList.add('disabled');
      item.setAttribute('aria-disabled', 'true');
    } else {
      item.tabIndex = -1;
      actions.set(item, run);
      focusables.push(item);
      item.addEventListener('click', (ev) => activate(item, ev));
    }
    host.appendChild(item);
    return item;
  };

  const addDetail = (item: HTMLElement, text: string): void => {
    if (!text) return;
    const d = document.createElement('span');
    d.className = 'ctx-detail';
    d.textContent = text;
    item.appendChild(d);
  };

  const addSeparator = (): void => {
    const sep = document.createElement('div');
    sep.className = 'ctx-sep';
    sep.setAttribute('role', 'separator');
    host.appendChild(sep);
  };

  const renderLevel = (): void => {
    const level = top();
    menu.replaceChildren();
    host = menu;
    focusables = [];
    actions = new Map();
    rows = new Map();
    drills = new Map();
    // The accessible name follows the level, so the menu says where the user is.
    const name = level.parent?.label ?? opts.ariaLabel;
    if (name) menu.setAttribute('aria-label', name);
    else menu.removeAttribute('aria-label');

    // Back row and filter share one header block (sticky in a list menu).
    let list: HTMLElement = menu;
    if (level.parent) {
      const head = document.createElement('div');
      head.className = 'ctx-head';
      menu.appendChild(head);
      host = head;
      const back = addRow(level.parent.label, () => pop(), 'back');
      back.classList.add('ctx-back');
      back.setAttribute('aria-label', t('ctx.back_to', { label: level.parent.label }));
      addDetail(back, detailOf(level.parent.detail));
      const filter = level.parent.filter;
      if (level.mode === 'combobox') {
        list = document.createElement('div');
        list.className = 'ctx-list';
        list.id = `${menuUid}-list`;
        list.setAttribute('role', 'group');
        head.appendChild(buildFilterField(level, filter!, list.id, setQuery));
        menu.appendChild(list);
      } else if (filter && level.query) {
        head.appendChild(buildQueryLine(level.query));
      } else {
        addSeparator();
      }
    }
    host = list;

    const entries = resolve(level.source);
    const matches = (e: CtxMenuCheckbox): boolean => filterMatch(e, level.query);
    if (level.query !== '' && !entries.some((e) => e.kind === 'checkbox' && matches(e))) {
      host.appendChild(buildEmptyHint(level.parent?.filter?.empty ?? ''));
    }

    for (const [index, entry] of entries.entries()) {
      if (entry.kind === 'separator') { addSeparator(); continue; }
      if (entry.kind === 'note') {
        const hintEl = document.createElement('div');
        hintEl.className = 'ctx-hint';
        hintEl.setAttribute('role', 'presentation');
        entry.render(hintEl);
        host.appendChild(hintEl);
        continue;
      }
      if (entry.kind === 'checkbox') {
        if (!matches(entry)) continue;
        const row = addRow(entry.label, () => {
          entry.checked = !entry.checked;
          entry.onToggle(entry.checked);
          repaint();
        }, entry.id, 'menuitemcheckbox');
        row.classList.add('ctx-check');
        row.setAttribute('aria-checked', String(entry.checked));
        addDetail(row, entry.detail ?? '');
        continue;
      }
      if (entry.kind === 'submenu') {
        const drill = (): void => push(entry, index);
        const row = addRow(entry.label, entry.action ? (ev) => {
          pick(entry.label);
          entry.action!(ev);
        } : drill, entry.id);
        row.setAttribute('aria-haspopup', 'menu');
        drills.set(row, drill);
        rows.set(index, row);
        if (entry.current) {
          row.setAttribute('aria-current', 'true');
          row.classList.add('ctx-current');
        }
        addDetail(row, detailOf(entry.detail));
        if (entry.action) {
          // Two targets: the chevron is its own button-sized zone.
          const chev = document.createElement('span');
          chev.className = 'ctx-chev';
          chev.setAttribute('aria-hidden', 'true');
          chev.addEventListener('click', (ev) => { ev.stopPropagation(); drill(); });
          row.classList.add('ctx-dual');
          row.appendChild(chev);
        }
        continue;
      }
      // Close BEFORE running the action so a follow-up surface the action
      // opens (input popover, dialog) isn't torn down by our own cleanup.
      const item = addRow(entry.label, entry.disabled ? null : (ev) => {
        pick(entry.label, entry.keepOpen);
        entry.action(ev);
        if (entry.keepOpen) repaint();
      }, entry.id);
      if (entry.danger) item.classList.add('danger');
      if (entry.current) {
        item.setAttribute('aria-current', 'true');
        item.classList.add('ctx-current');
      }
      addDetail(item, entry.detail ?? '');
      if (entry.kbd) {
        const chip = document.createElement('span');
        chip.className = 'ctx-kbd';
        chip.setAttribute('aria-hidden', 'true');
        chip.textContent = entry.kbd;
        item.appendChild(chip);
      }
    }
  };

  /** The keyboard's cursor for the level on screen. */
  const roving: Cursor = {
    current: () => document.activeElement as HTMLElement | null,
    entry: () => focusables[0],
    move: (el) => { tookFocus = true; el.focus(); },
    ownsNavKeys: true,
    followsPointer: true,
    enter: () => { if (tookFocus) focusItem(focusables.length > 1 ? 1 : 0); },
    land(first) { if (first) { tookFocus = true; first.focus({ preventScroll: true }); } },
    clear: () => false,
    hold() {
      const had = menu.contains(document.activeElement);
      return (next) => { if (had) next?.focus({ preventScroll: true }); };
    },
  };
  const comboboxes = new WeakMap<Level, Cursor>();
  const cursor = (): Cursor => {
    const level = top();
    if (level.mode === 'roving') return roving;
    let c = comboboxes.get(level);
    if (!c) {
      c = createComboboxCursor({ state: level, menu, rows: () => focusables, engage: () => { tookFocus = true; }, setQuery });
      comboboxes.set(level, c);
    }
    return c;
  };

  /** Narrow the level to `q` and put the keyboard on the best match, whether the text
   *  came from the level's field or from typed keys. */
  function setQuery(q: string): void {
    top().query = q;
    renderLevel();
    cursor().land(firstMatch(focusables));
    menu.scrollTop = 0;
  }

  /** Re-render the current level in place after a toggle, a keepOpen action or a
   *  filter keystroke: same scroll offset, and the keyboard's row stays put. */
  const repaint = (): void => {
    const current = cursor().current();
    const focusedId = current?.dataset.action;
    const focusedIdx = current ? focusables.indexOf(current) : -1;
    const restore = cursor().hold();
    const top0 = menu.scrollTop;
    renderLevel();
    menu.scrollTop = top0;
    const same = focusedId ? focusables.find((f) => f.dataset.action === focusedId) : undefined;
    restore(same ?? focusables[Math.min(Math.max(focusedIdx, 0), focusables.length - 1)]);
  };

  const focusItem = (idx: number): void => {
    if (focusables.length === 0) return;
    cursor().move(focusables[((idx % focusables.length) + focusables.length) % focusables.length]);
  };

  const push = (entry: CtxMenuSubmenu, parentIndex: number): void => {
    levels.push({ source: entry.entries, parent: entry, parentIndex, query: '', mode: entry.filter?.field ? 'combobox' : 'roving' });
    renderLevel();
    position(menu, opts.position);
    // WAI-ARIA: opening a submenu places focus on its first item — the first
    // entry row, past the back row (which ArrowUp still reaches by wrapping).
    cursor().enter();
  };

  const pop = (): void => {
    if (levels.length < 2) return;
    const { parentIndex } = levels.pop()!;
    renderLevel();
    position(menu, opts.position);
    // WAI-ARIA: closing a submenu returns focus to the parent menuitem — its
    // fresh row, and only when the keyboard had engaged.
    if (tookFocus) rows.get(parentIndex)?.focus();
  };

  /** A row was chosen: log it and, unless the menu stays open, close before the action runs. */
  const pick = (label: string, keepOpen = false): void => {
    if (opts.logSurface) uiEvent('menu.pick', { surface: opts.logSurface, target: opts.logTarget, item: label });
    if (!keepOpen) handle.close();
  };

  if (opts.logSurface) {
    const items = resolve(opts.entries).filter((e): e is CtxMenuEntry & { label: string } => 'label' in e).map((e) => e.label);
    uiEvent('menu.open', { surface: opts.logSurface, target: opts.logTarget, items: items.join('|') });
  }

  // Focus bookkeeping: restore the invoker only if the menu took focus.
  // For an anchored menu (`below`), the anchor IS the invoker — restoring to
  // it is correct even when opening didn't focus it in the first place.
  // WebKit doesn't focus a <button> on mouse click (unlike Chromium/Firefox),
  // so capturing `document.activeElement` here got `body` for a caret click
  // and Escape had nowhere real to send focus back to (create-menu gate,
  // webkit project). A `point` menu (right-click, no single owning element)
  // still falls back to whatever was focused when it opened.
  const invoker = opts.position.kind === 'below'
    ? opts.position.anchor
    : document.activeElement instanceof HTMLElement ? document.activeElement : null;
  renderLevel();
  document.body.appendChild(menu);
  position(menu, opts.position);
  if (opts.focusFirst) focusItem(0);

  const onKeydown = (ev: KeyboardEvent): void => {
    if (ev.key === 'Escape') {
      ev.preventDefault();
      ev.stopPropagation();
      // Escape clears a non-empty filter first, then is "back one level" when
      // nested and "close" at the root.
      if (cursor().clear()) return;
      if (levels.length > 1) pop();
      else handle.close();
      return;
    }
    if (ev.key === 'Tab') {
      // A menu has no tab stops (WAI-ARIA menu pattern) — close and let the
      // default focus move proceed underneath.
      handle.close();
      return;
    }
    const cur = cursor();
    const focusedIdx = focusables.indexOf(cur.current() as HTMLElement);
    // Type-to-filter: a filterable level claims printable keys even before the
    // keyboard has engaged, so a letter typed after a mouse drill-in narrows the
    // list instead of landing in the surface underneath. A level with a field
    // gets its text from the field itself.
    if (top().parent?.filter && top().mode === 'roving') {
      const q = nextQuery(ev, top().query);
      if (q !== null) {
        ev.preventDefault();
        ev.stopPropagation();
        setQuery(q);
        return;
      }
    }
    switch (ev.key) {
      case 'ArrowDown':
        ev.preventDefault();
        ev.stopPropagation();
        focusItem(focusedIdx === -1 ? Math.max(0, focusables.indexOf(cur.entry()!)) : focusedIdx + 1);
        return;
      case 'ArrowUp':
        ev.preventDefault();
        ev.stopPropagation();
        focusItem(focusedIdx === -1 ? focusables.length - 1 : focusedIdx - 1);
        return;
      case 'Home':
        if (focusedIdx === -1 || !cur.ownsNavKeys) return; // keyboard not engaged — leave Home to the surface below
        ev.preventDefault();
        ev.stopPropagation();
        focusItem(0);
        return;
      case 'End':
        if (focusedIdx === -1 || !cur.ownsNavKeys) return;
        ev.preventDefault();
        ev.stopPropagation();
        focusItem(focusables.length - 1);
        return;
      case 'ArrowLeft':
        if (levels.length < 2 || !cur.ownsNavKeys) return; // in the field, Left moves the caret
        ev.preventDefault();
        ev.stopPropagation();
        pop();
        return;
      case 'ArrowRight': {
        if (focusedIdx === -1 || !cur.ownsNavKeys) return;
        const drill = drills.get(focusables[focusedIdx]);
        if (!drill) return;
        ev.preventDefault();
        ev.stopPropagation();
        drill();
        return;
      }
      case 'Enter':
      case ' ': {
        if (focusedIdx === -1) return;
        ev.preventDefault();
        ev.stopPropagation();
        const item = focusables[focusedIdx];
        const r = item.getBoundingClientRect();
        // Synthesize the activating MouseEvent so point-positioned follow-up
        // surfaces open at the item rather than at (0,0).
        activate(item, new MouseEvent('click', { clientX: r.left, clientY: r.bottom }));
        return;
      }
    }
  };
  // Capture phase: the menu wins over surface-level handlers (e.g. the tree's
  // own Escape-clears-selection) while it is open.
  document.addEventListener('keydown', onKeydown, true);

  // Outside-click plus every gesture that never reaches this document — see
  // dismiss-outside.ts for the drag-region, separate-webview and preview-iframe
  // reasons. All of them belong to the one helper.
  let focusLeftDocument = false;
  const disarmOutside = armOutsideDismiss({
    inside: [menu, opts.outsideAnchor],
    // The menu only — NOT the anchor, whose own hint is the reported bug.
    covers: [menu],
    onDismiss: (reason) => {
      focusLeftDocument = reason !== 'outside-click';
      handle.close();
    },
  });

  let closed = false;
  const handle: CtxMenuHandle = {
    el: menu,
    refresh() {
      if (!closed) repaint();
    },
    close() {
      if (closed) return;
      closed = true;
      if (activeMenu === handle) activeMenu = null;
      disarmOutside();
      document.removeEventListener('keydown', onKeydown, true);
      // Never pull focus back when it already left this document — the user is
      // now in another webview or in the preview iframe, and focusing the
      // invoker would yank them back out of it.
      const focusWasInside = tookFocus && !focusLeftDocument && menu.contains(document.activeElement);
      menu.remove();
      if (focusWasInside && invoker?.isConnected) invoker.focus();
      opts.onClose?.();
    },
  };
  activeMenu = handle;
  return handle;
}

function position(menu: HTMLElement, pos: OpenCtxMenuOptions['position']): void {
  menu.style.position = 'fixed';
  if (pos.kind === 'point') {
    menu.style.left = `${pos.x}px`;
    menu.style.top = `${pos.y}px`;
  } else {
    const r = pos.anchor.getBoundingClientRect();
    menu.style.top = `${r.bottom + MENU_ANCHOR_GAP}px`;
    if (pos.align === 'right') {
      menu.style.right = `${Math.max(4, window.innerWidth - r.right)}px`;
      menu.style.left = 'auto';
      // Width grows leftward from the pin; cap it so the left edge keeps the
      // viewport reserve (a long template name ellipsizes — .ctx-label).
      menu.style.maxWidth = `${r.right - VIEWPORT_EDGE_PADDING}px`;
      // Horizontal clamp not applicable (already pinned off the right edge);
      // vertical clamp only — setting `left` on a right-anchored element
      // would stretch it.
      requestAnimationFrame(() => {
        const b = menu.getBoundingClientRect();
        if (b.bottom > window.innerHeight - VIEWPORT_EDGE_PADDING) {
          menu.style.top = `${window.innerHeight - b.height - VIEWPORT_EDGE_PADDING}px`;
        }
      });
      return;
    }
    menu.style.left = `${r.left}px`;
  }
  requestAnimationFrame(() => clampIntoViewport(menu));
}
