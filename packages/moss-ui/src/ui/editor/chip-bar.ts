// Chip bar — schema-driven horizontal frontmatter editor.
// Renders one chip per set field. Click to edit via popover. Boolean chips toggle inline.
// Priority-based overflow hides lowest-priority chips first.
// Unified "+" / "+N" button shows overflow + add-property dropdown.

import type { ContentSchema, FieldDefinition, WikilinkCompletion } from '@symbiosis-lab/moss-ui';
import type { PageFacts } from '@symbiosis-lab/moss-ui';
import { openOverlay, type OverlayHandle } from './overlay';
import { attachTermSuggestions, followTermChip, refreshVaultTerms } from './chip-term-nav';
import { fromNameList, toNameList } from "../utils/name-list";
import { joinCtxGroups, openCtxMenu, type CtxMenuEntry } from "../shared/ui/ctx-menu";
import { armOutsideDismiss } from "../shared/ui/dismiss-outside";
import { stripWikilinkBrackets, wikilinkTarget, wrapWikilink } from '@symbiosis-lab/moss-syntax';
import type { FrontmatterStore, FrontmatterValues } from './frontmatter-store';
import { suggestionFor, paintSuggestion, sameSuggestions, type Suggestions } from './expand-derived/chip-suggestions';
import { t, chipLabel, chipDetail, fieldHint, formatDate, onLocaleChange, type UiStringMap } from "../i18n/index";
import { sourceAssetUrl } from './source-asset-url';
import { browseButton, canBrowse } from './asset-picker/browse-button';
import { deepEqual as chipValuesEqual } from './document-history';
import { appliesHere, autoHint, NO_PAGE_FACTS } from './field-applies';
import { langName } from "../i18n/lang-names";
import { createGenerationToken } from "../shared/utils/generation-token";
import { engine } from '../engine';

/** A chip field entry: key, schema definition, and current value. */
interface ChipField { key: string; def: FieldDefinition | null; value: unknown; suggestion?: string }

/**
 * The open file fills a layout slot instead of becoming a page. Mirrors the
 * Rust `SourceRole` (`describe_source_role`) — `slot` is e.g. `"footer-left"`,
 * `language` is the language-tree bucket when the slot file lives under one.
 */
export interface SlotRole { slot: string; language: string | null }

/** Which segment of the union chip a value selects. */
function unionMode(v: unknown): 'on' | 'off' | 'custom' {
  if (v === false || v == null) return 'off';
  if (v === true) return 'on';
  if (Array.isArray(v)) return v.length ? 'custom' : 'on';
  return typeof v === 'string' && v !== '' ? 'custom' : 'on';
}

// ── SVG Icons (boolean checkbox only) ────────────────────────────────

const SVG_CHECK_ON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 12l2 2 4-4"/></svg>';
const SVG_CHECK_OFF = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/></svg>';

// ── Asset thumbnail object-fit ───────────────────────────────────────
// `cover` fills the square (photos look best); `contain` letterboxes on the
// tile and never crops (logos / icons / transparency). MIME is the only signal
// ResolvedAsset gives us — there is no alpha flag — so `cover` is an allowlist
// of reliably-photographic formats and everything else defaults to `contain`.
// If ResolvedAsset ever gains `has_alpha`, replace this with `has_alpha ? 'contain' : 'cover'`.
const COVER_MIME = new Set(['image/jpeg', 'image/webp', 'image/avif']);

export function assetObjectFit(mime: string | null | undefined): 'cover' | 'contain' {
  return mime && COVER_MIME.has(mime) ? 'cover' : 'contain';
}

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/**
 * Serialize a frontmatter value the way "Copy value" hands it out: the plain
 * scalar the file holds — a bare string for strings (no YAML quoting; the
 * user is pasting a VALUE, not YAML source), `a, b` flow form for arrays
 * (what the chip itself displays, and what pastes back into a tag editor),
 * `true`/`false` for booleans, decimal for numbers, and '' for the
 * null/empty family (absence is the canonical empty).
 */
export function serializeChipValue(value: unknown): string {
  if (value == null) return '';
  if (Array.isArray(value)) return value.map(v => String(v)).join(', ');
  return String(value);
}

// Chip tooltips say what the field MEANS (schema-sourced `fm_hint.<key>`
// strings). Portal tooltip via
// `data-tooltip` (never `title=`), muted style: these are explanations, not
// action labels. Unknown user fields have no hint and get no tooltip.
function applyFieldHint(chip: HTMLElement, key: string): void {
  const hint = fieldHint(key);
  if (!hint) return;
  // Bottom placement: the chips run horizontally at the TOP of the doc
  // surface — a side tooltip covers the neighbouring chip, a top one clips.
  Object.assign(chip.dataset, { tooltip: hint, tooltipStyle: 'muted', tooltipPosition: 'bottom' });
}

// Equality check for the values map: when an external file-changed event
// echoes back our own save, setValues should be a no-op rather than fight
// in-progress input. Frontmatter values are scalars (string, number,
// boolean, null) or string[] (tags), which this covers.
function shallowEqualValues(a: Record<string, unknown>, b: Record<string, unknown>): boolean {
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  for (const k of aKeys) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    const av = a[k];
    const bv = b[k];
    if (Array.isArray(av) && Array.isArray(bv)) {
      if (av.length !== bv.length) return false;
      for (let i = 0; i < av.length; i++) if (av[i] !== bv[i]) return false;
    } else if (av !== bv) {
      return false;
    }
  }
  return true;
}

/**
 * Preferred group display order. Groups not in this list sort to the end alphabetically.
 * Five scope groups ordered broad to narrow:
 *   1. "This Page"   — per-page content + display properties
 *   2. "Child Pages" — controls how children are listed
 *   3. "Child Styles"— visual/layout controls for child listings
 *   4. "Whole Site"  — properties read from homepage affecting the whole site
 *   5. "Other"       — unknown user-authored fields (catch-all)
 */
const GROUP_SORT_ORDER = [
  'This Page', 'Child Pages', 'Child Styles', 'Whole Site', 'Other',
];

// Maps schema English group names (used for sorting) to i18n display keys.
// GROUP_SORT_ORDER stays English — sort compares against schema group strings.
// Translation happens only at render time when setting header text.
/**
 * Ids for the "+" menu's listbox and its rows. `aria-activedescendant` points
 * at a row BY ID, so every row needs one and they must be unique in the
 * document; only one of these menus is open at a time, but a stale id would
 * silently point an assistive tech at nothing.
 */
const OMENU_LISTBOX_ID = 'omenu-listbox';
const OMENU_SETBOX_ID = 'omenu-setbox';
let MENU_ROW_SEQ = 0;

const GROUP_KEY_MAP: Record<string, keyof UiStringMap> = {
  'This Page':   'group.this_page',
  'Child Pages': 'group.child_pages',
  'Child Styles':'group.child_styles',
  'Whole Site':  'group.whole_site',
  'Other':       'group.other',
};

// ── Layout & timing constants ────────────────────────────────────────
//
// NOTE: there is deliberately NO "reserved width" constant for the unified
// "+" / "+N" button. The button lives in its own flex box (.chip-unified-btn,
// flex:0 0 auto) as a sibling of the clipping .chip-track. Its reachability is
// a LAYOUT invariant, not a width prediction — so overflow is measured against
// the track's real right edge, never `bar.right - someReserve`. See the
// two-box comment in editor.css.
// Do not re-introduce a pixel reserve here.

/** Duration of the FLIP slide animation when a hidden field is promoted. */
const PROMOTE_FLIP_MS = 220;

/**
 * Debounce before the file-picker (cover) edit input re-queries
 * `wikilinkCompletions` on each keystroke. That command does a full
 * uncached recursive project-tree walk server-side, so firing it on every
 * keystroke wastes backend work even though the requestId guard in
 * buildFilePickerEditor keeps a stale response from ever being shown.
 * Matches the store's own DEFAULT_DEBOUNCE_MS (frontmatter-store.ts) for a
 * consistent typing-to-settle feel across the chip bar.
 */
const SEARCH_DEBOUNCE_MS = 150;

/**
 * Dwell before a chip hover is treated as intent and announced to the shell
 * (which flashes what the field renders as in the preview). Scrubbing the
 * pointer across the property bar crosses several chips in well under this,
 * so a pass-through never strobes the preview.
 */
const HOVER_INTENT_MS = 150;

// ── Autocorrect policy ───────────────────────────────────────────────
//
// Two classes of field:
//   Prose (text-area widget): descriptive text that benefits from OS-level
//     spellcheck and autocapitalize. We still disable autocorrect (OS
//     autocorrect silently rewrites URLs and field values mid-keystroke).
//   Identifier (text-input, tag-input, and any structured picker): machine-
//     readable values (slug, URL, lang, translationKey, tags …). Full
//     suppression: autocorrect + autocapitalize + spellcheck off, plus
//     autocomplete=off so the browser does not suggest previous field values.
//
// The discriminator is the WIDGET TYPE string — text-area is unambiguously
// prose; text-input is unambiguously identifier (even `title` uses text-input
// in the schema, so it follows the same rule for consistency).

function applyCorrectionPolicy(
  el: HTMLInputElement | HTMLTextAreaElement,
  isProse: boolean,
): void {
  // autocorrect=off on all fields — OS autocorrect silently rewrites
  // structured values (URLs, dates, identifiers) mid-keystroke.
  el.setAttribute('autocorrect', 'off');
  if (isProse) {
    // Prose: keep OS-level spellcheck and autocapitalize (first-word cap,
    // proper-noun detection) — they help composition without mangling values.
    el.setAttribute('autocapitalize', 'sentences');
    el.spellcheck = true;
  } else {
    // Identifier: suppress all correction surfaces.
    el.setAttribute('autocapitalize', 'off');
    el.spellcheck = false;
    el.autocomplete = 'off';
  }
}

// ── Widget editors table ─────────────────────────────────────────────
// Every "simple" widget (i.e. anything that maps to a single <input>,
// <select>, or <textarea>) shares the same shell: pop header, focus on
// open, Escape to close, commit on edit. Per-widget differences are:
//   - element kind + initial value formatting
//   - which event fires the commit (input vs change)
//   - how to coerce the raw string back to the value the YAML cares about
//   - whether Enter closes the popover (true for text-input only — Enter
//     in a number field, date field, etc. is meaningful to the widget)
//
// Tag-input is NOT in this table: it has a different DOM (chip-list + add
// input) and multi-key handling (Enter to add, Backspace on empty to pop)
// that doesn't fold into the build/event/parse triple.

type WidgetInputEl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;

interface WidgetEditorSpec {
  build(def: FieldDefinition | null, value: unknown): WidgetInputEl;
  event: 'change' | 'input';
  parse(raw: string): unknown;
  closeOnEnter?: boolean;
}

const WIDGET_EDITORS: Record<string, WidgetEditorSpec> = {
  'date-picker': {
    build: (_def, v) => {
      const el = document.createElement('input');
      el.type = 'date';
      const s = String(v ?? '');
      if (s) el.value = s.length > 10 ? s.slice(0, 10) : s;
      // Date fields are structured/identifier — full suppression.
      applyCorrectionPolicy(el, false);
      return el;
    },
    event: 'change',
    parse: (raw) => raw || null,
  },
  'select': {
    build: (def, v) => {
      const el = document.createElement('select');
      const empty = document.createElement('option');
      empty.value = '';
      empty.textContent = t('chip.ui.select_placeholder');
      el.appendChild(empty);
      const cur = String(v ?? '');
      for (const opt of def?.enum_values ?? []) {
        const o = document.createElement('option');
        o.value = opt;
        o.textContent = opt;
        if (opt === cur) o.selected = true;
        el.appendChild(o);
      }
      return el;
    },
    event: 'change',
    parse: (raw) => raw || null,
  },
  'text-area': {
    build: (_def, v) => {
      const el = document.createElement('textarea');
      el.value = String(v ?? '');
      el.rows = 3;
      // Prose widget: keep autocapitalize + spellcheck, only disable autocorrect.
      applyCorrectionPolicy(el, true);
      return el;
    },
    event: 'input',
    parse: (raw) => raw,
  },
  'number-input': {
    build: (_def, v) => {
      const el = document.createElement('input');
      el.type = 'number';
      el.value = v != null ? String(v) : '';
      // Numbers are structured — full suppression.
      applyCorrectionPolicy(el, false);
      return el;
    },
    event: 'input',
    parse: (raw) => raw ? Number(raw) : null,
  },
  'text-input': {
    build: (_def, v) => {
      const el = document.createElement('input');
      el.type = 'text';
      el.value = String(v ?? '');
      // Identifier widget: full suppression (url, slug, lang, translationKey,
      // title, publisher, external_url — all text-input fields are
      // machine-readable or short-form identifiers in the moss schema).
      applyCorrectionPolicy(el, false);
      return el;
    },
    event: 'input',
    parse: (raw) => raw,
    closeOnEnter: true,
  },
};

/** Sort group names by GROUP_SORT_ORDER, unknowns at end alphabetically. */
function sortGroups(groups: string[]): string[] {
  return [...groups].sort((a, b) => {
    const ai = GROUP_SORT_ORDER.indexOf(a);
    const bi = GROUP_SORT_ORDER.indexOf(b);
    return (ai < 0 ? 999 : ai) - (bi < 0 ? 999 : bi) || a.localeCompare(b);
  });
}

// ── ChipBar ─────────────────────────────────────────────────────────

