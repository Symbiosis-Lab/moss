/**
 * Slash command menu for shortcode and block command insertion.
 * '/' at line start OR after whitespace opens a floating menu; selecting
 * inserts the appropriate syntax.
 *
 * Architecture notes:
 * - coordsAtPos() MUST NOT be called inside ViewPlugin.update() — throws
 *   "Reading the editor layout isn't allowed during an update". Use requestMeasure().
 * - Menu DOM uses position:fixed (editor container has overflow:auto — clips absolute).
 * - ViewPlugin constructor reads from view.state directly (ViewUpdate private constructor).
 */

// popup-dismiss: closes when the caret leaves the '/' trigger and on editor blur (slashBlurHandler); never anchored away from the text.
import { EditorView, ViewPlugin, ViewUpdate, keymap } from '@codemirror/view';
import { Extension, Prec, StateEffect, StateField } from '@codemirror/state';
import { SLASH_CATALOG, applyShortcodeTemplate, ts, type SlashCommandSpec } from './cm-shortcode-catalog.js';
import { suppressTooltips } from "../components/tooltip";

export type SlashContext = { query: string; from: number } | null;

/**
 * Find the last "/" in lineText that is either at index 0 or immediately
 * preceded by whitespace, is before or at cursorInLine, and whose following
 * text up to cursorInLine contains no whitespace.
 *
 * Returns `{ query, from }` where `from` is the line-relative offset of the "/".
 * Returns null if no valid slash position is found.
 */
export function parseSlashContext(lineText: string, cursorInLine: number): SlashContext {
  // Scan backwards from cursor looking for a valid trigger "/"
  for (let i = cursorInLine - 1; i >= 0; i--) {
    if (lineText[i] !== '/') continue;

    // The slash must be at index 0 or preceded by whitespace
    const prevCh = i > 0 ? lineText[i - 1] : '';
    if (i > 0 && !/\s/.test(prevCh)) {
      // mid-word slash (e.g. "http://", "path/to") — keep scanning left
      // but don't return a context for this slash
      continue;
    }

    // The character right after "/" must not be whitespace (e.g. "/ text" is invalid)
    const afterSlash = lineText[i + 1];
    if (afterSlash !== undefined && /\s/.test(afterSlash)) return null;

    // The text from slash+1 to cursor must not contain whitespace (query can't have spaces)
    const query = lineText.slice(i + 1, cursorInLine);
    if (query.includes(' ') || query.includes('\t')) return null;

    return { query, from: i };
  }

  return null;
}

// ── State ───────────────────────────────────────────────────────────────

export interface SlashMenuState {
  active: boolean;
  query: string;
  triggerFrom: number;
  selectedIndex: number;
}

const CLOSED: SlashMenuState = { active: false, query: '', triggerFrom: 0, selectedIndex: 0 };

// Exported for unit tests (activating the menu state in a test view).
export const setSlashMenu = StateEffect.define<SlashMenuState>();

// Exported for unit tests (asserting active state after close).
export const slashMenuField = StateField.define<SlashMenuState>({
  create: () => CLOSED,
  update(val, tr) {
    for (const e of tr.effects) {
      if (e.is(setSlashMenu)) return e.value;
    }
    return val;
  },
});

// ── Helpers ─────────────────────────────────────────────────────────────

function filteredItems(query: string): SlashCommandSpec[] {
  const q = query.toLowerCase();
  return SLASH_CATALOG.filter(s =>
    s.name.startsWith(q) || ts(s.labelKey).toLowerCase().startsWith(q)
  );
}

/**
 * Insert the selected command through the appropriate path:
 * - shortcode: applyShortcodeTemplate (snippet with tab stops)
 * - block command: spec.apply() (plain dispatch, no Tab-stop conflicts)
 * Exported for unit tests.
 */
export function applyInsert(view: EditorView, spec: SlashCommandSpec, state: SlashMenuState): void {
  const triggerTo = state.triggerFrom + 1 + state.query.length;
  // Close the menu first.
  view.dispatch({ effects: setSlashMenu.of(CLOSED) });
  if (spec.kind === 'block') {
    spec.apply(view, state.triggerFrom, triggerTo);
  } else {
    // shortcode: existing snippet path
    applyShortcodeTemplate(view, spec, state.triggerFrom, triggerTo, { fencePrefix: true });
  }
  view.focus();
}

/**
 * Close the menu WITHOUT selecting a command, deleting the provisional "/query"
 * trigger text so an abandoned trigger never lands in the document (and the
 * published site). Guarded: only deletes when the span still begins with "/"
 * (defends against position drift). Used by Escape, blur, and cursor-left-context.
 * Exported for unit tests.
 */
export function removeTriggerAndClose(view: EditorView): void {
  const state = view.state.field(slashMenuField);
  if (!state.active) return;
  const from = state.triggerFrom;
  const to = Math.min(from + 1 + state.query.length, view.state.doc.length);
  const startsWithSlash = view.state.doc.sliceString(from, from + 1) === '/';
  view.dispatch({
    changes: startsWithSlash ? { from, to, insert: '' } : undefined,
    effects: setSlashMenu.of(CLOSED),
    userEvent: 'delete',
  });
}

