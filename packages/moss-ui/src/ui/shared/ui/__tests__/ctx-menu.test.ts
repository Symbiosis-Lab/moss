/**
 * Unit tests for the shared context-menu primitive (shared/ui/ctx-menu.ts).
 *
 * The primitive is the one implementation of menu behaviour for every moss
 * context menu (file tree, chip bar, create-flow ＋▾) — so these tests carry
 * the behavioural contract, and the surface tests only assert menu CONTENTS:
 * - ARIA: role="menu", role="menuitem", role="separator", aria-disabled
 * - Dismissal: Escape, outside mousedown, webview blur; inside clicks don't
 *   dismiss (drag-region and cross-webview mechanics: dismiss-outside.test.ts)
 * - Roving focus: ArrowUp/Down wrap, Home/End, Enter/Space activate,
 *   disabled items skipped, focus returns to the invoker on close
 * - Activation closes the menu BEFORE running the action
 * - Drill-in levels: submenu push (click/Enter/Space/ArrowRight), the back
 *   row, pop (ArrowLeft/Escape/back row) with focus returned to the parent
 *   row, Escape-at-root closes
 * - Viewport clamping (shared clampIntoViewport)
 * - `.ctx-kbd` chips, danger styling, notes, open-closes-previous singleton
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { openCtxMenu, joinCtxGroups, clampIntoViewport, isCtxMenuKey, type CtxMenuEntry, type CtxMenuHandle } from '../ctx-menu';
import { initTooltips, destroyTooltips } from '../../../components/tooltip';
import { configureToastRuntime, _resetToastRuntime } from '../toast/toast-runtime';

// ctx-menu.ts is `shared/ui` and so does not import the log directly
// (shared/ is pure); the [ui] breadcrumb it logs goes through the same
// injected host seam toasts use (toast-runtime.ts).
const uiEvent = vi.fn();

let handle: CtxMenuHandle | null = null;

function open(entries: CtxMenuEntry[] | (() => CtxMenuEntry[]), extra: Partial<Parameters<typeof openCtxMenu>[0]> = {}): CtxMenuHandle {
  handle = openCtxMenu({
    entries,
    position: { kind: 'point', x: 10, y: 20 },
    ...extra,
  });
  return handle;
}

function items(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('.ctx-item'));
}

function keydown(key: string, init: KeyboardEventInit = {}): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
}

beforeEach(() => {
  vi.useFakeTimers();
  uiEvent.mockClear();
  configureToastRuntime({ logUiEvent: (line: string) => uiEvent(line) });
});

afterEach(() => {
  handle?.close();
  handle = null;
  _resetToastRuntime();
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('openCtxMenu — structure and ARIA', () => {
  it('renders role="menu" with role="menuitem" items and role="separator" rules', () => {
    open([
      { label: 'One', action: vi.fn() },
      { kind: 'separator' },
      { label: 'Two', action: vi.fn() },
    ]);
    const menu = document.querySelector('.ctx-menu')!;
    expect(menu.getAttribute('role')).toBe('menu');
    const menuItems = menu.querySelectorAll('[role="menuitem"]');
    expect(menuItems).toHaveLength(2);
    expect(menu.querySelectorAll('[role="separator"]')).toHaveLength(1);
    expect(menuItems[0].querySelector('.ctx-label')?.textContent).toBe('One');
  });

  it('renders a .ctx-kbd chip (aria-hidden) for items with a shortcut hint', () => {
    open([{ label: 'New Page', kbd: '⌘N', action: vi.fn() }]);
    const chip = document.querySelector('.ctx-item .ctx-kbd')!;
    expect(chip.textContent).toBe('Ctrl+N'); // jsdom reports Linux
    expect(chip.getAttribute('aria-hidden')).toBe('true');
  });

  it('marks danger items with the danger class and disabled items aria-disabled', () => {
    open([
      { label: 'Remove', danger: true, action: vi.fn() },
      { label: 'Inert', disabled: true, action: vi.fn() },
    ]);
    const [remove, inert] = items();
    expect(remove.classList.contains('danger')).toBe(true);
    expect(inert.getAttribute('aria-disabled')).toBe('true');
  });

  it('exposes an entry\'s id as data-action, and omits the attribute when none is given', () => {
    open([
      { label: 'New Page', id: 'new-page', action: vi.fn() },
      { label: 'Untitled item', action: vi.fn() },
    ]);
    const [withId, withoutId] = items();
    expect(withId.dataset.action).toBe('new-page');
    expect(withoutId.hasAttribute('data-action')).toBe(false);
  });

  it('renders a note as a presentational .ctx-hint outside the roving ring', () => {
    open([
      { kind: 'note', render: (host) => { host.textContent = 'From template'; } },
      { label: 'One', action: vi.fn() },
      { kind: 'note', render: (host) => { host.textContent = 'Creates in blog'; } },
    ], { focusFirst: true });
    const hints = document.querySelectorAll('.ctx-menu .ctx-hint');
    expect(Array.from(hints, (h) => h.textContent)).toEqual(['From template', 'Creates in blog']);
    expect(hints[0].getAttribute('role')).toBe('presentation');
    expect((document.activeElement as HTMLElement).textContent).toBe('One');
    keydown('ArrowDown'); // one item in the ring: wraps onto itself, never a note
    expect((document.activeElement as HTMLElement).textContent).toBe('One');
  });

  it('applies an aria-label when given', () => {
    open([{ label: 'One', action: vi.fn() }], { ariaLabel: 'File actions' });
    expect(document.querySelector('.ctx-menu')?.getAttribute('aria-label')).toBe('File actions');
  });
});

describe('openCtxMenu — dismissal', () => {
  it('closes on Escape and fires onClose exactly once', () => {
    const onClose = vi.fn();
    open([{ label: 'One', action: vi.fn() }], { onClose });
    keydown('Escape');
    expect(document.querySelector('.ctx-menu')).toBeNull();
    handle!.close(); // idempotent
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closes on outside mousedown, but not before the deferred registration', () => {
    open([{ label: 'One', action: vi.fn() }]);
    // Same-tick mousedown (the opening interaction) must NOT close it.
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
    vi.advanceTimersByTime(0);
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });

  it('does not close on mousedown inside the menu or on the outsideAnchor', () => {
    const anchor = document.createElement('button');
    document.body.appendChild(anchor);
    open([{ label: 'One', action: vi.fn() }], { outsideAnchor: anchor });
    vi.advanceTimersByTime(0);
    items()[0].dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    anchor.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
  });

  // The blur path itself belongs to dismiss-outside.test.ts; what this one
  // proves is the wiring — the menu really is armed through that helper, and
  // reacts to a signal no listener of its own could have received.
  it('closes on a blur, and does not steal focus back, when the webview lost focus', () => {
    const invoker = document.createElement('button');
    document.body.appendChild(invoker);
    invoker.focus();
    open([{ label: 'One', action: vi.fn() }], { focusFirst: true });
    vi.advanceTimersByTime(0);
    const refocus = vi.spyOn(invoker, 'focus');
    window.dispatchEvent(new Event('blur'));
    expect(document.querySelector('.ctx-menu')).toBeNull();
    expect(refocus).not.toHaveBeenCalled();
  });

  it('opening a second menu closes the first (a context menu is singular)', () => {
    const first = open([{ label: 'First', action: vi.fn() }]);
    openCtxMenu({ entries: [{ label: 'Second', action: vi.fn() }], position: { kind: 'point', x: 0, y: 0 } }).close();
    expect(first.el.isConnected).toBe(false);
  });
});

describe('openCtxMenu — activation', () => {
  it('click activates: the menu closes BEFORE the action runs', () => {
    let menuAtActionTime: Element | null = document.body; // sentinel
    open([{ label: 'One', action: () => { menuAtActionTime = document.querySelector('.ctx-menu'); } }]);
    items()[0].click();
    expect(menuAtActionTime).toBeNull();
  });

  it('a disabled item does nothing on click', () => {
    const action = vi.fn();
    open([{ label: 'Inert', disabled: true, action }]);
    items()[0].click();
    expect(action).not.toHaveBeenCalled();
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
  });
});

describe('openCtxMenu — roving keyboard focus', () => {
  it('focusFirst puts focus on the first enabled item at open', () => {
    open([
      { label: 'Inert', disabled: true, action: vi.fn() },
      { label: 'One', action: vi.fn() },
    ], { focusFirst: true });
    expect(document.activeElement?.querySelector('.ctx-label')?.textContent).toBe('One');
  });

  it('ArrowDown/ArrowUp move focus and wrap; disabled items are skipped', () => {
    open([
      { label: 'A', action: vi.fn() },
      { label: 'Inert', disabled: true, action: vi.fn() },
      { label: 'B', action: vi.fn() },
    ]);
    keydown('ArrowDown'); // enters the ring at A
    expect((document.activeElement as HTMLElement).textContent).toBe('A');
    keydown('ArrowDown'); // skips Inert
    expect((document.activeElement as HTMLElement).textContent).toBe('B');
    keydown('ArrowDown'); // wraps
    expect((document.activeElement as HTMLElement).textContent).toBe('A');
    keydown('ArrowUp'); // wraps backwards
    expect((document.activeElement as HTMLElement).textContent).toBe('B');
  });

  it('Home/End jump to the first/last enabled item once the ring is engaged', () => {
    open([
      { label: 'A', action: vi.fn() },
      { label: 'B', action: vi.fn() },
      { label: 'C', action: vi.fn() },
    ], { focusFirst: true });
    keydown('End');
    expect((document.activeElement as HTMLElement).textContent).toBe('C');
    keydown('Home');
    expect((document.activeElement as HTMLElement).textContent).toBe('A');
  });

  it('Enter and Space activate the focused item', () => {
    const action = vi.fn();
    open([{ label: 'One', action }], { focusFirst: true });
    keydown('Enter');
    expect(action).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.ctx-menu')).toBeNull();

    open([{ label: 'Two', action }], { focusFirst: true });
    keydown(' ');
    expect(action).toHaveBeenCalledTimes(2);
  });

  it('keyboard activation synthesizes a MouseEvent carrying coordinates', () => {
    let received: MouseEvent | null = null;
    open([{ label: 'One', action: (ev) => { received = ev; } }], { focusFirst: true });
    keydown('Enter');
    expect(received).toBeInstanceOf(MouseEvent);
  });

  it('returns focus to the invoker on close when the menu had taken it', () => {
    const invoker = document.createElement('button');
    document.body.appendChild(invoker);
    invoker.focus();
    open([{ label: 'One', action: vi.fn() }], { focusFirst: true });
    expect(document.activeElement).not.toBe(invoker);
    keydown('Escape');
    expect(document.activeElement).toBe(invoker);
  });

  it('a mouse-opened menu never steals focus, and close leaves focus alone', () => {
    const editor = document.createElement('textarea');
    document.body.appendChild(editor);
    editor.focus();
    open([{ label: 'One', action: vi.fn() }]);
    expect(document.activeElement).toBe(editor);
    handle!.close();
    expect(document.activeElement).toBe(editor);
  });
});

describe('openCtxMenu — a row marked current, and a list that refreshes while open', () => {
  it('marks a current item aria-current and leaves the others bare', () => {
    open([
      { label: 'A', current: true, action: vi.fn() },
      { label: 'B', action: vi.fn() },
    ]);
    const [a, b] = items();
    expect(a.getAttribute('aria-current')).toBe('true');
    expect(a.classList.contains('ctx-current')).toBe(true);
    expect(b.hasAttribute('aria-current')).toBe(false);
  });

  it('refresh re-reads a function source and keeps the keyboard on the same row', () => {
    let rows = ['a', 'b'];
    const h = open(() => rows.map((id) => ({ id, label: id.toUpperCase(), action: vi.fn() })));
    keydown('ArrowDown');
    keydown('ArrowDown');
    expect(document.activeElement?.getAttribute('data-action')).toBe('b');
    rows = ['x', 'a', 'b'];
    h.refresh();
    expect(items().map((i) => i.textContent)).toEqual(['X', 'A', 'B']);
    expect(document.activeElement?.getAttribute('data-action')).toBe('b');
  });

  it('refresh after close does nothing', () => {
    const h = open([{ label: 'A', action: vi.fn() }]);
    h.close();
    expect(() => h.refresh()).not.toThrow();
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });
});

describe('openCtxMenu — drill-in levels', () => {
  const sub = (label: string, entries: CtxMenuEntry[]): CtxMenuEntry => ({ kind: 'submenu', label, entries });
  const labelsOf = () => items().map((i) => i.querySelector('.ctx-label')?.textContent);
  const focusedText = () => (document.activeElement as HTMLElement | null)?.querySelector('.ctx-label')?.textContent;
  const two = (leaf = vi.fn()) => [
    { label: 'Root item', action: vi.fn() },
    sub('Manage', [sub('Essay', [{ label: 'Rename', action: leaf }])]),
  ];

  it('a submenu row carries aria-haspopup="menu" and no aria-expanded', () => {
    open(two());
    const row = items()[1];
    expect(row.getAttribute('role')).toBe('menuitem');
    expect(row.getAttribute('aria-haspopup')).toBe('menu');
    expect(row.hasAttribute('aria-expanded')).toBe(false);
  });

  it('a submenu row exposes its own id as data-action, same as a plain item', () => {
    open([
      { label: 'Root item', action: vi.fn() },
      { kind: 'submenu', label: 'Manage', id: 'manage-templates', entries: [{ label: 'Rename', action: vi.fn() }] },
    ]);
    expect(items()[1].dataset.action).toBe('manage-templates');
  });

  it('click replaces the rows in place under a back row named after the parent, in the SAME element', () => {
    const h = open(two(), { ariaLabel: 'Create' });
    items()[1].click();
    expect(document.querySelectorAll('.ctx-menu')).toHaveLength(1);
    expect(h.el.isConnected).toBe(true);
    expect(labelsOf()).toEqual(['Manage', 'Essay']);
    const back = items()[0];
    expect(back.classList.contains('ctx-back')).toBe(true);
    expect(back.getAttribute('role')).toBe('menuitem');
    expect(back.getAttribute('aria-label')).toBe('Back to Manage');
    expect(h.el.getAttribute('aria-label')).toBe('Manage');
    expect(h.el.querySelectorAll('[role="separator"]')).toHaveLength(1);
  });

  it.each(['Enter', ' ', 'ArrowRight'])('%j on a focused submenu row pushes and focuses the first child entry', (key) => {
    open(two(), { focusFirst: true });
    keydown('ArrowDown');
    expect(focusedText()).toBe('Manage');
    keydown(key);
    expect(labelsOf()).toEqual(['Manage', 'Essay']);
    expect(focusedText()).toBe('Essay');
    keydown('ArrowUp'); // the back row is in the ring, first
    expect(focusedText()).toBe('Manage');
  });

  it('ArrowRight on a plain item does nothing', () => {
    open(two(), { focusFirst: true });
    keydown('ArrowRight');
    expect(labelsOf()).toEqual(['Root item', 'Manage']);
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
  });

  it.each(['ArrowLeft', 'Escape'])('%s pops one level and refocuses the parent row', (key) => {
    open(two(), { focusFirst: true });
    keydown('ArrowDown');
    keydown('Enter'); // → Manage level
    keydown('Enter'); // → Essay level
    expect(labelsOf()).toEqual(['Essay', 'Rename']);
    keydown(key);
    expect(labelsOf()).toEqual(['Manage', 'Essay']);
    expect(focusedText()).toBe('Essay');
    keydown(key);
    expect(labelsOf()).toEqual(['Root item', 'Manage']);
    expect(focusedText()).toBe('Manage');
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
  });

  it('the back row pops; ArrowLeft at the root does nothing; Escape at the root closes', () => {
    open(two());
    keydown('ArrowLeft');
    expect(labelsOf()).toEqual(['Root item', 'Manage']);
    items()[1].click();
    items()[0].click(); // back row
    expect(labelsOf()).toEqual(['Root item', 'Manage']);
    keydown('Escape');
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });

  it('Back returns focus to the row that opened the level, even when two rows share a label', () => {
    const level = (): CtxMenuEntry[] => [
      { kind: 'submenu', label: 'Same', entries: [{ label: 'a', action: () => {} }] },
      { kind: 'submenu', label: 'Same', entries: [{ label: 'b', action: () => {} }] },
    ];
    open(level(), { focusFirst: true });
    keydown('ArrowDown');
    keydown('ArrowRight');
    expect(items().map((i) => i.textContent)).toEqual(['Same', 'b']);
    keydown('ArrowLeft');
    expect(document.activeElement).toBe(items()[1]);
  });

  it('a mouse-driven pop leaves focus alone', () => {
    const editor = document.createElement('textarea');
    document.body.appendChild(editor);
    editor.focus();
    open(two());
    items()[1].click();
    items()[0].click();
    expect(document.activeElement).toBe(editor);
  });

  it('an item at depth closes the menu before its action runs', () => {
    let menuAtActionTime: Element | null = document.body;
    open(two(vi.fn(() => { menuAtActionTime = document.querySelector('.ctx-menu'); })));
    items()[1].click();
    items()[1].click();
    items()[1].click(); // Rename
    expect(menuAtActionTime).toBeNull();
  });

  it('outside-click at depth two closes and fires onClose once', () => {
    const onClose = vi.fn();
    open(two(), { onClose });
    vi.advanceTimersByTime(0);
    items()[1].click();
    items()[1].click();
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(document.querySelector('.ctx-menu')).toBeNull();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('close() after a pop restores focus to the invoker exactly once', () => {
    const invoker = document.createElement('button');
    document.body.appendChild(invoker);
    invoker.focus();
    const refocus = vi.spyOn(invoker, 'focus');
    open(two(), { focusFirst: true });
    keydown('ArrowDown');
    keydown('Enter');
    keydown('ArrowLeft');
    expect(refocus).not.toHaveBeenCalled();
    keydown('Escape');
    expect(document.activeElement).toBe(invoker);
    expect(refocus).toHaveBeenCalledTimes(1);
  });
});

describe('clampIntoViewport', () => {
  it('pulls an overflowing element back inside the 8px viewport padding', () => {
    const el = document.createElement('div');
    el.style.left = '990px';
    el.style.top = '750px';
    document.body.appendChild(el);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
      left: 990, top: 750, right: 1150, bottom: 950, width: 160, height: 200,
      x: 990, y: 750, toJSON: () => ({}),
    } as DOMRect);
    vi.spyOn(window, 'innerWidth', 'get').mockReturnValue(1000);
    vi.spyOn(window, 'innerHeight', 'get').mockReturnValue(800);
    clampIntoViewport(el);
    expect(el.style.left).toBe(`${1000 - 160 - 8}px`);
    expect(el.style.top).toBe(`${800 - 200 - 8}px`);
  });

  it('leaves an element that already fits untouched', () => {
    const el = document.createElement('div');
    el.style.left = '10px';
    el.style.top = '20px';
    document.body.appendChild(el);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
      left: 10, top: 20, right: 170, bottom: 220, width: 160, height: 200,
      x: 10, y: 20, toJSON: () => ({}),
    } as DOMRect);
    clampIntoViewport(el);
    expect(el.style.left).toBe('10px');
    expect(el.style.top).toBe('20px');
  });
});

describe('joinCtxGroups', () => {
  it('separates only non-empty neighbouring groups (no leading/trailing/double separators)', () => {
    const item = (label: string): CtxMenuEntry => ({ label, action: () => {} });
    const joined = joinCtxGroups([[], [item('A')], [], [item('B'), item('C')], []]);
    expect(joined.map(e => ('label' in e ? e.label : '—'))).toEqual(['A', '—', 'B', 'C']);
  });
});

describe('isCtxMenuKey', () => {
  it('matches Shift+F10 and the ContextMenu key, nothing else', () => {
    expect(isCtxMenuKey(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true }))).toBe(true);
    expect(isCtxMenuKey(new KeyboardEvent('keydown', { key: 'ContextMenu' }))).toBe(true);
    expect(isCtxMenuKey(new KeyboardEvent('keydown', { key: 'F10' }))).toBe(false);
    expect(isCtxMenuKey(new KeyboardEvent('keydown', { key: 'Escape' }))).toBe(false);
  });
});

/**
 * A menu sits at `--z-toast` + 50 and a tooltip at `--z-popup`, so a tooltip
 * still up when a menu opens is buried under it — the reported ＋▾ bug, where
 * the create menu covered the ＋ button's hint. The primitive holds tooltips
 * off for the menu's whole life rather than only at the press, because
 * re-crossing the anchor with the menu open re-fires `mouseover`.
 */
