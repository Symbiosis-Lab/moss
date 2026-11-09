/**
 * Versions surface — the editor-body "room" for the publish-history panel.
 * Renders
 * all three screens: the list with the inline "+" save control and the empty
 * state; a page's version and a folder/site version, both with their restore flows.
 *
 * **Scope follows the click; there is no in-panel toggle.** `ctx.scope` /
 * `ctx.path` are fixed for the surface's whole life — a file opens 'page'
 * scope on that file, a subfolder opens 'folder' scope on that folder (its
 * own descendants only), the project root opens 'site' scope (`path` is
 * `PROJECT_ROOT_REL`, `''`).
 *
 * **The "+" replaces the always-open name field.** Tapping it morphs the
 * glyph into a name field with a confirm/cancel pair, transactional like
 * the frontmatter chip-bar's add flow — nothing is written until confirm,
 * and cancelling leaves nothing behind. Safe to reuse the bare "+" here
 * (moss's "one bare + per panel" rule, design-vocabulary.md): entering
 * this mode hides the tree's create-button "+" and the chip bar's
 * add-property "+" along with the rest of the file-editing chrome
 * (versions-mode.ts's `hideChrome()`), so this surface is the only "+"
 * on screen. **Save always snapshots the whole site, in every scope** —
 * a version is a picture of the whole site at one moment; scope only
 * filters which slice of the timeline this screen shows.
 *
 * **No day grouping, no "Current" row, no footer.** Every row carries its
 * own full date+time instead of a bare time under a "Today"/"Yesterday"
 * heading.
 *
 * Two backend gaps this frontend papers over — noted here rather than
 * silently worked around, because both need a real decision when the Rust
 * side of this lands (today `list_versions`/`restore_version` only ever saw
 * `scope: "page" | "site"`, and the mock in
 * `packages/moss-ui/src/ui/editor/__tests__/mock-data.ts` fakes both):
 *
 * - `restoreVersion`'s `path` already means "restore the entry at this path"
 *   for a page. Folder-scope restore (`openTreeDetail`) asks for the same
 *   parameter with a folder path, which only works if the backend widens
 *   the match from equality to "at or under this path" — a page path has no
 *   descendants, so this is backward compatible, not a new parameter.
 * - The folder/site detail screen's "Changed since / Unchanged" split has no
 *   such easy path: the existing drill-down (`listVersions('site',
 *   <record id>)`) already spends its one `path` slot on the record id, so
 *   there's no room left to also say which folder to filter to. This
 *   surface still always drills down with literal scope `'site'` (the
 *   existing, unextended contract) and filters the two returned arrays to
 *   `ctx.path`'s subtree client-side when `ctx.scope === 'folder'` — a
 *   filter over an answer Rust already computed, not a second diff. The
 *   real fix is a fourth `at` parameter on `list_versions`; this is the
 *   stopgap until that lands.
 *
 * One `root` element is reused across screens — a row click tears down the
 * current screen's DOM and builds the next one in place. Only screen one's
 * "‹ Editor" link and a completed restore leave the surface, through
 * `ctx.onExit`, which the editor webview's own `editor-main.ts` answers by
 * putting the source document back (Versions is a mode of the editor body,
 * not a panel view of its own). Going back from a detail screen re-renders
 * the list from the already cached `list`/`scope`, never a new
 * `listVersions` call.
 *
 * Mirrors email-composer-surface.ts's idioms — one root class with `__`
 * children, `el.className = …`, a `{ destroy() }` handle — and reuses
 * `bindAction`/`runAction` for every network-triggered control so a failure
 * always ends in a toast (shared/ui/run-action.ts's three obligations).
 *
 * Every command is the engine's, through `engine.ts`.
 */

import { uiEvent } from "../log";
import { engine } from '../engine';
import type { VersionRow, VersionList, ChangedPage, PageVerb } from '@symbiosis-lab/moss-ui';
import { bindAction, runAction } from "../shared/ui/run-action";
import { t, formatNumber, formatDate } from "../i18n/index";
import { VersionsRestoreModal } from './versions-restore-modal';
import { createGlyphToggleState, posePlusCrossGlyph } from "../shared/plus-cross-toggle";
import { mkBackCrumb, mkTitleBarRow } from './tree';