// ── ViewPlugin ──────────────────────────────────────────────────────────

class SlashMenuPlugin {
  private dom: HTMLElement | null = null;

  constructor(private view: EditorView) {
    // Read initial state directly — do NOT call this.update(plainObject)
    // ViewUpdate has a private constructor and cannot be created from plain objects.
    const state = view.state.field(slashMenuField);
    if (state.active) {
      this.renderMenu(state);
      this.scheduleMeasure();
    }
  }

  update(update: ViewUpdate) {
    const prev = update.startState.field(slashMenuField);
    const curr = update.state.field(slashMenuField);

    if (!curr.active) {
      this.hideMenu();
      return;
    }

    // Check if cursor moved away from the slash context
    const line = update.state.doc.lineAt(curr.triggerFrom);
    const cursorInLine = update.state.selection.main.head - line.from;
    const ctx = parseSlashContext(line.text, cursorInLine);
    if (!ctx) {
      // The trigger context is gone. If the cursor merely MOVED away (selection
      // changed, doc unchanged) the "/query" is an abandoned trigger → delete it
      // so it never reaches the published site. If the user TYPED something that
      // invalidated the query (docChanged — e.g. a space), that text is now
      // authored content → keep it, just close.
      const abandoned = !update.docChanged;
      // Defer dispatch out of the update cycle — dispatching synchronously from
      // inside ViewPlugin.update() can cause reentrant update loops (CM6 anti-pattern).
      queueMicrotask(() => {
        if (abandoned) removeTriggerAndClose(update.view);
        else update.view.dispatch({ effects: setSlashMenu.of(CLOSED) });
      });
      return;
    }

    // Update query if cursor advanced
    if (ctx.query !== curr.query) {
      queueMicrotask(() => update.view.dispatch({
        effects: setSlashMenu.of({ ...curr, query: ctx.query, selectedIndex: 0 }),
      }));
      return;
    }

    // Re-render if state or geometry changed
    if (prev !== curr || update.geometryChanged || update.viewportChanged) {
      this.renderMenu(curr);
      this.scheduleMeasure();
    }
  }

  private scheduleMeasure() {
    // coordsAtPos() is safe in requestMeasure's read phase (not in update())
    this.view.requestMeasure({
      read: (view) => {
        const state = view.state.field(slashMenuField);
        if (!state.active) return null;
        const coords = view.coordsAtPos(state.triggerFrom);
        if (!coords) return null;
        // Measure the menu's own width HERE (read phase); reading layout in the
        // write phase would force an extra reflow.
        return { coords, menuW: this.dom ? this.dom.offsetWidth : 0 };
      },
      write: (data) => {
        if (!this.dom || !data) return;
        const { coords, menuW } = data;
        const MARGIN = 8;
        const vh = window.innerHeight;
        const vw = window.innerWidth;
        const spaceBelow = vh - coords.bottom - MARGIN;
        const spaceAbove = coords.top - MARGIN;
        // position:fixed → viewport coordinates. Prefer below the "/"; flip
        // above only when below is cramped AND above has more room. Either way,
        // cap max-height to the chosen gap so the menu stays inside the editor
        // viewport and SCROLLS (overflow-y:auto) instead of being clipped by the
        // window edge. Inline max-height overrides the CSS 60vh fallback.
        const placeAbove = spaceBelow < 180 && spaceAbove > spaceBelow;
        if (placeAbove) {
          this.dom.style.top = 'auto';
          this.dom.style.bottom = `${vh - coords.top + 4}px`;
          this.dom.style.maxHeight = `${Math.max(0, spaceAbove)}px`;
        } else {
          this.dom.style.bottom = 'auto';
          this.dom.style.top = `${coords.bottom + 4}px`;
          this.dom.style.maxHeight = `${Math.max(0, spaceBelow)}px`;
        }
        // Keep the menu within the viewport horizontally too.
        this.dom.style.left = `${Math.max(MARGIN, Math.min(coords.left, vw - menuW - MARGIN))}px`;
        // Keep the arrow-highlighted item visible as the user navigates a list
        // that overflows the capped height.
        this.dom.querySelector<HTMLElement>('.is-active')
          ?.scrollIntoView({ block: 'nearest' });
      },
    });
  }

  private releaseTooltips: (() => void) | null = null;

  private renderMenu(state: SlashMenuState) {
    const items = filteredItems(state.query);
    if (items.length === 0) { this.hideMenu(); return; }

    if (!this.dom) {
      this.dom = document.createElement('div');
      this.dom.className = 'cm-slash-menu moss-menu';
      document.body.appendChild(this.dom); // body for fixed positioning
      // `--z-overlay` — above a `--z-popup` tooltip. Opening on a keystroke is
      // no protection: nothing dismisses a tooltip on a keypress but Escape,
      // so one parked under a stationary pointer survives typing "/", and
      // re-crossing any editor-chrome trigger while the menu is up re-fires
      // `mouseover` underneath it.
      this.releaseTooltips = suppressTooltips(this.dom);
    }

    this.dom.innerHTML = '';
    items.forEach((item, i) => {
      const el = document.createElement('div');
      el.className = 'cm-slash-menu-item moss-menu-row' +
        (i === state.selectedIndex ? ' is-active' : '');

      const label = document.createElement('span');
      label.className = 'cm-slash-menu-item-label';
      label.textContent = ts(item.labelKey);

      const detail = document.createElement('span');
      detail.className = 'cm-slash-menu-item-detail';
      detail.textContent = ts(item.detailKey);

      el.appendChild(label);
      el.appendChild(detail);
      el.addEventListener('mousedown', (e) => {
        e.preventDefault();
        applyInsert(this.view, item, state);
      });
      this.dom?.appendChild(el);
    });
  }