describe('openCtxMenu — tooltips yield to the menu', () => {
  function hoverTrigger(): HTMLElement {
    const trigger = document.createElement('button');
    trigger.setAttribute('data-tooltip', 'New page in William Blake');
    document.body.appendChild(trigger);
    trigger.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    return trigger;
  }

  const tooltipShown = (): boolean =>
    document.querySelector('.moss-tooltip')?.classList.contains('visible') ?? false;

  beforeEach(() => initTooltips());
  afterEach(() => destroyTooltips());

  it('hides a tooltip that is up when the menu opens', () => {
    hoverTrigger();
    expect(tooltipShown()).toBe(true);

    open([{ label: 'New page', action: () => {} }]);

    expect(tooltipShown()).toBe(false);
  });

  it("holds off the ANCHOR's own hint — the reported ＋▾ case", () => {
    // The anchor is in `inside` so a press on it does not dismiss the menu it
    // just opened. Scoping the tooltip hold to `inside` therefore spared the
    // one hint that is certainly covered, and a fixture that opens a menu
    // with no anchor cannot see that: this test exists to carry the anchor.
    const anchor = hoverTrigger();
    expect(tooltipShown()).toBe(true);

    open([{ label: 'New page', action: () => {} }], { outsideAnchor: anchor });
    expect(tooltipShown()).toBe(false);

    // ...and re-crossing it with the menu open must not resurrect it.
    anchor.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    expect(tooltipShown()).toBe(false);
  });
});