/** Same two-path markup as `#moss-add-plugin-button` (index.html) — the
 *  shared plus/cross morph (`shared/plus-cross-toggle.ts`) reads and rewrites
 *  these two `<path>` elements by class, not by position. */
const PLUS_CROSS_SVG =
  '<svg class="plus-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
  'stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path class="moss-add-glyph__mk" d="M19,12L5,12" />' +
  '<path class="moss-add-glyph__mk" d="M12,19L12,5" />' +
  '</svg>';

/** `failMessage` for every save/restore action on this surface. The one
 *  error `sealed_now` (`deploy/history.rs`) can still raise after settling
 *  the folder's pending seal is `one_shot::require_sealed`'s shared refusal —
 *  worded for Publish ("retry publishing"), because three callers share it.
 *  Substitute the Versions-appropriate sentence for exactly that one; any
 *  other failure keeps the generic "That didn't work: {error}" wording, which
 *  still says something true. */
function versionsFailMessage(detail: string): string {
  return detail.includes('are not sealed yet') ? t('versions.not_ready') : t('action.failed', { error: detail });
}

/** What a completed restore hands back on exit: the Undo toast is shown by
 *  the editor, not the surface. `message` is the finished toast text — the
 *  surface composes it, because it holds every fact (page vs. folder vs.
 *  site, and whether the restore was partial); the editor only displays it. */
export interface VersionsRestored {
  undoId: string | null;
  path: string | null;
  message: string;
}

export interface VersionsSurfaceContext {
  /** Fixed for the surface's whole life — see the file header's point 1.
   *  'site' always pairs with `path === ''` (PROJECT_ROOT_REL). */
  scope: 'page' | 'folder' | 'site';
  /** The file path in 'page' scope, the folder path in 'folder' scope, or
   *  '' in 'site' scope. Never re-derived once the surface is open. */
  path: string;
  /** The name of what was right-clicked — a file, a folder, or the site —
   *  shown as the surface's heading. */
  title: string;
   /** Replace the breadcrumb with a title bar: every screen calls this with its
   *  own back link + subject (+ actions). */
  setTitleBar(content: HTMLElement | null): void;
  /** The "‹ Editor" back link and every completed restore end here. */
  onExit(restored?: VersionsRestored): void;
}

export interface VersionsSurfaceHandle {
  destroy(): void;
}

const EMPTY_LIST: VersionList = {
  rows: [],
  live_id: null,
  has_multiple_targets: false,
  store_dir: '',
  changed_since: [],
  unchanged: [],
};

function timeLabel(d: Date): string {
  return formatDate(d, { hour: '2-digit', minute: '2-digit', hour12: false });
}

/**
 * "Sep 8, 11:20" — a version's own moment, always absolute. Every row and
 * every detail header uses this; there is no relative ("Today"/"Yesterday")
 * form anymore (file header, point 3).
 */
function fullWhenLabel(d: Date): string {
  return `${formatDate(d, { month: 'short', day: 'numeric' })}, ${timeLabel(d)}`;
}

/** The primary line of a row or a detail header: "Published" or the
 *  manual/restore label (falling back to "Saved by you" when unnamed), or
 *  the kept outside edit's own words. */
function primaryLabel(row: VersionRow): string {
  if (row.trigger === 'publish') return t('versions.published');
  if (row.trigger === 'displaced') return t('versions.outside_edit_kept');
  return row.label ?? t('versions.saved_by_you');
}

function basename(path: string): string {
  return path.split('/').pop() || path;
}

/** A `.mode-subject-name` span (round six's title-bar layer, file-tree.css). */
function boldName(text: string): HTMLElement {
  const el = document.createElement('span');
  el.className = 'mode-subject-name';
  el.textContent = text;
  return el;
}

/** List screen's subject: the path to what's being viewed, last segment
 *  bold — folder scope ends on the folder's own name; site scope is just
 *  the bold site name, no path prefix. The path prefix renders dimmed
 *  (`.mode-subject`'s own `--moss-color-muted`, distinct from the
 *  breadcrumb's `--moss-color-text-secondary`) so it doesn't read as a
 *  clickable crumb, and a page's name keeps its extension — the title bar
 *  has no icon to tell a file from a folder by, so ".md" is the only tell. */