  private hideMenu() {
    if (this.dom) { this.dom.remove(); this.dom = null; }
    this.releaseTooltips?.();
    this.releaseTooltips = null;
  }

  destroy() { this.hideMenu(); }
}

// ── Input handler ───────────────────────────────────────────────────────

function slashInputHandler(): Extension {
  return EditorView.inputHandler.of((view, from, to, text) => {
    if (text !== '/') return false;
    const line = view.state.doc.lineAt(from);
    // Fire when "/" is at line start OR immediately after whitespace.
    // Blocks "http://", "path/to", and mid-word slashes.
    const prevCh = from > line.from ? view.state.doc.sliceString(from - 1, from) : '';
    if (from !== line.from && !/\s/.test(prevCh)) return false;
    view.dispatch({
      changes: { from, to, insert: '/' },
      // Place the caret AFTER the inserted "/". Without an explicit selection,
      // CM6 maps the cursor to BEFORE the insertion (default association -1), so
      // the ViewPlugin computes cursorInLine=0, parseSlashContext returns null,
      // and the menu is closed (as "cursor left the slash") before it can render.
      selection: { anchor: from + 1 },
      effects: setSlashMenu.of({ active: true, query: '', triggerFrom: from, selectedIndex: 0 }),
      userEvent: 'input.type',
    });
    return true;
  });
}

// ── Keyboard handler ────────────────────────────────────────────────────

/**
 * Menu navigation keys. Implemented as a KEYMAP (not domEventHandlers) and
 * wrapped in Prec.highest by slashMenuExtension() so ArrowUp/Down/Enter beat
 * CM6's default cursor keymap when the menu is showing options. Each `run`
 * returns false when the menu is not showing options, so normal Arrow/Enter/
 * Escape behaviour is preserved. Escape only needs the menu to be active (so a
 * menu showing no matches can still be cancelled + its "/" removed).
 */
function slashKeyHandler(): Extension {
  // The active state IFF the menu is showing options (has items). Returns null
  // otherwise, so the binding falls through to the default keymap.
  const shownState = (view: EditorView): SlashMenuState | null => {
    const state = view.state.field(slashMenuField);
    if (!state.active || filteredItems(state.query).length === 0) return null;
    return state;
  };
  return keymap.of([
    {
      key: 'ArrowDown',
      run: (view) => {
        const state = shownState(view);
        if (!state) return false;
        const items = filteredItems(state.query);
        view.dispatch({ effects: setSlashMenu.of({
          ...state, selectedIndex: Math.min(state.selectedIndex + 1, items.length - 1),
        }) });
        return true;
      },
    },
    {
      key: 'ArrowUp',
      run: (view) => {
        const state = shownState(view);
        if (!state) return false;
        view.dispatch({ effects: setSlashMenu.of({
          ...state, selectedIndex: Math.max(state.selectedIndex - 1, 0),
        }) });
        return true;
      },
    },
    {
      key: 'Enter',
      run: (view) => {
        const state = shownState(view);
        if (!state) return false;
        const selected = filteredItems(state.query)[state.selectedIndex];
        if (selected) applyInsert(view, selected, state);
        return true;
      },
    },
    {
      key: 'Escape',
      run: (view) => {
        if (!view.state.field(slashMenuField).active) return false;
        removeTriggerAndClose(view);
        return true;
      },
    },
  ]);
}

// ── Blur handler ────────────────────────────────────────────────────────

/**
 * When the editor loses focus while the menu is open (e.g. the user clicks
 * outside the editor), treat it as an abandoned trigger and remove the
 * "/query". Clicking a menu item does NOT blur — the item's mousedown calls
 * preventDefault — so this never fires for a real selection.
 */
function slashBlurHandler(): Extension {
  return EditorView.domEventHandlers({
    blur(_e, view) {
      if (!view.state.field(slashMenuField).active) return false;
      // Defer: mutating the doc synchronously during the blur event is unsafe.
      queueMicrotask(() => removeTriggerAndClose(view));
      return false;
    },
  });
}

export function slashMenuExtension(): Extension {
  return [
    slashMenuField,
    slashInputHandler(),
    // Prec.highest so ArrowUp/Down/Enter/Escape reach the menu BEFORE CM6's
    // default keymap (registered earlier in the editor's extension array via
    // minimalSetup), which would otherwise move the cursor and dismiss the menu.
    Prec.highest(slashKeyHandler()),
    slashBlurHandler(),
    ViewPlugin.fromClass(SlashMenuPlugin),
  ];
}