describe('Send Logs UI trail (logSurface/logTarget)', () => {
  it('logs menu.open with the surface, target and visible item labels when logSurface is given', () => {
    open(
      [
        { label: 'Rename', action: () => {} },
        { label: 'Versions…', action: () => {} },
      ],
      { logSurface: 'tree-row', logTarget: 'file:foo.md' },
    );
    expect(uiEvent).toHaveBeenCalledWith(
      '[ui] menu.open surface=tree-row target=file:foo.md items=Rename|Versions…',
    );
  });

  it('logs menu.pick with the clicked item label, not every item shown', () => {
    open(
      [
        { label: 'Rename', action: () => {} },
        { label: 'Versions…', action: () => {} },
      ],
      { logSurface: 'tree-row', logTarget: 'file:foo.md' },
    );
    items()[1].click();
    expect(uiEvent).toHaveBeenCalledWith(
      '[ui] menu.pick surface=tree-row target=file:foo.md item=Versions…',
    );
  });

  it('logs nothing when the caller omits logSurface', () => {
    open([{ label: 'Rename', action: () => {} }]);
    items()[0].click();
    expect(uiEvent).not.toHaveBeenCalled();
  });
});

describe('openCtxMenu — checklist levels', () => {
  const labelOf = (el: Element) => el.querySelector('.ctx-label')?.textContent;
  const labelsOf = () => items().map(labelOf);
  const focusedText = () => (document.activeElement as HTMLElement | null) && labelOf(document.activeElement as HTMLElement);
  const names = ['ada', 'alan', 'grace', 'linus'];

  /** An audience row that chooses on Enter and drills on the chevron, over a live checklist. */
  function audienceMenu(extra: Partial<CtxMenuEntry & { kind: 'submenu' }> = {}) {
    const ticked = new Set(names);
    const chosen = vi.fn();
    const level = (): CtxMenuEntry[] => [
      ...(ticked.size < names.length
        ? [{ label: 'Select all', keepOpen: true, action: () => names.forEach((n) => ticked.add(n)) } as CtxMenuEntry]
        : []),
      ...names.map((n): CtxMenuEntry => ({
        kind: 'checkbox', label: `${n}@example.com`, checked: ticked.has(n), id: n,
        onToggle: (on) => (on ? ticked.add(n) : ticked.delete(n)),
      })),
    ];
    const root = (): CtxMenuEntry[] => [{
      kind: 'submenu', label: 'English', id: 'en', action: chosen,
      detail: () => (ticked.size === names.length ? '4' : `${ticked.size} of 4`),
      entries: level,
      filter: { hint: 'Type to filter', empty: 'No matches' },
      ...extra,
    }];
    return { ticked, chosen, root };
  }
  const rows = () => items().filter((i) => i.getAttribute('role') === 'menuitemcheckbox');

  it('renders checkbox rows as menuitemcheckbox with aria-checked, and a click flips one without closing the menu', () => {
    const { ticked, root } = audienceMenu();
    open(root());
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    expect(rows().map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'true', 'true', 'true']);
    rows()[1].click();
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
    expect(ticked.has('alan')).toBe(false);
    expect(rows()[1].getAttribute('aria-checked')).toBe('false');
  });

  it('Space toggles the focused checkbox, keeps focus on it, and Select all appears only once someone is unticked', () => {
    const { ticked, root } = audienceMenu();
    open(root(), { focusFirst: true });
    keydown('ArrowRight'); // drill in from the audience row
    expect(labelsOf()[0]).toBe('English');
    expect(labelsOf()).not.toContain('Select all');
    expect(focusedText()).toBe('ada@example.com'); // a keyboard drill-in lands on the first row
    keydown('ArrowDown');
    expect(focusedText()).toBe('alan@example.com');
    keydown(' ');
    expect(ticked.has('alan')).toBe(false);
    expect(focusedText()).toBe('alan@example.com');
    expect(labelsOf()).toContain('Select all');
    expect(items()[0].querySelector('.ctx-detail')?.textContent).toBe('3 of 4');
    keydown('Enter'); // Enter on a checkbox toggles too
    expect(ticked.has('alan')).toBe(true);
    expect(labelsOf()).not.toContain('Select all');
  });

  it('Select all (keepOpen) restores everyone and leaves the menu open', () => {
    const { ticked, root } = audienceMenu();
    open(root());
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    rows()[0].click();
    rows()[2].click();
    const all = items().find((i) => labelOf(i) === 'Select all')!;
    all.click();
    expect(ticked.size).toBe(4);
    expect(document.querySelector('.ctx-menu')).not.toBeNull();
  });

  it('on a two-target row Enter chooses and closes, ArrowRight and the chevron drill in', () => {
    const a = audienceMenu();
    open(a.root(), { focusFirst: true });
    keydown('Enter');
    expect(a.chosen).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.ctx-menu')).toBeNull();

    const b = audienceMenu();
    open(b.root(), { focusFirst: true });
    keydown('ArrowRight');
    expect(labelsOf()[0]).toBe('English');
    expect(b.chosen).not.toHaveBeenCalled();
    keydown('ArrowLeft');
    expect(labelsOf()).toEqual(['English']);
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    expect(items()[0].classList.contains('ctx-back')).toBe(true);
    expect(b.chosen).not.toHaveBeenCalled();
  });

  it('Left and Escape go back one level, Escape at the first level closes; Back re-reads the row detail', () => {
    const { ticked, root } = audienceMenu();
    open(root(), { focusFirst: true });
    keydown('ArrowRight');
    ticked.delete('ada');
    keydown('Escape');
    expect(items()[0].querySelector('.ctx-detail')?.textContent).toBe('3 of 4');
    expect(document.activeElement).toBe(items()[0]);
    keydown('Escape');
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });

  it('typing narrows the checkbox rows without a text field, Backspace widens, no match says so', () => {
    const { root } = audienceMenu();
    open(root());
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    expect(document.querySelector('.ctx-menu input')).toBeNull();
    expect(document.querySelector('.ctx-filter')).toBeNull(); // no field, so no fake one either
    keydown('l');
    keydown('i');
    expect(rows().map(labelOf)).toEqual(['linus@example.com']);
    expect(document.querySelector('.ctx-filter')?.textContent).toBe('li');
    expect(focusedText()).toBe('linus@example.com'); // Space would toggle the best match
    keydown('Backspace');
    expect(rows()).toHaveLength(4); // "l" is in every "example"
    keydown('Backspace');
    expect(document.querySelector('.ctx-filter')).toBeNull();
    keydown('z');
    keydown('z');
    expect(rows()).toHaveLength(0);
    expect(document.querySelector('.ctx-hint')?.textContent).toBe('No matches');
  });

  it('does not filter on a level that has no filter, and ignores modified keys', () => {
    const { root } = audienceMenu({ filter: undefined });
    open(root());
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    keydown('a');
    expect(rows()).toHaveLength(4);
    const { root: root2 } = audienceMenu();
    handle?.close();
    open(root2());
    items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
    keydown('a', { ctrlKey: true });
    expect(rows()).toHaveLength(4);
  });

  describe('with a filter field', () => {
    const fieldMenu = () => audienceMenu({ filter: { hint: 'Type to filter', empty: 'No matches', field: true } });
    const input = () => document.querySelector<HTMLInputElement>('.ctx-menu input')!;
    const type = (v: string) => { input().value = v; input().dispatchEvent(new Event('input', { bubbles: true })); };
    const activeText = () => labelOf(document.getElementById(input().getAttribute('aria-activedescendant') ?? '')!);

    it('drilling in focuses a combobox input that controls the list', () => {
      open(fieldMenu().root());
      items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
      expect(document.activeElement).toBe(input());
      expect(input().getAttribute('role')).toBe('combobox');
      expect(document.getElementById(input().getAttribute('aria-controls')!)!.contains(rows()[0])).toBe(true);
      expect(input().placeholder).toBe('Type to filter');
    });

    it('ArrowDown moves the active row while focus stays in the field; Space and Enter toggle it', () => {
      const { ticked, root } = fieldMenu();
      open(root());
      items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
      keydown('ArrowDown');
      expect(activeText()).toBe('ada@example.com');
      keydown('ArrowDown');
      expect(activeText()).toBe('alan@example.com');
      expect(document.activeElement).toBe(input());
      keydown(' ');
      expect(ticked.has('alan')).toBe(false);
      expect(document.activeElement).toBe(input());
      expect(activeText()).toBe('alan@example.com');
      keydown('Enter');
      expect(ticked.has('alan')).toBe(true);
    });

    it('typing in the field filters and activates the best match; Escape clears it, then goes back', () => {
      open(fieldMenu().root());
      items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
      type('li');
      expect(rows().map(labelOf)).toEqual(['linus@example.com']);
      expect(document.activeElement).toBe(input());
      expect(activeText()).toBe('linus@example.com');
      keydown('Escape');
      expect(rows()).toHaveLength(4);
      expect(input().value).toBe('');
      keydown('Escape');
      expect(document.querySelector('.ctx-menu input')).toBeNull();
      expect(labelsOf()).toEqual(['English']);
    });

    it('Left and Right stay with the caret instead of changing level', () => {
      open(fieldMenu().root());
      items()[0].querySelector<HTMLElement>('.ctx-chev')!.click();
      keydown('ArrowLeft');
      expect(document.querySelector('.ctx-menu input')).not.toBeNull();
    });
  });
});