function buildListSubject(ctx: VersionsSurfaceContext): HTMLElement {
  const el = document.createElement('span');
  el.className = 'mode-subject';
  if (ctx.scope === 'site') {
    el.appendChild(boldName(ctx.title));
    return el;
  }
  const segments = ctx.path.split('/');
  const last = segments.pop() ?? '';
  for (const seg of segments) el.appendChild(document.createTextNode(`${seg} › `));
  el.appendChild(boldName(last));
  return el;
}

/** Detail screen's subject: the version's own moment/label, with a Live
 *  badge inline when `row` is the top-level list's live one — same badge
 *  pattern `rowEl()` uses for its own name span. */
function buildDetailSubject(row: VersionRow, isLive: boolean): HTMLElement {
  const el = document.createElement('span');
  el.className = 'mode-subject';
  el.appendChild(boldName(`${fullWhenLabel(new Date(row.published_at))} · ${primaryLabel(row)}`));
  if (isLive) {
    const badge = document.createElement('span');
    badge.className = 'moss-versions__badge moss-status-badge moss-status-badge--green';
    badge.textContent = t('versions.live');
    el.appendChild(badge);
  }
  return el;
}

/** A file or folder path lies at or under `folder` — folder-scope's filter
 *  over an already-fetched site-wide answer (file header, "Two backend
 *  gaps"). `folder` is never '' here; callers only reach for this in
 *  'folder' scope. */
function underFolder(path: string, folder: string): boolean {
  return path === folder || path.startsWith(`${folder}/`);
}

/** `PageVerb` → the sub-line text on a "Changed since then" row. `restyled`
 *  (template-only re-render, no source bytes changed) has no dedicated
 *  string in the design; it reads as an edit, the closest of the three. */
function verbLabel(verb: PageVerb): string {
  switch (verb) {
    case 'added':
      return t('versions.verb_added');
    case 'deleted':
      return t('versions.verb_removed');
    case 'edited':
    case 'restyled':
    default:
      return t('versions.verb_changed');
  }
}

/** Folder/site-scope change summary: "3 pages changed · 1 added", zero parts
 *  omitted. Never computed in page scope — a page's timeline has no such
 *  notion (design doc, "The panel"). */
function summaryText(row: VersionRow): string {
  const parts: string[] = [];
  if (row.changed > 0) {
    parts.push(
      row.changed === 1
        ? t('versions.summary_changed_one')
        : t('versions.summary_changed', { n: formatNumber(row.changed) }),
    );
  }
  if (row.added > 0) parts.push(t('versions.summary_added', { n: formatNumber(row.added) }));
  if (row.removed > 0) parts.push(t('versions.summary_removed', { n: formatNumber(row.removed) }));
  return parts.join(' · ');
}

/** Render the Versions surface into `root`, starting on screen one (the
 *  list). Returns a handle for teardown. */