export class ChipBar {
  private schema: ContentSchema;
  private store: FrontmatterStore;
  // The store no longer carries a path; the active file path is pulled live
  // from editor-main (registry-backed) so asset resolution targets the file
  // currently being edited even across renames.
  private getActivePath: () => string;
  /** True while the Browse button's system file dialog is up — see the
   *  `suppressBlur` note on the edit popover. */
  private assetDialogOpen = false;
  private afterVaultImport: () => Promise<void>;
  // Project root path, used to enumerate folders for union-field autocomplete.
  private getProjectRoot: () => string | undefined;
  private unsubscribe: () => void;
  private container: HTMLElement | null = null;
  private abortController: AbortController | null = null;

  // Memoize editorResolveAsset calls by (filePath + '|' + value) for the
  // lifetime of this ChipBar instance. Prevents an RPC-per-keystroke storm:
  // every store notify re-runs renderChips(), which would re-call the command
  // for every file-picker chip even when the chip's value hasn't changed.
  // The cache is keyed by both filePath and value so it naturally invalidates
  // when either changes (the new key misses and fires a fresh RPC).
  private assetResolveCache = new Map<string, Promise<Awaited<ReturnType<typeof engine.editorResolveAsset>>>>();

  private resolveAssetCached(pathStr: string, filePath: string): Promise<Awaited<ReturnType<typeof engine.editorResolveAsset>>> {
    const key = filePath + '|' + pathStr;
    let promise = this.assetResolveCache.get(key);
    if (!promise) {
      promise = engine.editorResolveAsset(pathStr, filePath);
      this.assetResolveCache.set(key, promise);
    }
    return promise;
  }

  /**
   * Tracks which overlay (if any) is currently open. The chip bar needs to
   * answer two questions: "is anything open right now?" (so we can close it
   * before opening a new one) and "is the open overlay the edit popover for
   * key X?" (for click-toggle behavior — clicking the same chip twice
   * should close the popover, not reopen it).
   *
   * Discriminated by `kind`:
   *   - 'edit': editing a specific field (`key` is set)
   *   - 'ctx': right-click context menu on a specific field (`key` is set)
   *   - 'unified': add-property / hidden-overflow dropdown (no key)
   */
  private activeOverlay:
    | { kind: 'edit'; key: string; handle: OverlayHandle }
    | { kind: 'ctx'; key: string; handle: OverlayHandle }
    | { kind: 'unified'; handle: OverlayHandle }
    | null = null;

  // Overflow state
  private chipEls: { el: HTMLElement; field: ChipField; priority: number }[] = [];
  // The clipping content box that holds the chips. Sibling of unifiedBtn
  // inside the chip-bar. Overflow is measured against THIS box's right edge,
  // not the bar's — see the two-box layout note on the layout constants.
  private track: HTMLElement | null = null;
  private unifiedBtn: HTMLElement | null = null;
  /** Polite live region for changes moss made on the user's behalf (undo). */
  private liveRegion: HTMLElement | null = null;
  private announceTimer: ReturnType<typeof setTimeout> | null = null;
  private hiddenFields: ChipField[] = [];
  private resizeObserver: ResizeObserver | null = null;
  // Fields the user has promoted by editing them from the more menu. Sorting
  // puts these before priority-ordered fields (most-recently promoted first),
  // and measureOverflow treats them as highest priority so they don't
  // immediately fall back into overflow.
  private promotedKeys: string[] = [];
  // Non-null when the open file is a layout slot file (`footer.md`). The bar
  // then renders one read-only chip and nothing else — see renderChips.
  private slotRole: SlotRole | null = null;
  private pageFacts: PageFacts = NO_PAGE_FACTS;
  /** @see the `onFieldEdit` constructor option. */
  private onFieldEdit: (key: string, before: unknown, after: unknown) => void = () => {};
  /**
   * The value of the field an open edit popover owns, as it was when the
   * popover opened. `closePop` compares against it and emits at most one
   * history entry for the whole visit.
   */
  private editSnapshot: { key: string; before: unknown } | null = null;
  /**
   * Set for the duration of destroy(). A teardown closes the popover, and that
   * close must not emit a history entry: it runs after the store is gone and
   * after the load generation has moved on, so the entry it stamped would be a
   * forged one for the INCOMING document. See the plan's lifetime section.
   */
  private tearingDown = false;
  // Set when setValues() is called with values that differ from the current
  // map AND a popover is open. The full renderChips() is deferred until the
  // popover closes (so the user's in-progress input is not destroyed). When
  // values are equal, setValues short-circuits before reaching this flag.
  private pendingRefresh: boolean = false;
  // Unsubscribe for the live-language-switch relabel.
  private localeUnsub: () => void = () => {};
  // Set when a locale change arrives while a popover is open; the deferred full
  // relabel runs on popover close (onOverlayClosed).
  private localeRefreshPending: boolean = false;
  // Hover-intent state for the chip-hover → preview-flash gesture. The timer
  // holds a not-yet-announced hover (cancelled if the pointer leaves within
  // HOVER_INTENT_MS); hoverAnnouncedField is the field the shell was told
  // about, so hover-out (and destroy) can send the clearing `field: null`.
  private hoverIntentTimer: ReturnType<typeof setTimeout> | null = null;
  private hoverAnnouncedField: string | null = null;
  private suggestions: Suggestions = {};
  // Transactional add (design-vocabulary: "Property add is transactional;
  // absence is the canonical empty"). Set when the user clicked an add row:
  // the chip renders ghosted and the STORE deliberately has no entry — the
  // file must not change until the first commit. Dismissing (Escape, ×,
  // click-away) drops this and the ghost; nothing was ever written.
  // Provisionality lives here, in UI state, NOT in the store: the store
  // stays the single source of persisted truth.
  // `seed`: the value the page already has (Nav settings); a seeded ghost previews it.
  private provisional: { key: string; def: FieldDefinition | null; widget: string; seed?: unknown } | null = null;
  // Removes the document-level dismiss listeners of a provisional checkbox
  // ghost (which has no popover to own its dismissal). Null when none armed.
  private provisionalDismissCleanup: (() => void) | null = null;
  // Set for exactly one closePop(): right-clicking a provisional ghost closes
  // its edit popover to make room for the context menu, and that close must
  // NOT be read as the user dismissing the ghost (onOverlayClosed's usual
  // rule). The ctx menu then owns the ghost's fate: "Remove field" dismisses,
  // anything else re-arms the document-level dismissal.
  private retainProvisionalOnClose = false;

  constructor(
    schema: ContentSchema,
    store: FrontmatterStore,
    opts?: {
      getActivePath?: () => string;
      getProjectRoot?: () => string | undefined;
      /** Refresh the file tree after the Browse button imports a file into
       *  the vault (editor-main owns the tree; see afterVaultImport). */
      afterVaultImport?: () => Promise<void>;
      /** Set when the open file is a layout slot file (`footer.md`). */
      slotRole?: SlotRole | null;
      /**
       * One frontmatter step for the combined document history. Called with the
       * value BEFORE and AFTER a completed edit — once per popover visit, not
       * once per keystroke: `text-input` widgets commit on every `input` event,
       * so a store-level hook would put sixty entries in the log for a sixty
       * character description and make ⌘Z strictly worse than the native undo
       * it replaces.
       */
      onFieldEdit?: (key: string, before: unknown, after: unknown) => void;
    },
  ) {
    this.schema = schema;
    this.store = store;
    this.getActivePath = opts?.getActivePath ?? (() => '');
    this.getProjectRoot = opts?.getProjectRoot ?? (() => undefined);
    this.afterVaultImport = opts?.afterVaultImport ?? (async () => {});
    this.slotRole = opts?.slotRole ?? null;
    this.onFieldEdit = opts?.onFieldEdit ?? (() => {});
    // Subscribe to store changes: re-render when values change externally.
    // The store notifies immediately on setField so chip-bar reflects every
    // user interaction without buffering. External writes (onFileChanged) also
    // flow through the same subscriber path, keeping both surfaces in sync.
    this.unsubscribe = this.store.subscribe((_values) => {
      // While a popover is open, defer the full re-render to avoid destroying
      // the user's in-progress input. Mark pendingRefresh so closePop fires
      // renderChips on close. The chip behind the popover is live-painted by
      // repaintChipValue independently of this path.
      if (this.activeOverlay) {
        this.pendingRefresh = true;
      } else if (this.container) {
        this.renderChips();
      }
    });
    // Live language switch (no restart): rebuild the chip bar (field labels,
    // chip labels/tooltips) under the new locale. While a popover is open, an
    // in-progress edit must not be destroyed — defer via localeRefreshPending
    // and rebuild when the popover closes (onOverlayClosed).
    this.localeUnsub = onLocaleChange(() => {
      if (!this.container) return;
      if (this.activeOverlay) this.localeRefreshPending = true;
      else this.render(this.container);
    });
  }

  render(container: HTMLElement): void {
    if (this.abortController) this.abortController.abort();
    this.abortController = new AbortController();
    this.container = container;
    // NOTE: the `data-inline-task-anchor="save"` anchor used to live HERE on
    // the chip-bar, but it moved to the titlebar (`#moss-save-status` in
    // editor.html). The chip-bar is per-file and `render()`/`destroy()`
    // innerHTML-clear it on every file switch, which wiped a persistent
    // "Failed: …" save badge. The titlebar is stable across
    // switches, so the save inline-status badge now anchors there.
    // No double-click on the empty bar: a hidden gesture has to shadow a visible
    // control, and this one shadowed none. The divider double-click toggles the tree
    // in both directions; the breadcrumb chevron is the visible control.
    //
    // Hovering a chip (past a short intent dwell) flashes what the field
    // renders as in the preview — the reverse direction of the preview-click
    // vocabulary, riding the same `data-source-fm` anchors. Delegated on the
    // container (pointerenter doesn't bubble) so re-renders don't re-wire it;
    // relatedTarget checks skip pointer moves WITHIN one chip's children.
    container.addEventListener('pointerover', (e) => {
      const chip = (e.target as Element | null)?.closest?.('.chip[data-key]') as HTMLElement | null;
      if (!chip) return;
      const from = (e as PointerEvent).relatedTarget as Node | null;
      if (from && chip.contains(from)) return; // intra-chip move, already hovering
      this.onChipHoverStart(chip.dataset.key!);
    }, { signal: this.abortController.signal });
    container.addEventListener('pointerout', (e) => {
      const chip = (e.target as Element | null)?.closest?.('.chip[data-key]') as HTMLElement | null;
      if (!chip) return;
      const to = (e as PointerEvent).relatedTarget as Node | null;
      if (to && chip.contains(to)) return; // still inside this chip
      this.onChipHoverEnd();
    }, { signal: this.abortController.signal });
    // Which fields carry terms is a build fact (`ArticleMap.kinds`), and
    // cmd+click has to decide synchronously whether it owns the gesture — so
    // the table is refreshed here, a file-open ahead of any chip click, not
    // fetched at click time.
    void refreshVaultTerms();
    this.renderChips();
  }

  /** Arm the hover-intent timer for `field`; announces after HOVER_INTENT_MS. */
  private onChipHoverStart(field: string): void {
    if (this.hoverIntentTimer) clearTimeout(this.hoverIntentTimer);
    this.hoverIntentTimer = setTimeout(() => {
      this.hoverIntentTimer = null;
      this.hoverAnnouncedField = field;
      document.dispatchEvent(
        new CustomEvent('editor:chip-hover', { detail: { field }, bubbles: true }),
      );
    }, HOVER_INTENT_MS);
  }

  /** Cancel a pending intent; clear an announced flash with `field: null`. */
  private onChipHoverEnd(): void {
    if (this.hoverIntentTimer) {
      clearTimeout(this.hoverIntentTimer);
      this.hoverIntentTimer = null;
    }
    if (this.hoverAnnouncedField !== null) {
      this.hoverAnnouncedField = null;
      document.dispatchEvent(
        new CustomEvent('editor:chip-hover', { detail: { field: null }, bubbles: true }),
      );
    }
  }

  /**
   * Return the current frontmatter values. Delegates to the store — ChipBar
   * holds no value state of its own; the store is the single source of truth.
   */
  getValues(): FrontmatterValues {
    return this.store.getValues();
  }

  /**
   * Replace all values from an EXTERNAL source (file reloaded from disk).
   * The subscriber re-render path handles popover preservation and the
   * equality short-circuit (see the constructor's subscribe call).
   */
  setValues(values: FrontmatterValues): void {
    this.store.onFileChanged(values);
  }

  /** Derived values the Title and Description chips show while those fields are unset. */
  setSuggestions(next: Suggestions): void {
    if (sameSuggestions(this.suggestions, next)) return;
    this.suggestions = { ...next };
    if (this.activeOverlay) this.pendingRefresh = true; else if (this.container) this.renderChips();
  }

  /**
   * Programmatically write a single frontmatter field. Used by the editor
   * heading element when the user edits the heading in title-mode.
   *
   * Empty string vs undefined: passing `''` preserves the field with an
   * explicit empty value (writes `title: ""` to YAML). Passing `undefined`
   * deletes the field. This distinction is load-bearing for the title
   * consolidation: `title: ""` is the explicit "no auto-injected H1" signal,
   * while a missing `title:` field falls back to filename-mode.
   *
   * Delegates to the store so the write is debounced and echoes suppressed.
   * The store subscriber notifies ChipBar to re-render.
   */
  setFieldValue(key: string, value: unknown): void {
    // Not routed through commitField, so it needs its own capture: a
    // Title-source heading commit IS a frontmatter write ({key: 'title'}) and
    // is fully expressible as a history entry.
    const before = this.store.getValues()[key];
    if (value === undefined) {
      this.store.removeField(key);
    } else {
      // setFieldValue is called from heading-edit (user commits a title).
      // Treat as immediate (same as select/toggle) — heading commits are
      // discrete user gestures, not character-by-character input.
      this.store.setField(key, value, { immediate: true });
    }
    this.emitOutsideOverlay(key, before);
    // Repaint the chip behind any open popover — mirrors an in-popover
    // edit. The store subscriber fires renderChips when no popover is open.
    if (this.activeOverlay) {
      this.pendingRefresh = true;
      this.repaintChipValue(key);
    }
  }

