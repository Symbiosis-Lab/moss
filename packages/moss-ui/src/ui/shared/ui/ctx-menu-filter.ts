/**
 * The filter half of the shared context menu (ctx-menu.ts): one matching rule for
 * every filterable level, the text field a long level gets, and the combobox
 * keyboard cursor that goes with it. A level with no field filters from typed keys
 * through the same `filterMatch` and `setQuery` path; only where the keystrokes
 * come from differs.
 */

import { createPillInput } from './pill-select';

export interface FilterSpec { hint: string; empty: string; field?: boolean }

/** The per-level state the filter owns. */
export interface FilterState {
  query: string;
  /** The level's filter field, built once so a repaint keeps it (and its caret). */
  input?: HTMLInputElement;
  /** Combobox level: the row the arrow keys have reached (DOM focus stays on the input). */
  active?: HTMLElement | null;
}

/**
 * Where the keyboard is in a level. A level with no text field moves real DOM focus
 * between rows (`roving`); a level with one keeps focus in the field and names an
 * active row through `aria-activedescendant` (`combobox`). The menu's key handler
 * asks the cursor instead of testing which kind of level it is on.
 */
export interface Cursor {
  /** The row the keyboard is on, or null when it has not engaged. */
  current(): HTMLElement | null;
  /** The row an ArrowDown from an unengaged keyboard lands on. */
  entry(): HTMLElement | undefined;
  move(el: HTMLElement): void;
  /** Home, End, Left and Right belong to the menu (false: they move the field's caret). */
  ownsNavKeys: boolean;
  /** Hover may move the keyboard's row (false: the field keeps focus). */
  followsPointer: boolean;
  /** Where the keyboard goes when the level is first shown. */
  enter(): void;
  /** Where it goes once the list was re-filtered; `first` is the best match. */
  land(first: HTMLElement | undefined): void;
  /** Escape: clear any filter text; true when there was some. */
  clear(): boolean;
  /** Snapshot before a repaint; the returned function restores onto the new rows. */
  hold(): (next: HTMLElement | undefined) => void;
}

/** One rule for what a query keeps: the label or the detail text, case-insensitive. */
export function filterMatch(e: { label: string; detail?: string }, query: string): boolean {
  return query === '' || `${e.label} ${e.detail ?? ''}`.toLowerCase().includes(query.toLowerCase());
}

/** The best match to land on: the first checkbox row, else the first row. */
export const firstMatch = (rows: HTMLElement[]): HTMLElement | undefined =>
  rows.find((r) => r.getAttribute('role') === 'menuitemcheckbox') ?? rows[0];

/** The query after a keystroke on a level with no field, or null when the key is not filtering. */
export function nextQuery(ev: KeyboardEvent, query: string): string | null {
  const printable = ev.key.length === 1 && ev.key !== ' ' && !ev.ctrlKey && !ev.metaKey && !ev.altKey;
  if (printable) return query + ev.key;
  return ev.key === 'Backspace' && query !== '' ? query.slice(0, -1) : null;
}

/** The level's combobox text field, kept across repaints. */
export function buildFilterField(
  state: FilterState,
  filter: FilterSpec,
  listId: string,
  onQuery: (q: string) => void,
): HTMLInputElement {
  const input = state.input ??= createPillInput({ placeholder: filter.hint, ariaLabel: filter.hint, extraClass: 'ctx-filter-input' });
  if (!input.dataset.wired) {
    input.dataset.wired = '1';
    input.addEventListener('input', () => onQuery(input.value));
  }
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-expanded', 'true');
  input.setAttribute('aria-autocomplete', 'list');
  input.setAttribute('aria-controls', listId);
  return input;
}

/** The query line a field-less level shows while the query is non-empty. */
export function buildQueryLine(query: string): HTMLElement {
  const f = document.createElement('div');
  f.className = 'ctx-filter';
  f.setAttribute('role', 'presentation');
  f.setAttribute('aria-live', 'polite');
  f.textContent = query;
  return f;
}

export function buildEmptyHint(text: string): HTMLElement {
  const none = document.createElement('div');
  none.className = 'ctx-hint';
  none.setAttribute('role', 'presentation');
  none.textContent = text;
  return none;
}

export interface ComboboxDeps {
  state: FilterState;
  menu: HTMLElement;
  rows(): HTMLElement[];
  /** The menu now holds the keyboard (so closing returns focus to the invoker). */
  engage(): void;
  setQuery(q: string): void;
}

export function createComboboxCursor(d: ComboboxDeps): Cursor {
  const { state } = d;
  const focusInput = (): void => {
    d.engage();
    state.input?.focus({ preventScroll: true });
  };
  const setActive = (el: HTMLElement | null): void => {
    if (!state.input) return;
    state.active?.classList.remove('is-active');
    state.active = el;
    if (el) {
      el.classList.add('is-active');
      state.input.setAttribute('aria-activedescendant', el.id);
      el.scrollIntoView?.({ block: 'nearest' });
    } else {
      state.input.removeAttribute('aria-activedescendant');
    }
  };
  return {
    current: () => state.active ?? null,
    entry: () => firstMatch(d.rows()),
    move: (el) => { d.engage(); setActive(el); },
    ownsNavKeys: false,
    followsPointer: false,
    enter: focusInput,
    land(first) { setActive(first ?? null); focusInput(); },
    clear() {
      if (!state.input || state.input.value === '') return false;
      state.input.value = '';
      d.setQuery('');
      return true;
    },
    hold() {
      const had = state.active;
      const input = state.input!;
      const sel = [input.selectionStart, input.selectionEnd] as const;
      return (next) => {
        state.active = null;
        if (had) setActive(next ?? null);
        focusInput();
        if (sel[0] !== null && sel[1] !== null) input.setSelectionRange(sel[0], sel[1]);
      };
    },
  };
}