export function renderVersionsSurface(root: HTMLElement, ctx: VersionsSurfaceContext): VersionsSurfaceHandle {
  uiEvent('versions.rendered', { scope: ctx.scope });
  root.innerHTML = '';
  root.className = 'moss-versions';

  let destroyed = false;
  const scope = ctx.scope;
  let list: VersionList = EMPTY_LIST;
  const stale = () => destroyed || !root.isConnected;

  // The list screen's body element. Reassigned each time `buildListScreen()`
  // runs (including a "‹ Versions" return from a detail screen), so
  // `renderListBody` always paints into the CURRENT one.
  let body!: HTMLElement;

  // Folder/site scope opens the tree detail (screen three); page scope opens
  // the page detail (screen two).
  function rowEl(row: VersionRow): HTMLElement {
    const el = document.createElement('div');
    el.className = 'moss-versions__row moss-versions__row--link';
    el.dataset.id = row.id;
    el.addEventListener('click', () => {
      if (scope === 'page') {
        openPageDetail(row, ctx.path, () => buildListScreen());
      } else {
        openTreeDetail(row);
      }
    });

    const time = document.createElement('span');
    time.className = 'moss-versions__time';
    time.textContent = fullWhenLabel(new Date(row.published_at));
    el.appendChild(time);

    const main = document.createElement('div');
    main.className = 'moss-versions__row-main';

    const name = document.createElement('span');
    name.className = 'moss-versions__name';
    let byText: string | null = null;

    if (row.trigger === 'publish') {
      name.textContent = primaryLabel(row);
      if (list.has_multiple_targets) byText = row.target;
      if (row.id === list.live_id) {
        const badge = document.createElement('span');
        badge.className = 'moss-versions__badge moss-status-badge moss-status-badge--green';
        badge.textContent = t('versions.live');
        name.appendChild(badge);
      }
    } else {
      name.classList.add('moss-versions__name--strong');
      name.textContent = primaryLabel(row);
      if (row.label) byText = t('versions.saved_by_you');
    }
    main.appendChild(name);

    if (byText) {
      const by = document.createElement('span');
      by.className = 'moss-versions__by';
      by.textContent = byText;
      main.appendChild(by);
    }

    if (scope !== 'page') {
      const summary = summaryText(row);
      if (summary) {
        const sum = document.createElement('span');
        sum.className = 'moss-versions__summary';
        sum.textContent = summary;
        main.appendChild(sum);
      }
    }

    el.appendChild(main);
    return el;
  }

  function renderEmpty(): void {
    body.className = 'moss-versions__empty';
    const emptyTitle = document.createElement('div');
    emptyTitle.className = 'moss-versions__empty-title';
    emptyTitle.textContent = t('versions.empty_title');
    const emptyBody = document.createElement('div');
    emptyBody.className = 'moss-versions__empty-body';
    emptyBody.textContent = t('versions.empty_body');
    body.appendChild(emptyTitle);
    body.appendChild(emptyBody);
  }

  function renderListBody(): void {
    body.innerHTML = '';
    if (list.rows.length === 0) {
      renderEmpty();
      return;
    }
    body.className = 'moss-versions__list';
    for (const row of list.rows) body.appendChild(rowEl(row));
  }

  /** Screen one. Rebuilds the whole shell — called once at mount (before
   *  `loadInitial()` has anything to show) and again on "‹ Versions" from a
   *  detail screen (no network call: `list` is already cached). Resets the
   *  save control back to "+", the same way navigating away from an
   *  uncommitted frontmatter chip abandons it. */
  function buildListScreen(): void {
    root.innerHTML = '';
    let savingOpen = false;

    // The title bar: "‹ Editor" + the path, replacing the breadcrumb
    // wholesale (round six) — set on every call, including a "‹ Versions"
    // return from a detail screen, which the strip crossfades as a swap.
    ctx.setTitleBar(mkTitleBarRow(
      mkBackCrumb(t('versions.back_editor'), () => ctx.onExit()),
      buildListSubject(ctx),
    ));

    // --- Header: the scope caption ("Versions"/"Site versions") paired with
    // the save control, so the "+" reads as "add to THIS LIST" not "THIS FILE".
    const header = document.createElement('div');
    header.className = 'moss-versions__header';

    const captionRow = document.createElement('div');
    captionRow.className = 'moss-versions__caption-row';
    const caption = document.createElement('div');
    caption.className = 'moss-versions__caption';
    caption.textContent = scope === 'site' ? t('versions.site_title') : t('versions.title');
    captionRow.appendChild(caption);
    const saveSlot = document.createElement('div');
    saveSlot.className = 'moss-versions__save-slot';
    captionRow.appendChild(saveSlot);
    header.appendChild(captionRow);
    root.appendChild(header);

    // --- Save a version: a persistent "+"/cancel toggle beside a name field
    // + confirm that mount/unmount ------------------------------------------
    // The toggle button is created ONCE and persists across open/closed
    // state — only the field+confirm pair beside it mount and unmount, the
    // shared plus/cross morph also used by the channel-pill "+". The
    // field is only ever (re)created on open/cancel/confirm, never mid-
    // keystroke, so its value can be read straight off the DOM at confirm
    // time — no separate draft variable to hold.
    const toggleState = createGlyphToggleState();
    const toggleBtn = document.createElement('button');
    toggleBtn.type = 'button';
    toggleBtn.className = 'moss-versions__save-toggle';
    toggleBtn.setAttribute('aria-expanded', 'false');
    toggleBtn.setAttribute('data-tooltip-position', 'bottom');
    toggleBtn.innerHTML = PLUS_CROSS_SVG;
    saveSlot.appendChild(toggleBtn);

    const fieldSlot = document.createElement('div');
    fieldSlot.className = 'moss-versions__save-field-slot';
    saveSlot.appendChild(fieldSlot);

    function updateToggleLabel(): void {
      const label = savingOpen ? t('versions.save_cancel') : t('versions.save');
      toggleBtn.setAttribute('aria-label', label);
      toggleBtn.setAttribute('data-tooltip', label);
    }

    // Also the field's Escape handler and a successful confirm's own close —
    // every path back to "+" goes through here so the glyph, the label and
    // the field slot never drift out of step with each other.
    function setSavingOpen(open: boolean): void {
      savingOpen = open;
      posePlusCrossGlyph(toggleBtn, open, toggleState);
      updateToggleLabel();
      renderSaveField();
    }

    function renderSaveField(): void {
      fieldSlot.innerHTML = '';
      if (!savingOpen) return;

      const inline = document.createElement('div');
      inline.className = 'moss-versions__save-inline';

      const field = document.createElement('input');
      field.type = 'text';
      field.className = 'moss-versions__save-field pop-input';
      field.placeholder = t('versions.save_placeholder');
      field.setAttribute('aria-label', t('versions.save'));

      const confirmBtn = document.createElement('button');
      confirmBtn.type = 'button';
      confirmBtn.className = 'moss-versions__save-confirm moss-btn moss-btn-primary';
      confirmBtn.setAttribute('aria-label', t('versions.save_confirm'));
      confirmBtn.textContent = '✓';

      field.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') confirmBtn.click();
        else if (e.key === 'Escape') setSavingOpen(false);
      });

      bindAction(confirmBtn, {
        run: () => engine.saveVersion(field.value.trim() === '' ? null : field.value.trim()),
        onOk: (newRow) => {
          list = { ...list, rows: [newRow, ...list.rows] };
          setSavingOpen(false);
          renderListBody();
        },
        failMessage: versionsFailMessage,
        stale,
      });

      inline.appendChild(field);
      inline.appendChild(confirmBtn);
      fieldSlot.appendChild(inline);
      field.focus();
    }

    toggleBtn.addEventListener('click', () => setSavingOpen(!savingOpen));
    updateToggleLabel();

    // --- List / empty body ------------------------------------------------
    body = document.createElement('div');
    root.appendChild(body);

    renderListBody();
  }

  /**
   * Both detail screens label Restore with the affected file, folder or site
   * name. The rarely needed "Save a copy" action is omitted so restoration
   * stays the primary choice. The click still differs by scope:
   * page restores immediately, while folder/site opens a
   * confirm modal first, a deliberate difference in blast radius, not
   * chrome — so this builds and appends the one element and returns it for
   * the caller to wire. `actions` is a `.mode-actions` div in the strip's
   * title-bar layer now, not a header row of this surface's own (round six)
   * — the explanatory note (`versions.restore_note_page`/`_folder`/`_site`)
   * is the button's tooltip; `aria-label` carries both the action and the
   * explanation, since the button already has real visible text.
   */
  function renderRestoreAction(actions: HTMLElement, name: string, noteText: string): HTMLButtonElement {
    const restoreBtn = document.createElement('button');
    restoreBtn.type = 'button';
    restoreBtn.className = 'moss-versions__restore moss-btn moss-btn-secondary';
    const label = t('versions.restore_scope', { name });
    restoreBtn.textContent = label;
    restoreBtn.setAttribute('data-tooltip', noteText);
    restoreBtn.setAttribute('aria-label', `${label} — ${noteText}`);
    actions.appendChild(restoreBtn);
    return restoreBtn;
  }

  /**
   * Screen two, "A page's version" (design section of the same name). `row`
   * carries the moment (published_at/trigger/label) — the SAME row whether
   * reached from the top-level page-scope list or from a folder/site
   * version's drill-down, since in the latter case it is that folder/site
   * version's own row: the page is being read AT that moment, not at one of
   * its own. `backTo` is screen one for the first case, the originating
   * folder/site screen for the second (design, "A site version": "each row
   * opens that page's version view").
   */
  function openPageDetail(row: VersionRow, path: string, backTo: () => void): void {
    root.innerHTML = '';

    // The title bar: "‹ Versions" + moment/label + Restore trailing — no
    // header row of its own any more (round six); body starts under the strip.
    const actions = document.createElement('div');
    actions.className = 'mode-actions';
    ctx.setTitleBar(mkTitleBarRow(
      mkBackCrumb(t('versions.title'), backTo),
      buildDetailSubject(row, row.id === list.live_id),
      actions,
    ));

    // A restore auto-saves the current state first and offers Undo, so it is
    // no longer a one-way, high-stakes action — secondary weight, not primary
    // so the label follows the same pattern as folder/site scope:
    // `name` is the PAGE being viewed, i.e. `path` (the function parameter),
    // never `ctx.title` — `ctx.title` is the FOLDER's name when this screen
    // is reached via a folder/site drill-down.
    const restoreBtn = renderRestoreAction(actions, basename(path), t('versions.restore_note_page'));

    const errorLine = document.createElement('div');
    errorLine.className = 'moss-versions__restore-error';
    errorLine.hidden = true;
    root.appendChild(errorLine);

    const pageBody = document.createElement('div');
    pageBody.className = 'moss-versions__page-body';
    root.appendChild(pageBody);

    const detailStale = () => stale() || !pageBody.isConnected;

    function showRestoreError(message: string): void {
      errorLine.hidden = false;
      errorLine.textContent = message;
    }

    bindAction(restoreBtn, {
      run: () => engine.restoreVersion(row.id, path, 'in_place'),
      onOk: (report) => {
        if (report.failed.length > 0) {
          showRestoreError(report.failed[0].error);
          return;
        }
        ctx.onExit({
          undoId: report.undo_id,
          path,
          message: t('versions.restored', { what: basename(path), when: fullWhenLabel(new Date(row.published_at)) }),
        });
      },
      failMessage: versionsFailMessage,
      stale: detailStale,
    });

    void engine.readVersion(row.id, path).then((result) => {
      if (detailStale()) return;
      if (result.status !== 'ok') {
        console.error('[versions-surface] readVersion failed:', result.error);
        return;
      }
      if (!result.data.kept) {
        restoreBtn.disabled = true;
        const notKept = document.createElement('p');
        notKept.className = 'moss-versions__not-kept';
        notKept.textContent = t('versions.not_kept');
        pageBody.appendChild(notKept);
        return;
      }
      const pre = document.createElement('pre');
      pre.className = 'moss-versions__source';
      pre.textContent = result.data.text ?? '';
      pageBody.appendChild(pre);
    });
  }

  /**
   * Screen three, "A folder or site version". `row` is the top-level
   * folder/site-scope row that was clicked — its id is what the drill-down
   * `listVersions` call keys off. Folder scope additionally filters the
   * drill-down's two arrays to `ctx.path`'s subtree (file header, "Two
   * backend gaps") and restores only that subtree.
   */
  function openTreeDetail(row: VersionRow): void {
    root.innerHTML = '';

    const isSite = scope === 'site';

    // The title bar follows the page-scope layout — one
    // restore button, one label pattern, one weight, in all three scopes.
    const actions = document.createElement('div');
    actions.className = 'mode-actions';
    ctx.setTitleBar(mkTitleBarRow(
      mkBackCrumb(t('versions.title'), () => buildListScreen()),
      buildDetailSubject(row, row.id === list.live_id),
      actions,
    ));

    // `name` is always `ctx.title`: the folder's own name in folder scope,
    // the root's display name in site scope — `restore_scope` replaces the
    // old site-only "Restore site" wording, so there is exactly one restore-
    // button label key for this whole surface.
    const restoreBtn = renderRestoreAction(
      actions,
      ctx.title,
      isSite ? t('versions.restore_note_site') : t('versions.restore_note_folder'),
    );

    const treeBody = document.createElement('div');
    treeBody.className = 'moss-versions__list';
    root.appendChild(treeBody);

    const treeStale = () => stale() || !treeBody.isConnected;

    let changedSince: ChangedPage[] = [];
    let unchanged: string[] = [];

    function pageRow(label: string, sub: string | null, onOpen: () => void): HTMLElement {
      const el = document.createElement('div');
      el.className = 'moss-versions__row moss-versions__row--link';
      const main = document.createElement('div');
      main.className = 'moss-versions__row-main';
      const name = document.createElement('span');
      name.className = 'moss-versions__name';
      name.textContent = label;
      main.appendChild(name);
      if (sub) {
        const subEl = document.createElement('span');
        subEl.className = 'moss-versions__by';
        subEl.textContent = sub;
        main.appendChild(subEl);
      }
      el.appendChild(main);
      const chevron = document.createElement('span');
      chevron.className = 'moss-versions__chevron';
      chevron.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 6 15 12 9 18"></polyline></svg>';
      el.appendChild(chevron);
      el.addEventListener('click', onOpen);
      return el;
    }

    function renderTreeBody(): void {
      treeBody.innerHTML = '';
      if (changedSince.length > 0) {
        const label = document.createElement('div');
        label.className = 'moss-versions__group';
        label.textContent = t('versions.changed_since');
        treeBody.appendChild(label);
        for (const page of changedSince) {
          treeBody.appendChild(
            pageRow(basename(page.source_path), verbLabel(page.verb), () =>
              openPageDetail(row, page.source_path, () => openTreeDetail(row)),
            ),
          );
        }
      }
      if (unchanged.length > 0) {
        const label = document.createElement('div');
        label.className = 'moss-versions__group';
        label.textContent = t('versions.unchanged');
        treeBody.appendChild(label);
        for (const path of unchanged) {
          treeBody.appendChild(
            pageRow(basename(path), null, () => openPageDetail(row, path, () => openTreeDetail(row))),
          );
        }
      }
    }

    restoreBtn.addEventListener('click', () => {
      // A page with verb "added" was created after this moment: a restore
      // trashes it rather than reverting it, so it does not "go back" — the
      // pages count excludes it from changed_since's total.
      const added = changedSince.filter((p) => p.verb === 'added').length;
      const pages = changedSince.length - added;
      new VersionsRestoreModal({
        when: fullWhenLabel(new Date(row.published_at)),
        pages,
        added,
        folderName: isSite ? null : ctx.title,
        onConfirm: () => {
          void runAction({
            key: 'versions-restore-tree',
            run: () => engine.restoreVersion(row.id, isSite ? null : ctx.path, 'in_place'),
            onOk: (report) => {
              // A partial restore's own string already says what came back
              // ("Restored 1 of 2 pages"); running it through the generic
              // "Restored {what} from {when}" template a second time doubled
              // the word (fix: the restore toast is composed here, once).
              const message =
                report.failed.length > 0
                  ? t('versions.restored_partial', {
                      done: report.restored.length,
                      total: report.restored.length + report.failed.length,
                    })
                  : t('versions.restored', {
                      what: isSite ? t('versions.scope_site') : ctx.title,
                      when: fullWhenLabel(new Date(row.published_at)),
                    });
              ctx.onExit({ undoId: report.undo_id, path: null, message });
            },
            failMessage: versionsFailMessage,
            stale: treeStale,
          });
        },
      }).open();
    });

    void engine.listVersions('site', row.id).then((result) => {
      if (treeStale()) return;
      if (result.status !== 'ok') {
        console.error('[versions-surface] listVersions (tree detail) failed:', result.error);
        return;
      }
      changedSince = isSite
        ? result.data.changed_since
        : result.data.changed_since.filter((p) => underFolder(p.source_path, ctx.path));
      unchanged = isSite
        ? result.data.unchanged
        : result.data.unchanged.filter((p) => underFolder(p, ctx.path));
      renderTreeBody();
    });
  }

  async function loadInitial(): Promise<void> {
    const result = await engine.listVersions(ctx.scope, ctx.path);
    if (stale()) return;
    if (result.status === 'ok') {
      uiEvent('versions.list-loaded', { scope: ctx.scope, count: result.data.rows.length });
      list = result.data;
      renderListBody();
    } else {
      uiEvent('versions.list-loaded', { scope: ctx.scope, error: result.error });
      console.error('[versions-surface] listVersions failed:', result.error);
    }
  }

  buildListScreen();
  void loadInitial();

  return {
    destroy(): void {
      destroyed = true;
    },
  };
}