  destroy(): void {
    // closePop() below fires onOverlayClosed, whose emit would run against a
    // store that is about to be destroyed and stamp itself with the INCOMING
    // document's generation — forging exactly the stale entry the stamp exists
    // to prevent. The history's own reset() runs before this, not after.
    this.tearingDown = true;
    // A file switch mid-hover must not leave the old page's wash standing.
    this.onChipHoverEnd();
    this.disarmProvisionalDismiss();
    this.provisional = null;
    this.closePop();
    this.unsubscribe();
    this.localeUnsub();
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.abortController?.abort();
    this.abortController = null;
    this.chipEls = [];
    this.track = null;
    this.unifiedBtn = null;
    this.liveRegion = null;
    if (this.announceTimer !== null) clearTimeout(this.announceTimer);
    this.announceTimer = null;
    this.hiddenFields = [];
    this.promotedKeys = [];
    this.pendingRefresh = false;
    this.assetResolveCache.clear();
    if (this.container) this.container.innerHTML = '';
    this.container = null;
  }

  /**
   * Show the user where a frontmatter field lives — nothing more.
   *
   * The caller is a click in the PREVIEW (article title, date, byline,
   * colophon). A preview click is a question, not an edit: it may point at the
   * chip, and it must not open the chip's editor or take focus. Opening the
   * popover here moved the caret into the editor webview mid-gesture and
   * collapsed whatever the user was selecting, which made preview text
   * unselectable.
   *
   * A chip sitting in the "+N" overflow menu is promoted to the front of the
   * strip first — pointing at something invisible is not pointing. Promotion
   * reuses the same `promotedKeys` ordering the more-menu uses, so there is
   * one answer to "which chips come first".
   */
  revealField(fieldName: string, opts: { promote?: boolean } = {}): void {
    if (!this.container) return;
    // `hiddenFields` alone is not enough for a chip that was just CREATED (undo
    // of a removal): a key that did not exist a moment ago cannot appear in a
    // measurement of the previous render, and that measurement is taken in a
    // rAF, so at this instant every chip still reports visible. Without the
    // explicit flag the wash plays for one frame and the chip then collapses
    // into the overflow `+N` — the case the feedback was written for.
    if (opts.promote || this.hiddenFields.some(f => f.key === fieldName)) {
      this.promoteToFront(fieldName);
    }
    const chip = this.container.querySelector(
      `.chip[data-key="${CSS.escape(fieldName)}"]`,
    ) as HTMLElement | null;
    if (!chip) return;
    chip.scrollIntoView({ behavior: 'smooth', block: 'center' });
    // A wash, not the just-committed flash: nothing was committed. Same
    // vocabulary as the preview's `moss-slot-reveal` — a pointer, in the quiet
    // accent, so it cannot be misread as "your edit landed".
    chip.classList.remove('moss-revealed');
    void chip.offsetWidth; // force reflow so a second reveal restarts the wash
    chip.classList.add('moss-revealed');
  }

  /**
   * The double-click strength of {@link revealField}: a DOUBLE click in the
   * preview commits to the edit, so open the chip's editor exactly as clicking the chip
   * itself would — same popover, same autofocus.
   *
   * A checkbox chip's click interaction is a value TOGGLE, not an opener;
   * synthesizing it would silently flip a frontmatter value from a preview
   * gesture, so those fall back to the reveal wash instead.
   */
  openField(fieldName: string, seed?: unknown): void {
    if (!this.container) return;
    if (this.hiddenFields.some(f => f.key === fieldName)) {
      this.promoteToFront(fieldName);
    }
    const chip = this.container.querySelector(
      `.chip[data-key="${CSS.escape(fieldName)}"]`,
    ) as HTMLElement | null;
    const def = this.schema.frontmatter.fields[fieldName] ?? null;
    const widget = def?.widget ?? this.inferWidget(this.store.getValues()[fieldName]);
    if (!chip) {
      if (!def || widget !== 'checkbox') return;
      this.beginProvisionalAdd({ key: fieldName, def, value: undefined }, widget, seed);
      this.revealField(fieldName);
      return;
    }
    if (widget === 'checkbox') {
      this.revealField(fieldName);
      return;
    }
    // Scroll instantly, then open a frame later: the CM scroller's scroll
    // listener closes any popover, and a smooth scroll would keep re-closing it.
    chip.scrollIntoView({ block: 'center' });
    requestAnimationFrame(() => chip.click());
  }

  /**
   * Flash a chip with the shared just-committed confirmation (the green
   * `.moss-just-committed` pulse — see editor.css). Restart-safe (remove +
   * reflow + add) so a rapid second edit re-triggers the animation. No-op if
   * the chip for `key` is not currently rendered (e.g. overflow-hidden).
   */
  /**
   * True while an edit popover is open. ⌘Z has to consult this before reaching
   * into the history: `showEdit` skips autofocus for the union widget and there
   * is no overlay-level Escape handler, so a freshly opened popover can have
   * focus on nothing — and an undo would then revert some older change while
   * the popover sits there with an uncommitted snapshot.
   */
  hasOpenEditOverlay(): boolean {
    return this.activeOverlay?.kind === 'edit';
  }

  /** Close an open edit popover, flushing its history entry. */
  closeEditOverlay(): void {
    if (this.activeOverlay?.kind === 'edit') this.closePop();
  }

  /** Close whatever overlay is open, any kind. The chip bar scrolls with the
   *  document (Amendment III), and a body-portaled popover would float away
   *  from its anchor — the scroller's scroll closes it instead. */
  closeAnyOverlay(): void {
    this.closePop();
  }

  flashChip(key: string): void {
    if (!this.container) return;
    const chip = this.container.querySelector(`.chip[data-key="${CSS.escape(key)}"]`) as HTMLElement | null;
    if (!chip) return;
    chip.classList.remove('moss-just-committed');
    void chip.offsetWidth; // force reflow so the next class add restarts the animation
    chip.classList.add('moss-just-committed');
  }

  /**
   * Signal that a field was REMOVED, which no chip-based signal can carry:
   * `revealField` and `flashChip` both no-op when the element is absent, and
   * renderChips skips a field whose value is undefined — so an undo that
   * deletes a property, the case most in need of feedback, would show nothing
   * at all. Flash the add button instead, and name what left.
   */
  flashAddButton(removedKey: string): void {
    const btn = this.unifiedBtn;
    if (!btn) return;
    btn.classList.remove('moss-just-committed');
    void btn.offsetWidth; // force reflow so a repeated removal re-animates
    btn.classList.add('moss-just-committed');
    // The flash only says "look here" — it has to be SAID which key went, and
    // only a live region announces a change; same idiom as the save badges
    // (inline-status.ts).
    this.announce(t('chip.removed_field').replace('{field}', chipLabel(removedKey)));
  }

  /**
   * Say something once, politely. Created on demand and kept for the bar's
   * life: a region inserted and populated in the same tick is not announced by
   * most screen readers, so it has to already be in the tree when the text
   * lands, which the empty-then-fill split below arranges.
   */
  private announce(message: string): void {
    if (!this.container) return;
    // `renderChips()` rebuilds the container's children, so a region from a
    // previous render can be detached — a detached region announces nothing.
    if (!this.liveRegion?.isConnected) {
      const el = document.createElement('div');
      el.className = 'sr-only';
      el.setAttribute('role', 'status');
      el.setAttribute('aria-live', 'polite');
      this.container.appendChild(el);
      this.liveRegion = el;
    }
    const region = this.liveRegion;
    region.textContent = '';
    // Clear, then fill on the next tick. Whether a screen reader repeats
    // identical live-region text is AT-dependent — not something the DOM spec
    // promises either way — so the empty tick between is what makes a second
    // removal of the SAME key reliably audible. Cancelling a pending fill keeps
    // that honest if two ever land in one task: `'' → A → A` would collapse
    // whatever the AT does with it.
    if (this.announceTimer !== null) clearTimeout(this.announceTimer);
    this.announceTimer = setTimeout(() => {
      this.announceTimer = null;
      region.textContent = message;
    }, 0);
  }

  // ── Chip Rendering ──────────────────────────────────────────────────

  /**
   * Render the read-only bar for a slot file: a single inert pill naming what
   * the file is, no field chips, no add-property button.
   *
   * `data-tooltip` (not `title=`) per the project tooltip convention — the chip
   * bar is stationary chrome, not a floating overlay.
   */
  private renderSlotChip(bar: HTMLElement): void {
    const track = document.createElement('div');
    track.className = 'chip-track';
    bar.appendChild(track);
    this.track = track;
    this.chipEls = [];
    this.hiddenFields = [];
    this.unifiedBtn = null;

    const chip = document.createElement('span');
    chip.className = 'chip chip--static';
    chip.setAttribute('data-slot-role', this.slotRole?.slot ?? '');
    Object.assign(chip.dataset, { tooltip: t('slot.footer.tooltip'), tooltipPosition: 'bottom' });

    const label = document.createElement('span');
    label.textContent = t('slot.footer.label');
    chip.appendChild(label);

    // A language-scoped slot file (`zh-hans/footer.md`) only affects pages in
    // that language tree. Naming the bucket pre-empts "I edited the Chinese
    // footer and nothing changed" — the author was looking at an English page.
    const lang = this.slotRole?.language;
    if (lang) {
      const sep = document.createElement('span');
      sep.className = 'chip-sep';
      chip.appendChild(sep);
      const langEl = document.createElement('span');
      langEl.className = 'chip-key';
      langEl.textContent = langName(lang);
      chip.appendChild(langEl);
    }

    track.appendChild(chip);
  }

  private renderChips(): void {
    this.closePop();
    this.disarmProvisionalDismiss();
    const bar = this.container;
    if (!bar) return;
    bar.innerHTML = '';
    const signal = this.abortController?.signal;

    // A slot file (`footer.md`) fills layout chrome and emits no page, and NO
    // frontmatter field changes how it renders: the slot collector reads only
    // `slot:` (redundant on a root footer.md), and the language bucket comes
    // from the PATH, not frontmatter. `title` has no page to title, and
    // `draft`/`listed` are already forced false by `slot_only`. So the bar
    // shows one read-only chip and no add-property button — offering `+` here
    // would invite fields that silently do nothing.
    //
    // The FrontmatterStore is still constructed by the caller and still holds
    // any existing values: `saveActiveDocument` serializes `getValues()` over
    // the whole file, so dropping the store would DELETE leftover frontmatter
    // on the first autosave. Hidden must not mean deleted.
    if (this.slotRole) {
      this.renderSlotChip(bar);
      return;
    }

    const schemaFields = this.schema.frontmatter.fields;
    const schemaKeys = Object.keys(schemaFields);

    // Schema-declared internal fields (mirrors `skip_schema: true` in
    // BUILTIN_FIELDS) — present in the FrontMatter struct but not rendered
    // as chips. getValues() still returns them so they round-trip through
    // saveEditorContent unchanged. Defaults to [] for plugin schemas that
    // were loaded from JSON predating internal_fields.
    const internalFields = new Set(this.schema.frontmatter.internal_fields ?? []);

    // Read current values from the store — ChipBar owns no value state.
    const currentValues = this.store.getValues();

    // Only render chips for fields that are present in the file. `null` is
    // deliberately NOT skipped: a legacy `key: null` in the YAML renders an
    // explicit "Empty" chip — the property bar never hides what is in the
    // file.
    const fields: ChipField[] = [];
    for (const key of schemaKeys) {
      const value = currentValues[key];
      const suggestion = suggestionFor(key, currentValues, this.suggestions);
      if (value === undefined && !suggestion) continue;
      fields.push({ key, def: schemaFields[key], value, suggestion });
    }
    const unknownKeys = Object.keys(currentValues)
      .filter(k => !schemaKeys.includes(k))
      .filter(k => !internalFields.has(k));
    for (const key of unknownKeys) {
      fields.push({ key, def: null, value: currentValues[key] });
    }

    // A provisional add renders as a ghost chip until its first commit — the
    // store (and the file) deliberately hold nothing for it yet.
    if (this.provisional && currentValues[this.provisional.key] === undefined) {
      fields.push({ key: this.provisional.key, def: this.provisional.def, value: undefined });
    }

    // Promoted keys go first (in promotion order, most recent first).
    // Remaining fields sort by priority ascending (lower number = more important).
    fields.sort((a, b) => {
      const ai = this.promotedKeys.indexOf(a.key);
      const bi = this.promotedKeys.indexOf(b.key);
      if (ai >= 0 && bi >= 0) return ai - bi;
      if (ai >= 0) return -1;
      if (bi >= 0) return 1;
      return (a.def?.priority || 100) - (b.def?.priority || 100);
    });

    // Two-box layout: chips go in the clipping .chip-track; the unified
    // button is a SEPARATE sibling box that can never be pushed off-screen by
    // chip content. The track's flex:1 1 auto pushes the button to the edge
    // (no spacer needed). See the two-box note in editor.css — do not append
    // chips directly to `bar` or the button loses its structural guarantee.
    const track = document.createElement('div');
    track.className = 'chip-track';
    bar.appendChild(track);
    this.track = track;

    // Render chips into the track
    const chipEls: { el: HTMLElement; field: ChipField; priority: number }[] = [];
    for (const f of fields) {
      const el = this.mkChip(f.key, f.def, f.value, signal, this.provisional?.key === f.key);
      if (f.suggestion) paintSuggestion(el, el.querySelector('.chip-value') as HTMLElement, f.suggestion);
      const priority = f.def?.priority || 100;
      chipEls.push({ el, field: f, priority });
      track.appendChild(el);
    }
    this.chipEls = chipEls;

    // A provisional checkbox ghost has no popover to own its dismissal, so
    // it gets document-level click-away/Escape listeners instead.
    if (this.provisional?.widget === 'checkbox') {
      const ghost = chipEls.find(c => c.field.key === this.provisional!.key);
      if (ghost) this.armProvisionalDismiss(ghost.el);
    }

    // Unified "+" / "+N" button — sibling of the track, not inside it.
    const btn = document.createElement('span');
    btn.className = 'chip-unified-btn round';
    btn.textContent = '+';
    // A real tab stop, not just a click target: the menu it opens hands focus
    // BACK here on Escape, and a control that cannot hold focus makes that
    // restoration a no-op that drops the user at <body>.
    btn.setAttribute('role', 'button');
    btn.setAttribute('aria-label', t('chip.add_property'));
    btn.setAttribute('aria-haspopup', 'listbox');
    btn.tabIndex = 0;
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      if (this.activeOverlay?.kind === 'unified') { this.closePop(); return; }
      this.closePop();
      btn.classList.add('on');
      this.showUnifiedDropdown(btn);
    }, { signal });
    btn.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter' && e.key !== ' ') return;
      e.preventDefault();
      btn.click();
    }, { signal });
    bar.appendChild(btn);
    this.unifiedBtn = btn;

    // Initial measurement after layout
    requestAnimationFrame(() => this.measureOverflow());

    // Live re-measurement on resize (e.g. divider drag)
    this.resizeObserver?.disconnect();
    this.resizeObserver = new ResizeObserver(() => this.measureOverflow());
    this.resizeObserver.observe(bar);
  }

  /**
   * Priority-based overflow: hide lowest-priority chips (highest number) first.
   * High-priority chips like title (10), tags (50), draft (60) stay visible longest.
   *
   * The boundary is the .chip-track's real right edge — NOT a bar-minus-reserve
   * prediction. The unified button is in its own box past the track, so there
   * is nothing to reserve for: a chip fits iff its right edge ≤ the track's.
   * (Two-box invariant, Principle 3 in the design doc.)
   */
  private measureOverflow(): void {
    const bar = this.container;
    const track = this.track;
    if (!bar?.isConnected || !track || !this.chipEls.length || !this.unifiedBtn) return;

    // Pre-size the button to its widest ("+N") form BEFORE reading the track
    // right edge. The unified button is `flex:0 0 auto` — its intrinsic width
    // is subtracted from the bar by the flex engine, leaving the rest for the
    // track. When the button is in the narrow `.round` form (32px) the track
    // appears wider than it will be once overflow is found and the button grows
    // to "+N" form (padding: 0 10px, wider). Measuring `trackRight` while the
    // button is still round causes the chip budget to overcount: chips that
    // appear to fit within the round-button track right edge get clipped when
    // the button later expands. Forcing the button wide first ensures the track
    // right edge we capture is the worst-case (smallest) value — so no chip
    // that we mark "visible" will be clipped after `updateUnifiedBtn()` fires.
    // `updateUnifiedBtn()` called below always restores the correct text + class.
    const btn = this.unifiedBtn;
    if (btn.classList.contains('round')) {
      // Temporarily expand to "+N" form: remove .round and set a placeholder
      // text. Any "+N" text works; the exact count doesn't matter for sizing.
      btn.classList.remove('round');
      btn.textContent = '+1';
    }

    const trackRight = track.getBoundingClientRect().right;

    // Show all chips first to get accurate measurements
    for (const { el } of this.chipEls) el.style.display = '';

    // Fast check — does any chip exceed the track edge?
    const hasOverflow = this.chipEls.some(({ el }) =>
      el.getBoundingClientRect().right > trackRight
    );

    if (!hasOverflow) {
      // Restore the button to its narrow round form and update label.
      // `updateUnifiedBtn` writes the correct text and class — no need to
      // manually re-add `.round` here.
      this.hiddenFields = [];
      this.updateUnifiedBtn();
      return;
    }

    // Sort by priority descending (highest number = lowest priority = hide first).
    // Promoted keys get an artificial priority of -1 so they sort to the end and
    // are the LAST chips to be hidden — the user just expressed interest by
    // editing them from the more menu, so don't snap them back into overflow.
    //
    // CRITICAL: "-1" makes promoted chips hide LAST, not NEVER. The loop still
    // visits them, so a promoted chip that overflows the track even as the sole
    // visible chip IS demoted to the more menu (whole-chip granularity — it is
    // hidden entirely, never left straddling the track edge to be clipped).
    // This cap is what keeps a wide promoted chip from clipping the more-menu
    // button. Do not special-case promoted chips out of this loop.
    // (Principles 2+4 in the design doc.)
    const sortedByHidePriority = [...this.chipEls].sort((a, b) => {
      const ap = this.promotedKeys.includes(a.field.key) ? -1 : a.priority;
      const bp = this.promotedKeys.includes(b.field.key) ? -1 : b.priority;
      return bp - ap;
    });

    const hidden = new Set<HTMLElement>();
    for (const entry of sortedByHidePriority) {
      entry.el.style.display = 'none';
      hidden.add(entry.el);

      // Check if remaining visible chips now fit within the track.
      const stillOverflows = this.chipEls
        .filter(c => !hidden.has(c.el))
        .some(({ el }) => {
          el.style.display = '';
          const r = el.getBoundingClientRect().right;
          return r > trackRight;
        });

      if (!stillOverflows) break;
    }

    this.hiddenFields = this.chipEls
      .filter(c => hidden.has(c.el))
      .map(c => c.field);

    this.updateUnifiedBtn();
  }

  private updateUnifiedBtn(): void {
    if (!this.unifiedBtn) return;
    const n = this.hiddenFields.length;
    if (n > 0) {
      this.unifiedBtn.textContent = `+${n}`;
      this.unifiedBtn.classList.remove('round');
    } else {
      this.unifiedBtn.textContent = '+';
      this.unifiedBtn.classList.add('round');
    }
  }

  /** Cmd/ctrl+click on a term field's surface navigates to the term's page
   *  (chip-term-nav.ts); the bar chip resolves the field's first value. */
  private followTermChip(e: MouseEvent, key: string, name?: string): boolean {
    return followTermChip(e, key, this.store.getValues()[key], name);
  }

  /** Decide the add list's fields and their automatic values; refresh after each build. */
  setPageFacts(facts: PageFacts): void { this.pageFacts = facts; }

  private mkChip(key: string, def: FieldDefinition | null, value: unknown, signal?: AbortSignal, provisional = false): HTMLElement {
    const c = document.createElement('span');
    c.className = 'chip';
    c.dataset.key = key;
    if (provisional) c.classList.add('chip--provisional');
    const widget = provisional
      ? (this.provisional?.widget ?? def?.widget ?? this.inferWidget(value))
      : (def?.widget ?? this.inferWidget(value));

    // Boolean chip: toggle on click; a provisional ghost's first click commits its preview.
    if (widget === 'checkbox') {
      // A seeded ghost previews the seed and its click writes the flip.
      const seed = provisional ? this.provisional?.seed : undefined;
      const preview = seed !== undefined ? !!seed : !!this.defaultValueForWidget('checkbox', def);
      const isOn = provisional ? preview : !!value;
      c.classList.add(isOn ? 'bool-on' : 'bool-off');
      const ic = document.createElement('span');
      ic.className = 'chip-icon';
      ic.innerHTML = isOn ? SVG_CHECK_ON : SVG_CHECK_OFF;
      c.appendChild(ic);
      const k = document.createElement('span');
      k.className = 'chip-key';
      k.textContent = chipLabel(key);
      c.appendChild(k);
      applyFieldHint(c, key);
      const sep = document.createElement('span');
      sep.className = 'chip-sep';
      c.appendChild(sep);
      const lbl = document.createElement('span');
      lbl.className = 'chip-bool-label';
      lbl.textContent = isOn ? '\u2713' : '\u2014';
      c.appendChild(lbl);

      c.addEventListener('click', (e) => {
        e.stopPropagation();
        if (provisional) {
          // First commit: materialize the ghost with the value it previews.
          this.commitField(key, seed !== undefined ? !preview : preview, { immediate: true });
          return;
        }
        // Boolean toggle is a discrete gesture → immediate write. Through
        // commitField, not the store directly, so it lands in the combined
        // history: a `draft: true` the user regrets is exactly the kind of
        // frontmatter change ⌘Z is expected to take back.
        this.commitField(key, !this.store.getValues()[key], { immediate: true });
        // Store subscriber fires renderChips() (no popover is open during a
        // checkbox toggle — checkbox chips have no edit popover path).
      }, { signal });
      c.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.showChipCtx(e, key);
      }, { signal });
      return c;
    }

    // Standard chip: key | value
    const k = document.createElement('span');
    k.className = 'chip-key';
    k.textContent = chipLabel(key);
    c.appendChild(k);
    applyFieldHint(c, key);
    const sep = document.createElement('span');
    sep.className = 'chip-sep';
    c.appendChild(sep);
    const v = document.createElement('span');
    v.className = 'chip-value';
    // Union chips (children, series) are standard chips whose popover holds
    // the three-way choice; the pill states the value once, in words.
    if (widget === 'union') this.renderUnionValue(v, def, value);
    else this.renderChipValue(v, widget, value);
    c.appendChild(v);

    c.addEventListener('click', (e) => {
      if (this.followTermChip(e, key)) return;
      e.stopPropagation();
      if (this.activeOverlay?.kind === 'edit' && this.activeOverlay.key === key) {
        this.closePop();
        return;
      }
      this.closePop();
      c.classList.add('on');
      this.showEdit(c, key, def, widget);
    }, { signal });
    c.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.showChipCtx(e, key);
    }, { signal });
    return c;
  }

  /** Union pill value: one word. A missing value shows the field's default
   *  (the state the build applies), never "empty". A folder shows its name,
   *  ellipsized by CSS, with the full value in the tooltip. */
  private renderUnionValue(el: HTMLElement, def: FieldDefinition | null, value: unknown): void {
    el.classList.remove('chip-value-mono');
    el.removeAttribute('title');
    const v = value === undefined ? this.defaultValueForWidget('union', def) : value;
    const mode = unionMode(v);
    if (mode === 'custom') {
      const full = Array.isArray(v) ? v.map(x => stripWikilinkBrackets(String(x))).join(', ') : stripWikilinkBrackets(String(v));
      const shown = Array.isArray(v) ? full : full.split('/').filter(Boolean).pop() ?? full;
      el.textContent = shown;
      el.title = full;
    } else {
      el.textContent = t(mode === 'on' ? 'chip.ui.on' : 'chip.ui.off');
    }
  }

  private renderChipValue(el: HTMLElement, widget: string, value: unknown): void {
    // Truncation is handled by CSS (.chip-value max-width + text-overflow: ellipsis)
    // so the cut-off width matches what fits on screen rather than a fixed
    // character count \u2014 important for CJK where one char \u2248 two Latin chars.
    el.classList.remove('chip-value-mono');

    // Absence is the canonical empty: a `key: null` / `key: ""` present in
    // the file (or a provisional ghost, whose value is undefined) shows an
    // explicit "Empty" label \u2014 the same treatment tags always had \u2014 instead
    // of hiding the key. moss itself never writes these; clearing removes
    // the key (commitField).
    if (value == null || value === '') {
      el.innerHTML = `<span class="chip-value-empty">${t('chip.ui.empty')}</span>`;
      return;
    }

    if (widget === 'tag-input') {
      const names = toNameList(value);
      if (names.length === 0) {
        el.innerHTML = `<span class="chip-value-empty">${t('chip.ui.empty')}</span>`;
      } else {
        el.innerHTML = names.map(n => `<span class="chip-accent">${esc(n)}</span>`)
          .join('<span class="chip-tag-sep">, </span>');
      }
    } else if (widget === 'date-picker') {
      // Parse the stored YYYY-MM-DD string as UTC midnight so a bare date is
      // never shifted by the viewer's negative offset (e.g. UTC-8 would roll
      // a bare Jan 1 back to Dec 31 without the explicit 'Z'). Display with
      // dateStyle:'medium' for a locale-friendly label (no time component).
      // (The empty case is handled by the "Empty" guard above.)
      const s = String(value);
      const isoDate = s.length >= 10 ? s.slice(0, 10) : s;
      const d = new Date(isoDate + 'T00:00:00Z');
      el.textContent = isNaN(d.getTime())
        ? s
        : formatDate(d, { dateStyle: 'medium', timeZone: 'UTC' });
    } else if (widget === 'file-picker') {
      el.classList.add('chip-value-mono');
      const pathStr = String(value ?? '');
      // The stored value may be an Obsidian wikilink/embed (e.g. a cover
      // `cover: '[[DSCF4053.jpeg]]'`). Resolve and display the bare asset path
      // — same unwrap the build does — so the chip isn't spuriously "broken".
      const assetPath = wikilinkTarget(pathStr);
      // Render asset widget: 32px thumbnail + path text, async resolution
      el.innerHTML = '';
      const assetWrap = document.createElement('span');
      assetWrap.className = 'chip-bar-asset';

      const img = document.createElement('img');
      img.className = 'asset-thumb';
      img.alt = '';
      img.src = ''; // will be set after resolution
      assetWrap.appendChild(img);

      const pathSpan = document.createElement('span');
      pathSpan.className = 'asset-path';
      pathSpan.textContent = assetPath;
      assetWrap.appendChild(pathSpan);

      el.appendChild(assetWrap);

      // Async: resolve the asset and update the thumbnail.
      // Uses a per-instance cache keyed by (filePath, value) so repeated
      // renderChips() calls (triggered by store notifies) don't fire a new
      // RPC for each file-picker chip on every keystroke.
      const filePath = this.getActivePath();
      void this.resolveAssetCached(assetPath, filePath).then((result) => {
        if (result.status !== 'ok') {
          assetWrap.classList.add('is-unresolved');
          return;
        }
        const asset = result.data;
        if (!asset) {
          assetWrap.classList.add('is-unresolved');
          return;
        }
        // Resolved \u2014 set thumbnail source. Call the getter at this point
        // (NOT at constructor time) so the preview-server URL is picked up
        // even if the server started after this ChipBar was constructed. Same
        // pattern as cm-asset-hover.
        // Resolved — set thumbnail source via moss-source:// (source images,
        // no preview server required).
        assetWrap.classList.remove('is-unresolved');
        img.style.objectFit = assetObjectFit(asset.mime_type);
        img.src = sourceAssetUrl(asset.request_url);
      }).catch(() => {
        assetWrap.classList.add('is-unresolved');
      });
    } else {
      el.textContent = String(value ?? '');
    }
  }

  /**
   * Route a popover/toggle commit to the store, enforcing the transactional
   * vocabulary:
   *   - the FIRST commit is what materializes a provisional chip; before it,
   *     nothing was ever written, so dismissal leaves the file untouched;
   *   - `null` means "cleared", and clearing removes the key — absence is
   *     the canonical empty; `key: null` residue is never written.
   * Every write originating inside an edit popover goes through here, never
   * through `store.setField` directly.
   */
  private commitField(key: string, value: unknown, opts: { immediate?: boolean } = {}): void {
    if (value === null) {
      // A provisional chip cleared to empty has nothing to remove — the key
      // was never written. It stays provisional; dismissal drops the ghost.
      if (this.provisional?.key === key) return;
      const beforeRemove = this.store.getValues()[key];
      this.store.removeField(key);
      this.emitOutsideOverlay(key, beforeRemove);
      return;
    }
    if (this.provisional?.key === key) {
      this.provisional = null;
      this.disarmProvisionalDismiss();
      // The ghost is now real: shed the provisional styling in place (the
      // full re-render is deferred while its popover stays open).
      this.chipEls.find(c => c.field.key === key)?.el.classList.remove('chip--provisional');
    }
    const before = this.store.getValues()[key];
    this.store.setField(key, value, opts);
    // An immediate write that happens WHILE an edit popover is open belongs to
    // that popover's span — the boundary owns it. Emitting here as well would
    // double count: adding tags `a` then `b` and closing would log {[],[a]},
    // {[a],[a,b]} AND {[],[a,b]}, so the second undo walks the value forward.
    this.emitOutsideOverlay(key, before);
  }

  /**
   * Record a completed frontmatter step, unless an edit popover is open — in
   * which case its boundary will record the whole visit as one.
   */
  private emitOutsideOverlay(key: string, before: unknown): void {
    if (this.tearingDown) return;
    // The popover owns the writes to ITS OWN field, and only those. The
    // boundary snapshot in onOverlayClosed covers `snap.key` alone, so
    // suppressing every key while a popover is open would drop a write to a
    // different field out of the log entirely — applied to the store, absent
    // from undo. No path does that today (the heading and create-flow writers
    // fire on blur, which precedes the chip's click), so this is a trap set for
    // the next writer rather than a live bug.
    if (this.activeOverlay?.kind === 'edit' && this.activeOverlay.key === key) return;
    const after = this.store.getValues()[key];
    if (!chipValuesEqual(before, after)) this.onFieldEdit(key, before, after);
  }

  /**
   * Document-level click-away/Escape dismissal for a provisional checkbox
   * ghost, which has no popover to own that gesture. Registration is
   * deferred a tick (same as openOverlay) so the click that created the
   * ghost cannot dismiss it. Commit and re-render paths disarm via
   * renderChips/destroy.
   */
  /** Drop a provisional ghost: nothing was ever written, the file never
   *  changed. The one dismissal path shared by click-away/Escape (via
   *  armProvisionalDismiss) and the ghost's context-menu "Remove field". */
  private dismissProvisional(): void {
    this.disarmProvisionalDismiss();
    if (!this.provisional) return;
    this.promotedKeys = this.promotedKeys.filter(k => k !== this.provisional!.key);
    this.provisional = null;
    this.renderChips();
  }

  private armProvisionalDismiss(chipEl: HTMLElement): void {
    this.disarmProvisionalDismiss();
    const dismiss = () => this.dismissProvisional();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') dismiss();
    };
    // Click-away goes through the shared helper: a bubble-phase document
    // listener misses a click on a window drag region (the breadcrumb strip
    // right above this bar) and every click in another webview entirely.
    const disarmOutside = armOutsideDismiss({ inside: [chipEl], covers: [], onDismiss: dismiss });
    const timerId = window.setTimeout(() => {
      document.addEventListener('keydown', onKey);
    }, 0);
    this.provisionalDismissCleanup = () => {
      window.clearTimeout(timerId);
      disarmOutside();
      document.removeEventListener('keydown', onKey);
      this.provisionalDismissCleanup = null;
    };
  }

  private disarmProvisionalDismiss(): void {
    this.provisionalDismissCleanup?.();
  }

  // Repaint just the value portion of one chip in place \u2014 used while a
  // popover is open so the chip behind the popover stays in sync with what
  // the user is typing. Does not run renderChips() (which would close the
  // popover) or measure overflow. Looks up the chip via chipEls (direct
  // refs) and the value span via its `.chip-value` class so the helper
  // does not depend on the precise DOM order inside mkChip().
  private repaintChipValue(key: string): void {
    const entry = this.chipEls.find(c => c.field.key === key);
    if (!entry) return;
    const def = this.schema.frontmatter.fields[key] ?? null;
    const value = this.store.getValues()[key];
    const widget = def?.widget ?? this.inferWidget(value);
    // Boolean chips re-render via renderChips on toggle (no popover path).
    // Union chips go through the popover path and CAN repaint here.
    if (widget === 'checkbox') return;
    const valueSpan = entry.el.querySelector('.chip-value') as HTMLElement | null;
    if (!valueSpan) return;
    if (widget === 'union') this.renderUnionValue(valueSpan, def, value);
    else this.renderChipValue(valueSpan, widget, value);
    const suggestion = suggestionFor(key, this.store.getValues(), this.suggestions);
    if (suggestion) paintSuggestion(entry.el, valueSpan, suggestion);
  }

  // ── Popovers ────────────────────────────────────────────────────────

  /**
   * Close the currently-open overlay (if any). Triggers the overlay's
   * onClose, which clears `activeOverlay`, removes `.on` styling, and
   * fires a deferred re-render if `pendingRefresh` was set while the
   * overlay was open.
   *
   * Idempotent — safe to call when nothing is open. Named `closePop`
   * for historical continuity; covers all three overlay kinds.
   */
  private closePop(): void {
    this.activeOverlay?.handle.close();
  }

  /** Shared post-close cleanup wired into every openOverlay({ onClose }). */
  private onOverlayClosed(): void {
    const closed = this.activeOverlay;
    this.activeOverlay = null;
    // Close the history boundary before anything below can write to the store.
    // Scoped to `edit`: this is also the onClose for `ctx` and `unified`, whose
    // writes land after the menu is gone and emit directly instead.
    const snap = this.editSnapshot;
    this.editSnapshot = null;
    if (snap && closed?.kind === 'edit' && !this.tearingDown) {
      const after = this.store.getValues()[snap.key];
      if (!chipValuesEqual(snap.before, after)) {
        this.onFieldEdit(snap.key, snap.before, after);
      }
    }
    this.container?.querySelectorAll('.chip.on, .chip-unified-btn.on')
      .forEach(e => e.classList.remove('on'));
    // Closing a provisional chip's edit popover with nothing committed IS
    // the dismissal (Escape, ×, click-away, chip re-click all land here):
    // drop the ghost — no write ever happened, the file never changed.
    let dismissedProvisional = false;
    if (
      this.provisional && closed?.kind === 'edit' && closed.key === this.provisional.key
      && !this.retainProvisionalOnClose
    ) {
      this.promotedKeys = this.promotedKeys.filter(k => k !== this.provisional!.key);
      this.provisional = null;
      dismissedProvisional = true;
    }
    this.retainProvisionalOnClose = false;
    // A locale change while the popover was open was deferred — do the full
    // relabel now (labels + chip labels/tooltips), which subsumes a value refresh.
    if (this.localeRefreshPending) {
      this.localeRefreshPending = false;
      this.pendingRefresh = false;
      if (this.container) this.render(this.container);
    } else if (this.pendingRefresh || dismissedProvisional) {
      this.pendingRefresh = false;
      if (this.container) this.renderChips();
    }
  }

  private showEdit(anchor: HTMLElement, key: string, def: FieldDefinition | null, widget: string): void {
    // The history boundary opens here and closes in onOverlayClosed. A
    // provisional ghost has written nothing yet, so its `before` is undefined —
    // which is what makes undoing the first commit a REMOVE rather than a write
    // back to a value that never existed.
    this.editSnapshot = { key, before: this.store.getValues()[key] };
    const handle = openOverlay({
      className: 'pop',
      position: { kind: 'below', anchor },
      onClose: () => this.onOverlayClosed(),
      // The system file dialog blurs this webview by existing; that blur is
      // the Browse button working, not the user leaving. Outside clicks still
      // dismiss throughout (shared/ui/dismiss-outside.ts, `suppressBlur`).
      suppressBlur: () => this.assetDialogOpen,
      build: (root) => {
        // Header
        const hdr = document.createElement('div');
        hdr.className = 'pop-hdr';
        const tit = document.createElement('span');
        tit.className = 'pop-title';
        tit.textContent = chipLabel(key);
        hdr.appendChild(tit);
        const x = document.createElement('span');
        x.className = 'pop-x';
        x.textContent = '\u00d7';
        x.addEventListener('click', () => this.closePop());
        hdr.appendChild(x);
        root.appendChild(hdr);

        let value = this.store.getValues()[key];
        // A provisional date opens with today prefilled as the picker's
        // SUGGESTION — committed on accept (change/Enter) only, never
        // written by merely opening the popover.
        if (this.provisional?.key === key && value === undefined && widget === 'date-picker') {
          value = this.defaultValueForWidget('date-picker', def);
        }

        if (widget === 'union') {
          this.buildUnionEditor(root, key, def, value);
        } else if (widget === 'tag-input') {
          this.buildTagEditor(root, key, toNameList(value), def);
        } else if (widget === 'file-picker') {
          this.buildFilePickerEditor(root, key, value, def);
        } else {
          this.buildSimpleEditor(root, key, widget, def, value);
        }
      },
    });
    this.activeOverlay = { kind: 'edit', key, handle };

    // Announce which field the user just opened, so the preview can point at
    // what it controls — the other direction of the gesture where a preview
    // click points at this chip. A DOM event, not a Tauri emit: the chip bar
    // knows nothing about the shell, and editor-main.ts already owns that
    // relay.
    document.dispatchEvent(
      new CustomEvent('editor:chip-opened', { detail: { field: key }, bubbles: true }),
    );

    requestAnimationFrame(() => {
      // A union field's primary control is its segmented choice; the folder
      // input only appears after "Folder…". Focusing that input at open would
      // pop the folder datalist over the choice — which is why the Child Pages
      // chip read as folder-search-only. Focus the selected segment instead.
      const inp = handle.el.querySelector(widget === 'union' ? '.pop-union-seg[tabindex="0"]' : 'input, textarea, select') as HTMLElement;
      inp?.focus();
    });
  }

  // Build the input/select/textarea for non-tag widgets via the WIDGET_EDITORS
  // table. Each entry describes how to construct the element, which event
  // commits, and how to coerce the raw string back to a YAML scalar.
  private buildSimpleEditor(
    p: HTMLElement,
    key: string,
    widget: string,
    def: FieldDefinition | null,
    value: unknown,
  ): void {
    const spec = WIDGET_EDITORS[widget] ?? WIDGET_EDITORS['text-input'];
    const el = spec.build(def, value);
    el.className = 'pop-input';
    el.addEventListener(spec.event, () => {
      const parsed = spec.parse((el as HTMLInputElement).value);
      // select and date-picker fire 'change' (discrete gesture) → immediate write.
      // text-input and text-area fire 'input' (per-character) → debounced write.
      // number-input fires 'input' → debounced (same as text).
      const immediate = spec.event === 'change';
      // commitField owns the transactional semantics: a null parse (cleared
      // select/date/number) removes the key; the first non-null commit
      // materializes a provisional chip.
      this.commitField(key, parsed, { immediate });
      this.repaintChipValue(key);
    });
    (el as HTMLElement).addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { this.closePop(); return; }
      if (e.key !== 'Enter') return;
      if (spec.closeOnEnter) { this.closePop(); return; }
      // Enter in a textarea types a newline — never a commit gesture.
      if (el instanceof HTMLTextAreaElement) return;
      // Enter accepts what the widget currently shows. For a provisional
      // chip this is the commit that materializes it — e.g. the date
      // picker's prefilled "today" suggestion, which 'change' alone would
      // never fire for. (A no-op re-commit on an existing chip is absorbed
      // by the store's equality short-circuit.)
      const parsed = spec.parse((el as HTMLInputElement).value);
      if (parsed !== null) this.commitField(key, parsed, { immediate: true });
      this.closePop();
    });
    p.appendChild(el);
  }

  // Union editor: a three-way segmented choice [On] [Off] [Folder…]. The
  // value's type picks the segment: true → On, false → Off, string/array →
  // the third (wikilink picker / list picker), which is revealed only while
  // that segment is selected. On and Off write `true`/`false` and so replace
  // any folder. Picking the third segment writes nothing until a folder is
  // typed; clearing the input restores the boolean `true`, never "".
  private buildUnionEditor(
    p: HTMLElement,
    key: string,
    def: FieldDefinition | null,
    value: unknown,
  ): void {
    const members = (def as any)?.one_of as Array<{ type: string; widget?: string }> | undefined ?? [];
    const hasWikilink = members.some(m => m.widget === 'wikilink-picker');
    const hasList = members.some(m => m.widget === 'wikilink-list-picker');
    const hasCustom = hasWikilink || hasList;

    const v0 = value === undefined ? this.defaultValueForWidget('union', def) : value;
    type Mode = 'on' | 'off' | 'custom';
    let mode: Mode = unionMode(v0);
    const folderValue = typeof value === 'string' ? value : '';

    const row = document.createElement('div');
    row.className = 'pop-union-row';
    row.setAttribute('role', 'radiogroup');
    row.setAttribute('aria-label', chipLabel(key));
    p.appendChild(row);

    const choices: Array<[Mode, string]> = [['on', t('chip.ui.on')], ['off', t('chip.ui.off')]];
    if (hasCustom) choices.push(['custom', t(hasWikilink ? 'chip.ui.union_folder' : 'chip.ui.union_list')]);
    const segs = new Map<Mode, HTMLButtonElement>();
    for (const [m, label] of choices) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'pop-union-seg';
      b.setAttribute('role', 'radio');
      b.textContent = label;
      segs.set(m, b);
      row.appendChild(b);
    }

    const pickerWrap = document.createElement('div');
    pickerWrap.className = 'pop-union-picker';
    p.appendChild(pickerWrap);

    let folderInput: HTMLInputElement | null = null;
    let listContainer: HTMLDivElement | null = null;
    // Empties the list branch's local entries; set below when there is one.
    let clearList = () => {};

    if (hasWikilink) {
      const inp = document.createElement('input');
      inp.type = 'text';
      inp.className = 'pop-input pop-union-folder';
      inp.placeholder = t('chip.ui.folder_placeholder');
      inp.value = stripWikilinkBrackets(folderValue); // strip [[ ]] for display
      // Folder/path picker is structured — full suppression.
      applyCorrectionPolicy(inp, false);
      folderInput = inp;

      const suggestions = document.createElement('datalist');
      suggestions.id = `union-folders-${key}`;
      inp.setAttribute('list', suggestions.id);
      pickerWrap.appendChild(inp);
      pickerWrap.appendChild(suggestions);

      // Load folder autocomplete from the project
      const projectPath = this.getProjectRoot();
      if (projectPath) {
        void engine.listSectionFolders().then((result) => {
          if (result.status !== 'ok') return;
          result.data.forEach(name => {
            const opt = document.createElement('option');
            opt.value = name;
            suggestions.appendChild(opt);
          });
        });
      }

      inp.addEventListener('input', () => {
        const raw = inp.value.trim();
        if (!raw) {
          // Clearing the field reverts to on-self (true) through this custom handler.
          this.commitField(key, true, { immediate: true });
        } else {
          this.commitField(key, wrapWikilink(raw));
        }
        this.repaintChipValue(key);
      });
      inp.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') this.closePop();
      });
    }

    if (hasList) {
      // WikilinkListPicker: reuse tag editor style for the list branch.
      const lc = document.createElement('div');
      lc.className = 'pop-union-list';
      pickerWrap.appendChild(lc);
      listContainer = lc;
      // Build a simple tag editor for the string array
      const arr: string[] = Array.isArray(value) ? [...value] as string[] : [];
      // focusInput: true only for author-driven re-renders (added/removed an
      // entry), never the first paint — see the union autofocus note in showEdit.
      const renderList = (focusInput: boolean) => {
        lc.innerHTML = '';
        arr.forEach((item, i) => {
          const tag = document.createElement('span');
          tag.className = 'pop-tag';
          tag.textContent = item;
          const rx = document.createElement('span');
          rx.className = 'pop-tag-x';
          rx.textContent = '×';
          rx.addEventListener('click', () => {
            arr.splice(i, 1);
            this.commitField(key, arr.length ? [...arr] : true, { immediate: true });
            this.repaintChipValue(key);
            // An empty list is `true`, so the segment must follow the pill.
            if (!arr.length) { mode = 'on'; paint(); }
            renderList(true);
          });
          tag.appendChild(rx);
          lc.appendChild(tag);
        });
        const inp2 = document.createElement('input');
        inp2.type = 'text';
        inp2.className = 'pop-tag-input';
        inp2.placeholder = '[[Folder]]';
        // Wikilink/path inputs are structured — full suppression.
        applyCorrectionPolicy(inp2, false);
        lc.appendChild(inp2);
        inp2.addEventListener('keydown', (e) => {
          if (e.key === 'Enter') {
            const v2 = inp2.value.trim();
            if (v2) {
              arr.push(wrapWikilink(v2));
              this.commitField(key, [...arr]);
              this.repaintChipValue(key);
              renderList(true);
            }
          }
        });
        if (focusInput) setTimeout(() => inp2.focus(), 0);
      };
      renderList(false);
      clearList = () => { arr.length = 0; renderList(false); };
    }

    const paint = () => {
      for (const [m, b] of segs) {
        const sel = m === mode;
        b.classList.toggle('is-on', sel);
        b.setAttribute('aria-checked', String(sel));
        b.tabIndex = sel ? 0 : -1;
      }
      pickerWrap.classList.toggle('is-hidden', mode !== 'custom');
    };
    paint();

    const choose = (m: Mode, focusSeg: boolean) => {
      mode = m;
      paint();
      if (focusSeg) segs.get(m)?.focus();
      if (m === 'custom') {
        // Revealed, not written: the value changes when a folder is named.
        (folderInput ?? listContainer?.querySelector('input'))?.focus();
        return;
      }
      this.commitField(key, m === 'on', { immediate: true });
      if (folderInput) folderInput.value = '';
      clearList(); // else a later List… would append to entries no longer stored
      this.repaintChipValue(key);
    };

    const order = choices.map(([m]) => m);
    for (const [m, b] of segs) {
      b.addEventListener('click', () => choose(m, false));
      b.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') { this.closePop(); return; }
        const i = order.indexOf(m);
        const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1
          : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
        const next = e.key === 'Home' ? 0 : e.key === 'End' ? order.length - 1
          : step ? (i + step + order.length) % order.length : -1;
        if (next < 0) return;
        e.preventDefault();
        // Arrows move and select, as radios do; the segment keeps focus.
        choose(order[next], true);
        segs.get(order[next])?.focus();
      });
    }
  }

  // Tag editor has its own DOM shape (pop-tags container + input) and
  // multi-key handling (Enter to add, Backspace on empty to pop), so it
  // sits outside the WIDGET_EDITORS table.
  //
  // `names` is already normalized by the caller; every write back goes
  // through `writeNames`, which denormalizes again — but only a `oneof`
  // field (author/editor/jury: string-or-list upstream) collapses a
  // one-name list to a bare string. `tags` is a plain array field; the
  // build rejects a bare string there ("expected array"), so it must
  // always write a list, even at one entry.
  private buildTagEditor(p: HTMLElement, key: string, names: string[], def: FieldDefinition | null): void {
    const td = document.createElement('div');
    td.className = 'pop-tags';
    const tagArr = [...names];
    const isOneOf = def?.type === 'oneof';
    // Every edit here is a discrete gesture → immediate write.
    const writeNames = () => {
      this.commitField(key, isOneOf ? fromNameList(tagArr) : [...tagArr], { immediate: true });
      this.repaintChipValue(key);
    };

    const renderTags = () => {
      td.innerHTML = '';
      tagArr.forEach((t, i) => {
        const tag = document.createElement('span');
        tag.className = 'pop-tag';
        tag.textContent = t;
        tag.addEventListener('click', (e) => {
          this.followTermChip(e, key, t);
        });
        const rx = document.createElement('span');
        rx.className = 'pop-tag-x';
        rx.textContent = '\u00d7';
        rx.addEventListener('click', (e) => {
          e.stopPropagation();
          tagArr.splice(i, 1);
          writeNames();
          renderTags();
        });
        tag.appendChild(rx);
        td.appendChild(tag);
      });
    };
    renderTags();
    p.appendChild(td);

    const inp = document.createElement('input');
    inp.className = 'pop-input';
    inp.placeholder = t('chip.ui.tag_input_hint');
    // Tags are identifier/structured — full suppression.
    applyCorrectionPolicy(inp, false);
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && inp.value.trim()) {
        e.preventDefault();
        tagArr.push(inp.value.trim());
        inp.value = '';
        writeNames();
        renderTags();
      }
      if (e.key === 'Backspace' && !inp.value && tagArr.length) {
        tagArr.pop();
        writeNames();
        renderTags();
      }
      if (e.key === 'Escape') this.closePop();
    });
    attachTermSuggestions(inp, key, p);
    p.appendChild(inp);
  }

  // File-picker editor (cover): a plain text input + a live search dropdown
  // of matching assets. Backed by the SAME Rust ranker the CM6 `![[asset]]`
  // wikilink completion uses (`engine.wikilinkCompletions`, embed:true so
  // assets rank first) — no parallel search implementation. Sits outside the
  // WIDGET_EDITORS table, like tag-input and union: it needs async data plus
  // custom keyboard handling that doesn't fit the table's synchronous
  // build()+event+parse triple.
  //
  // Round-trip: the stored value may be a bare path or an Obsidian wikilink
  // (`cover: '[[DSCF4053.jpeg]]'` — see wikilink-syntax.ts). The input shows
  // the UNWRAPPED target, matching the chip's read-only display
  // (renderChipValue's file-picker branch); on commit the value is re-wrapped
  // via wrapWikilink so the stored format doesn't change. `def.file_kinds`
  // (schema SSOT — cover: image/video/iframe, logo: image only) is passed to the
  // RPC as `allowed_kinds` so the backend ranker itself drops pages and
  // non-matching-extension assets; no client-side kind filter needed.
  private buildFilePickerEditor(
    p: HTMLElement,
    key: string,
    value: unknown,
    def: FieldDefinition | null,
  ): void {
    const fileKinds = def?.file_kinds ?? null;
    const listboxId = `file-picker-listbox-${Math.random().toString(36).slice(2)}`;

    const inp = document.createElement('input');
    inp.type = 'text';
    inp.className = 'pop-input';
    inp.value = wikilinkTarget(String(value ?? ''));
    inp.setAttribute('role', 'combobox');
    inp.setAttribute('aria-expanded', 'true');
    inp.setAttribute('aria-autocomplete', 'list');
    inp.setAttribute('aria-controls', listboxId);
    // Asset filename — identifier field, full correction suppression.
    applyCorrectionPolicy(inp, false);
    // The input and its Browse button are one object: a relatively-positioned
    // field box so the button can sit inset on the trailing edge.
    const field = document.createElement('div');
    field.className = 'pop-field';
    field.appendChild(inp);
    p.appendChild(field);

    const dropdown = document.createElement('div');
    dropdown.className = 'pop-file-picker-list';
    dropdown.setAttribute('role', 'listbox');
    dropdown.id = listboxId;
    p.appendChild(dropdown);

    let items: WikilinkCompletion[] = [];
    let highlighted = 0;
    // Guards against a slow, now-superseded RPC response overwriting a newer one.
    const requestId = createGenerationToken();
    // Debounce timer for the per-keystroke search RPC. `wikilinkCompletions`
    // does a full recursive project-tree walk server-side with no cache — the
    // requestId guard above only stops a stale result from being SHOWN, it
    // doesn't stop the wasted walk. Debouncing avoids firing one per
    // keystroke, matching how CM6's own `![[...]]` autocomplete gets an
    // implicit typing delay from `autocompletion()`.
    let searchTimer: ReturnType<typeof setTimeout> | null = null;

    const clearSearchTimer = (): void => {
      if (searchTimer) {
        clearTimeout(searchTimer);
        searchTimer = null;
      }
    };

    // Commit whatever is currently typed, verbatim — the per-keystroke path
    // (popover stays open, so the chip behind it is live-repainted, mirroring
    // text-input's same behavior). Empty input removes the key (via
    // commitField's null route) rather than leaving `cover: ""` residue —
    // an empty cover path is one of the silent-damage cases the
    // transactional-add decision names.
    const commitRaw = (immediate: boolean): void => {
      const raw = inp.value.trim();
      this.commitField(key, raw ? wrapWikilink(raw) : null, { immediate });
      this.repaintChipValue(key);
    };

    // Commit a chosen dropdown item — a discrete gesture (mouse click or
    // Enter-on-highlighted), so it writes immediately and closes the popover.
    // No interim repaintChipValue call here: closePop() synchronously
    // triggers onOverlayClosed's full renderChips() (the store's subscriber
    // already flagged pendingRefresh from setField, above), so a targeted
    // repaint here would just be overwritten before the next paint — same
    // reasoning as the add-property dropdown's row-click (chip-bar.ts, "Store
    // subscriber fires renderChips() after closePop clears activeOverlay").
    const selectItem = (item: WikilinkCompletion): void => {
      clearSearchTimer();
      // `insert` is the reference form the backend chose for THIS file — path-
      // qualified whenever the author typed a path or picked a file, never a
      // bare stem then. Frontmatter wikilinks are resolved: `resolve_frontmatter_wikilinks` calls
      // `graph.resolve_path(ref, source)` on every one of them, and that
      // function's filename branch is stem-keyed with tiebreaks that always
      // return *some* answer and never report ambiguity. So a bare `cover.jpg`
      // silently re-points the day a same-named file appears elsewhere in the
      // vault, so this surface writes the qualified form, never the bare one.
      const ref = item.insert;
      inp.value = ref;
      this.commitField(key, wrapWikilink(ref), { immediate: true });
      this.closePop();
    };

    // Move the highlight to `index` IN PLACE — toggle the .is-highlighted
    // class + aria-selected on the EXISTING row nodes and re-point
    // aria-activedescendant, without rebuilding the list. Mouse hover and
    // keyboard nav (ArrowUp/Down) both funnel through here so they share one
    // code path. The old hover handler called renderDropdown() (which starts
    // with `dropdown.innerHTML = ''`), destroying the row under the cursor
    // between mousedown and mouseup — so the real WebKit webview never
    // synthesized a `click` on it and the dropdown was unusable by mouse.
    const applyHighlight = (index: number): void => {
      highlighted = index;
      const rows = dropdown.querySelectorAll<HTMLElement>('.pop-file-picker-item');
      rows.forEach((row, i) => {
        const on = i === index;
        row.classList.toggle('is-highlighted', on);
        row.setAttribute('aria-selected', String(on));
      });
      inp.setAttribute('aria-activedescendant', `${listboxId}-item-${index}`);
    };

    const renderDropdown = (): void => {
      dropdown.innerHTML = '';
      if (items.length === 0) {
        const empty = document.createElement('div');
        empty.className = 'pop-file-picker-empty';
        empty.textContent = t('chip.ui.file_picker_no_matches');
        dropdown.appendChild(empty);
        inp.removeAttribute('aria-activedescendant');
        return;
      }
      items.forEach((item, i) => {
        const row = document.createElement('div');
        row.className = 'pop-file-picker-item' + (i === highlighted ? ' is-highlighted' : '');
        row.id = `${listboxId}-item-${i}`;
        row.setAttribute('role', 'option');
        row.setAttribute('aria-selected', String(i === highlighted));
        const label = document.createElement('span');
        label.className = 'pop-file-picker-label';
        label.textContent = item.label;
        row.appendChild(label);
        if (item.detail) {
          const detail = document.createElement('span');
          detail.className = 'pop-file-picker-detail';
          detail.textContent = item.detail;
          row.appendChild(detail);
        }
        // Hover moves the highlight in place — never rebuilds the list (see
        // applyHighlight above for why rebuilding here broke mouse selection).
        row.addEventListener('mouseenter', () => applyHighlight(i));
        // Commit on mousedown + preventDefault, NOT click — mirrors the working
        // pattern in cm-slash-menu.ts. preventDefault keeps focus on the input
        // (so keyboard nav survives a hover) AND the selection lands before any
        // node-swap can lose the gesture. On the real WebKit webview a bare
        // `click` is never synthesized here because the row can be replaced
        // mid-press.
        row.addEventListener('mousedown', (e) => {
          e.preventDefault();
          selectItem(item);
        });
        dropdown.appendChild(row);
      });
      inp.setAttribute('aria-activedescendant', `${listboxId}-item-${highlighted}`);
    };

    const runSearch = async (query: string): Promise<void> => {
      const myRequest = requestId.bump();
      const filePath = this.getActivePath();
      let result: WikilinkCompletion[] = [];
      try {
        const res = await engine.wikilinkCompletions(query, filePath, 'embed', null, fileKinds);
        if (res.status === 'ok') result = res.data;
      } catch {
        result = [];
      }
      if (requestId.isStale(myRequest)) return; // superseded by a newer keystroke
      items = result;
      highlighted = 0;
      renderDropdown();
    };

    inp.addEventListener('input', () => {
      commitRaw(false);
      clearSearchTimer();
      const query = inp.value.trim();
      searchTimer = setTimeout(() => {
        searchTimer = null;
        void runSearch(query);
      }, SEARCH_DEBOUNCE_MS);
    });

    inp.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (items.length === 0) return;
        applyHighlight((highlighted + 1) % items.length);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        if (items.length === 0) return;
        applyHighlight((highlighted - 1 + items.length) % items.length);
      } else if (e.key === 'Enter') {
        if (e.isComposing) return; // IME composition commit, not a real Enter
        e.preventDefault();
        if (items.length > 0) {
          selectItem(items[highlighted]);
        } else {
          // No dropdown match: commit the raw typed text and close — same
          // outcome as commitRaw(true), but skips its interim repaint since
          // closePop()'s full renderChips() supersedes it immediately (see
          // selectItem's comment above).
          clearSearchTimer();
          const raw = inp.value.trim();
          this.commitField(key, raw ? wrapWikilink(raw) : null, { immediate: true });
          this.closePop();
        }
      } else if (e.key === 'Escape') {
        clearSearchTimer();
        this.closePop();
      }
    });

    // The escape hatch: pick the file from disk instead of naming it.
    // `--has-browse` reserves the room;
    // `.pop-input` is shared by four editors that must not gain the padding.
    if (canBrowse()) {
      inp.classList.add('pop-input--has-browse');
      field.appendChild(browseButton({
        variant: 'inset',
        ctx: {
          pagePath: () => this.getActivePath(),
          projectRoot: () => this.getProjectRoot() ?? null,
          kinds: () => fileKinds,
          afterImport: () => this.afterVaultImport(),
        },
        // Stand the search down for the length of the dialog — see `onOpen`.
        onOpen: () => {
          clearSearchTimer();
          requestId.bump();
          this.assetDialogOpen = true;
        },
        onPicked: (item) => {
          this.assetDialogOpen = false;
          selectItem(item);
        },
        onSettled: () => { this.assetDialogOpen = false; },
      }));
    }

    // Browse-on-open: search with whatever value is already there so the
    // dropdown shows matches immediately, before the first keystroke.
    void runSearch(inp.value);
  }

  // ── Context Menu (Copy value / Reset to default / Remove field) ─────

  private showChipCtx(e: MouseEvent, key: string): void {
    // A provisional ghost gets a one-item menu: "Remove field" = dismiss.
    // Closing the ghost's own edit popover to make room for the menu must
    // not count as the user dismissing it (retainProvisionalOnClose).
    const isGhost = this.provisional?.key === key;
    if (isGhost) this.retainProvisionalOnClose = true;
    this.closePop();

    const def = this.schema.frontmatter.fields[key] ?? null;

    const utilityGroup: CtxMenuEntry[] = [];
    if (!isGhost) {
      utilityGroup.push({
        label: t('chip.ui.copy_value'),
        action: () => {
          void navigator.clipboard?.writeText(serializeChipValue(this.store.getValues()[key]))
            .catch((err) => console.error('[chip-bar] copy value failed:', err));
        },
      });
      // Only fields whose SCHEMA declares a default can be reset to one — an
      // invented per-widget fallback would silently rewrite user data.
      if (def?.default != null) {
        utilityGroup.push({
          label: t('chip.ui.reset_default'),
          // A discrete gesture → immediate write, same as toggles.
          action: () => { this.commitField(key, def.default, { immediate: true }); },
        });
      }
    }

    // Shared context-menu primitive: Escape + outside-click dismissal, menu
    // ARIA roles, roving focus. The chip stays an outside-click "inside" so
    // right-clicking the same chip while the menu is open doesn't fight with
    // the contextmenu handler's own re-open call.
    const outsideAnchor = e.currentTarget as HTMLElement;
    const handle = openCtxMenu({
      entries: joinCtxGroups([
        utilityGroup,
        [{
          label: t('chip.ui.remove_field'),
          danger: true,
          // Ghost: dismissal, not removal — the key was never written.
          // Real chip: removeField notifies store subscribers → renderChips().
          // `commitField(key, null)` IS removal — and it is what puts the most
          // destructive chip gesture in the combined history. Removing a field
          // by hand and being unable to undo it was the one case where ⌘Z's
          // silence cost real data.
          action: () => {
            if (this.provisional?.key === key) this.dismissProvisional();
            else this.commitField(key, null);
          },
        }],
      ]),
      position: { kind: 'point', x: e.clientX, y: e.clientY },
      outsideAnchor,
      onClose: () => {
        this.onOverlayClosed();
        // The menu closed without materializing or dismissing the ghost
        // (Escape / outside click): it no longer has a popover to own its
        // dismissal, so arm the document-level click-away/Escape listeners
        // the checkbox ghost already uses.
        if (this.provisional?.key === key) {
          const ghost = this.chipEls.find(c => c.field.key === key);
          if (ghost) this.armProvisionalDismiss(ghost.el);
        }
      },
      logSurface: 'chip-bar',
      logTarget: `field:${key}`,
    });
    this.activeOverlay = { kind: 'ctx', key, handle };
  }

  // ── Unified Dropdown ────────────────────────────────────────────────

  /**
   * The "+" menu: hidden-but-set fields on top, then a search box over the
   * grouped list of fields you can add.
   *
   * Focus model — cm-slash-menu.ts's, not ctx-menu.ts's, and the difference
   * matters. DOM focus STAYS in the search box so typing keeps filtering; an
   * `activeIdx` moves a highlight class over the add list, announced to
   * assistive tech with `aria-activedescendant`. Roving DOM focus (what
   * ctx-menu.ts does, correctly, for a menu with no text input) put focus on
   * a `<div>` after one ArrowDown and typing stopped working — and every
   * keystroke re-renders the list, which destroyed the focused row and
   * dropped focus to `<body>`.
   *
   * ARIA shape follows from that: a combobox owning a listbox, NOT
   * `role="menu"` — a menu's children must be menuitem/group/separator and
   * this surface's first child is a textbox. When there is nothing to add at
   * all (every schema field already set) there is no search box either, and
   * the whole overlay is simply the listbox of already-set fields.
   *
   * The keydown listener is on `document` in the CAPTURE phase, like
   * ctx-menu.ts's, for two reasons: a root-scoped listener receives nothing
   * when nothing inside the overlay holds focus, and Escape has to be
   * stopped from reaching the document-level listener `armProvisionalDismiss`
   * arms — otherwise dismissing this menu also silently discards the
   * provisional chip the user was in the middle of adding.
   */
  private showUnifiedDropdown(anchor: HTMLElement): void {
    // Rows the active index ranges over, and the element carrying
    // `aria-activedescendant` — filled in by build() below.
    let navRows: HTMLElement[] = [];
    let navHost: HTMLElement | null = null;
    let activeIdx = 0;
    let restoreFocus = false;
    let detachKeys: (() => void) | null = null;

    const paintActive = (): void => {
      navRows.forEach((r, i) => r.classList.toggle('orow-active', i === activeIdx));
      const active = navRows[activeIdx];
      if (navHost) {
        if (active) navHost.setAttribute('aria-activedescendant', active.id);
        else navHost.removeAttribute('aria-activedescendant');
      }
      // jsdom has no scrollIntoView; the menu scrolls in a real engine only.
      active?.scrollIntoView?.({ block: 'nearest' });
    };
    const moveActive = (delta: number): void => {
      if (navRows.length === 0) return;
      activeIdx = Math.min(Math.max(activeIdx + delta, 0), navRows.length - 1);
      paintActive();
    };

    const handle = openOverlay({
      className: 'omenu',
      position: { kind: 'below', anchor },
      onClose: () => {
        detachKeys?.();
        detachKeys = null;
        // Focus goes back to the "+" button, but only for the keyboard
        // dismissals (Escape/Tab). An outside click or a webview blur has
        // already moved the user somewhere deliberate; pulling focus back
        // would yank them out of it.
        // `anchor` IS the invoking control (the "+" button) — no need to
        // guess from document.activeElement the way ctx-menu.ts must, since
        // that menu can be opened at a bare point with no anchor at all.
        if (restoreFocus && anchor.isConnected) anchor.focus();
        this.onOverlayClosed();
      },
      build: (root) => {
        const unset = this.getUnsetSchemaFields();
        const hasAddList = unset.length > 0;
        const hiddenRows: HTMLElement[] = [];

        // Section 1: Hidden set properties (only when overflow exists)
        if (this.hiddenFields.length > 0) {
          const hdr = document.createElement('div');
          hdr.className = 'omenu-group';
          hdr.textContent = t('chip.ui.set_on_this_page');
          root.appendChild(hdr);

          // With an add list below, these rows get their own listbox so the
          // search box can own BOTH (aria-controls takes a list of ids) and
          // the highlight can walk the menu top to bottom in one run.
          // Without one, the overlay itself is the single listbox — see the
          // root role further down.
          const setBox = hasAddList ? document.createElement('div') : root;
          if (setBox !== root) {
            setBox.id = OMENU_SETBOX_ID;
            setBox.setAttribute('role', 'listbox');
            root.appendChild(setBox);
          }

          for (const f of this.hiddenFields) {
            const widget = f.def?.widget ?? this.inferWidget(f.value);
            const row = document.createElement('div');
            row.className = 'orow';
            row.id = `omenu-row-${MENU_ROW_SEQ++}`;
            row.setAttribute('role', 'option');
            const label = document.createElement('span');
            label.className = 'ok';
            label.textContent = chipLabel(f.key);
            row.appendChild(label);
            const val = document.createElement('span');
            val.className = 'ov';
            this.renderOverflowValue(val, widget, f.value);
            row.appendChild(val);
            row.addEventListener('click', () => this.promoteAndEdit(f, widget));
            row.addEventListener('mouseenter', () => {
              activeIdx = navRows.indexOf(row);
              paintActive();
            });
            setBox.appendChild(row);
            hiddenRows.push(row);
          }

          const sep = document.createElement('div');
          sep.className = 'osep';
          sep.setAttribute('role', 'separator');
          root.appendChild(sep);
        }

        // Section 2: Search + grouped add list
        let search: HTMLInputElement | null = null;
        if (hasAddList) {
          search = document.createElement('input');
          search.className = 'omenu-search';
          search.placeholder = t('chip.ui.search_properties');
          search.setAttribute('role', 'combobox');
          search.setAttribute('aria-expanded', 'true');
          search.setAttribute('aria-autocomplete', 'list');
          search.setAttribute(
            'aria-controls',
            hiddenRows.length > 0 ? `${OMENU_SETBOX_ID} ${OMENU_LISTBOX_ID}` : OMENU_LISTBOX_ID,
          );
          search.setAttribute('aria-label', t('chip.ui.search_properties'));
          // Search box is an identifier field (property name lookup) — full suppression.
          applyCorrectionPolicy(search, false);
          root.appendChild(search);

          const listContainer = document.createElement('div');
          listContainer.id = OMENU_LISTBOX_ID;
          listContainer.setAttribute('role', 'listbox');
          root.appendChild(listContainer);

          const renderGroupedList = (filter: string) => {
            listContainer.innerHTML = '';
            const addRows: HTMLElement[] = [];
            const grouped = this.groupFields(unset);
            const lc = filter.toLowerCase();

            const orderedGroups = sortGroups([...grouped.keys()]);
            for (const groupName of orderedGroups) {
              const fields = grouped.get(groupName);
              if (!fields) continue;
              const filtered = lc ? fields.filter(f =>
                chipLabel(f.key).toLowerCase().includes(lc) ||
                f.key.toLowerCase().includes(lc)
              ) : fields;
              if (filtered.length === 0) continue;

              const ghdr = document.createElement('div');
              ghdr.className = 'omenu-group';
              ghdr.textContent = GROUP_KEY_MAP[groupName] ? t(GROUP_KEY_MAP[groupName]) : groupName;
              ghdr.setAttribute('role', 'presentation');
              listContainer.appendChild(ghdr);

              for (const f of filtered) {
                const widget = f.def?.widget ?? 'text-input';
                const row = document.createElement('div');
                row.className = 'orow orow-add';
                row.id = `omenu-row-${MENU_ROW_SEQ++}`;
                row.setAttribute('role', 'option');
                const head = document.createElement('div');
                head.className = 'orow-add-head';
                const plus = document.createElement('span');
                plus.className = 'oadd-plus';
                plus.textContent = '+';
                head.appendChild(plus);
                const label = document.createElement('span');
                label.className = 'orow-add-label';
                label.textContent = chipLabel(f.key);
                head.appendChild(label);
                const hint = autoHint(this.pageFacts.auto, f.key);
                if (hint) head.appendChild(hint);
                row.appendChild(head);
                // Second line, absent for plugin fields with no chip.<key>.detail.
                // The set rows above show the VALUE instead.
                const detail = chipDetail(f.key);
                if (detail) {
                  const detailEl = document.createElement('span');
                  detailEl.className = 'orow-add-detail';
                  detailEl.textContent = detail;
                  row.appendChild(detailEl);
                }
                // Adding writes nothing: the provisional chip's first commit is the
                // first disk write (transactional add, design-vocabulary.md).
                row.addEventListener('click', () => this.beginProvisionalAdd(f, widget));
                // Mouse and keyboard must never highlight two rows at once.
                row.addEventListener('mouseenter', () => {
                  activeIdx = navRows.indexOf(row);
                  paintActive();
                });
                listContainer.appendChild(row);
                addRows.push(row);
              }
            }

            // The highlight runs over the whole menu in visual order —
            // already-set rows first, then the add list — so ArrowDown always
            // moves DOWN. (It used to read the rows in document order and
            // treat the search box as position -1, which on a page with
            // overflowing chips sent ArrowDown UPWARDS into the set rows.)
            navRows = [...hiddenRows, ...addRows];
            // Typing rebuilds the add list, so the previously active row is
            // gone. Land on the first MATCH rather than trying to preserve a
            // position in a set that just changed underneath it: the user is
            // typing to add something, and the first match is what they
            // narrowed towards.
            activeIdx = addRows.length > 0 ? hiddenRows.length : -1;
            paintActive();
          };

          renderGroupedList('');
          search.addEventListener('input', () => renderGroupedList(search!.value));
        } else if (hiddenRows.length > 0) {
          // No search box, so the overlay itself is the listbox and hosts
          // the active descendant. It also has to be focusable, or opening
          // the menu leaves focus outside it entirely.
          root.setAttribute('role', 'listbox');
          root.tabIndex = -1;
          navRows = hiddenRows;
          activeIdx = 0;
        }

        navHost = search ?? root;
        paintActive();
      },
    });
    this.activeOverlay = { kind: 'unified', handle };

    const onKeydown = (ev: KeyboardEvent): void => {
      switch (ev.key) {
        case 'Escape':
          ev.preventDefault();
          // Escape belongs to the menu and stops here. Without this the
          // document-level listener armed by armProvisionalDismiss also sees
          // it and throws away a provisional chip the user is still editing.
          ev.stopPropagation();
          restoreFocus = true;
          handle.close();
          return;
        case 'Tab':
          // Neither a menu nor a combobox popup holds tab stops: close and
          // let the default focus move proceed from the restored invoker.
          restoreFocus = true;
          handle.close();
          return;
        case 'ArrowDown':
          ev.preventDefault();
          ev.stopPropagation();
          moveActive(1);
          return;
        case 'ArrowUp':
          ev.preventDefault();
          ev.stopPropagation();
          moveActive(-1);
          return;
        case 'Home':
        case 'End':
          if (navRows.length === 0) return;
          ev.preventDefault();
          ev.stopPropagation();
          activeIdx = ev.key === 'Home' ? 0 : navRows.length - 1;
          paintActive();
          return;
        case 'Enter': {
          const row = navRows[activeIdx];
          if (!row) return;
          ev.preventDefault();
          ev.stopPropagation();
          row.click();
          return;
        }
      }
    };
    // Capture phase, on document: the menu's keys must work regardless of
    // what holds focus (a menu with no search box has nothing focusable
    // inside it at all), and must not leak to the surfaces below.
    document.addEventListener('keydown', onKeydown, true);
    detachKeys = () => document.removeEventListener('keydown', onKeydown, true);

    // Focus the search box, or the overlay itself when there isn't one.
    requestAnimationFrame(() => {
      const s = handle.el.querySelector('.omenu-search') as HTMLInputElement | null;
      (s ?? handle.el).focus();
    });
  }

  /**
   * Transactional add (design-vocabulary: "Property add is transactional;
   * absence is the canonical empty"). Clicking an add row writes nothing:
   * it renders a ghosted provisional chip at the front of the bar and opens
   * the field's editor. The first commit materializes the field — the first
   * disk write; dismissing removes the ghost and the file never changed.
   * Checkbox fields skip the popover: the ghost IS the toggle.
   */
  private beginProvisionalAdd(f: ChipField, widget: string, seed?: unknown): void {
    this.provisional = { key: f.key, def: f.def, widget, seed };
    this.pendingRefresh = false;
    this.promoteToFront(f.key);
    if (widget === 'checkbox') return;
    const ghost = this.chipEls.find(c => c.field.key === f.key);
    if (!ghost) return;
    ghost.el.classList.add('on');
    this.showEdit(ghost.el, f.key, f.def, widget);
  }

  // Promote a hidden field to the front of the chip bar, animate the other
  // chips sliding right to make room, and open the same edit popover the
  // user would get from clicking a chip directly. Used by the "more menu"
  // hidden-field rows.
  private promoteAndEdit(f: ChipField, widget: string): void {
    this.promoteToFront(f.key);

    // Open the editor on the promoted chip (now first in the bar). Look it up
    // through chipEls (direct refs) rather than a CSS selector so we don't need
    // to escape arbitrary YAML keys.
    const promoted = this.chipEls.find(c => c.field.key === f.key);
    if (promoted) {
      promoted.el.classList.add('on');
      this.showEdit(promoted.el, f.key, f.def, widget);
    }
  }

  // Move one field to the front of the chip strip and animate the rest sliding
  // right to make room. The ONE place chip order is changed on purpose — both
  // "I edited it from the more menu" and "the preview pointed at it" go
  // through here, so there is a single notion of which chips come first.
  private promoteToFront(key: string): void {
    // FLIP — record currently-visible chip positions so we can play them back
    // after the re-render. Hidden chips (display:none) report all-zeros from
    // getBoundingClientRect, so we skip them and let the (typically previously-
    // hidden) promoted chip just appear at the front.
    const oldPositions = new Map<string, number>();
    for (const { el, field } of this.chipEls) {
      if (el.style.display !== 'none') {
        oldPositions.set(field.key, el.getBoundingClientRect().left);
      }
    }
    this.promotedKeys = [key, ...this.promotedKeys.filter(k => k !== key)];
    // renderChips() calls closePop() — so the unified menu closes too.
    this.renderChips();
    this.flipFromPositions(oldPositions);
  }

  // Apply a FLIP (First-Last-Invert-Play) animation to chipEls given a map
  // of pre-render left-edge positions. For each chip whose key is in the map,
  // sets transform: translateX(oldLeft - newLeft) with no transition (the
  // "Invert" step), flushes a reflow, then transitions transform to 0 (the
  // "Play" step). Chips whose key is NOT in the map (e.g. the just-promoted
  // chip that was display:none before the re-render) are skipped — they
  // simply appear at their new position.
  private flipFromPositions(oldPositions: Map<string, number>): void {
    for (const { el, field } of this.chipEls) {
      const oldLeft = oldPositions.get(field.key);
      if (oldLeft === undefined) continue;
      const newLeft = el.getBoundingClientRect().left;
      const dx = oldLeft - newLeft;
      if (Math.abs(dx) < 1) continue;
      el.style.transition = 'none';
      el.style.transform = `translateX(${dx}px)`;
    }
    // One reflow flush commits the pre-transition state to the layout tree;
    // without this the browser may collapse the two style writes into one and
    // skip the animation. Reading offsetWidth is the standard force-layout
    // idiom — `void` is cosmetic, just signals "discard the value."
    if (this.container) void this.container.offsetWidth;
    for (const { el, field } of this.chipEls) {
      if (!oldPositions.has(field.key)) continue;
      el.style.transition = `transform ${PROMOTE_FLIP_MS}ms cubic-bezier(.4,0,.2,1)`;
      el.style.transform = 'translateX(0)';
      const cleanup = (e: TransitionEvent) => {
        if (e.propertyName !== 'transform') return;
        el.style.transition = '';
        el.style.transform = '';
        el.removeEventListener('transitionend', cleanup);
      };
      el.addEventListener('transitionend', cleanup);
    }
  }

  private renderOverflowValue(el: HTMLElement, widget: string, value: unknown): void {
    // Truncation is handled by CSS (.orow .ov text-overflow: ellipsis with
    // min-width:0 + .omenu max-width). Keep the rendering pure-text so the
    // browser computes the cut-off based on actual width.
    if (widget === 'tag-input') {
      const names = toNameList(value);
      el.textContent = names.length > 0 ? names.join(', ') : t('chip.ui.empty');
    } else if (widget === 'checkbox') {
      el.textContent = value ? '\u2713' : '\u2014';
    } else {
      // Legacy `key: null` / `key: ""` read as "Empty", never "null".
      el.textContent = value == null || value === ''
        ? t('chip.ui.empty')
        : String(value);
    }
  }

  // ── Add-field helpers ───────────────────────────────────────────────

  private getUnsetSchemaFields(): ChipField[] {
    const schemaFields = this.schema.frontmatter.fields;
    const currentValues = this.store.getValues();
    const result: ChipField[] = [];
    for (const [key, def] of Object.entries(schemaFields)) {
      // Only absent keys are addable (`key: null` renders an "Empty" chip), and only
      // those that can change this page.
      if (currentValues[key] === undefined && !suggestionFor(key, currentValues, this.suggestions)
        && appliesHere(def, this.pageFacts.position, currentValues)) {
        result.push({ key, def, value: null });
      }
    }
    // Lower score ("priority" on the wire) sorts first.
    result.sort((a, b) => (a.def?.priority || 100) - (b.def?.priority || 100));
    return result;
  }

  private groupFields(fields: ChipField[]): Map<string, ChipField[]> {
    const grouped = new Map<string, ChipField[]>();
    for (const f of fields) {
      const group = f.def?.group ?? 'Other';
      if (!grouped.has(group)) grouped.set(group, []);
      grouped.get(group)!.push(f);
    }
    return grouped;
  }

  private defaultValueForWidget(widget: string, def: FieldDefinition | null): unknown {
    if (def?.default != null) return def.default;
    switch (widget) {
      case 'checkbox': return false;
      case 'number-input': return 0;
      case 'tag-input': return [];
      case 'date-picker': {
        // Default to the user's LOCAL calendar day as YYYY-MM-DD. toISOString()
        // would give the UTC date, which is off by one for ahead-of-UTC zones in
        // the morning (e.g. China UTC+8) and behind-of-UTC zones at night.
        const now = new Date();
        const y = now.getFullYear();
        const m = String(now.getMonth() + 1).padStart(2, '0');
        const d = String(now.getDate()).padStart(2, '0');
        return `${y}-${m}-${d}`;
      }
      // A union's first member is its boolean branch; default to on-self.
      case 'union':
        return ((def as any)?.one_of as Array<{ type: string }> | undefined)?.[0]?.type === 'boolean';
      default: return '';
    }
  }

  // ── Helpers ─────────────────────────────────────────────────────────

  private inferWidget(value: unknown): string {
    if (typeof value === 'boolean') return 'checkbox';
    if (typeof value === 'number') return 'number-input';
    if (Array.isArray(value)) return 'tag-input';
    return 'text-input';
  }
}
