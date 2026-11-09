/**
 * Untitled-first file creation.
 *
 * A persistent ＋ in the action panel's `.nav-right` creates `untitled.md` in
 * the browsed folder IMMEDIATELY (no name prompt, auto-suffix on collision),
 * opens it with the caret on the empty heading, and lets the committed H1 name
 * the file through the tree's rename-with-reference-updates machinery. A
 * status line under the editor shows the live destination + filename + Undo
 * until the first body edit; after that the filename is the author's.
 *
 * The session is identified by EntryId (never path-string comparison), so the
 * open buffer, tree row and breadcrumb stay stable through the heading rename.
 *
 * This module owns: the ＋/▾ buttons' behavior, the create menu, the session
 * state machine, the status line, and the ⌘N/⇧⌘N bindings for the editor
 * webview. editor-main wires it with five one-line touch-points (loadFile
 * notification, heading-commit bypass, body-edit notification, focus handoff,
 * teardown) — see CreateFlowDeps.
 */

import { t } from "../i18n/index";
import { showToast } from "../shared/ui/toast/toast-manager";
import { joinCtxGroups, openCtxMenu, type CtxMenuEntry, type CtxMenuHandle, type CtxMenuNote } from "../shared/ui/ctx-menu";
import type { EntryId } from './entry-registry';
import { PROJECT_ROOT_REL, parentDirOf, resolveCreateTarget, siblingPath, toAbsolute, toRelative } from './path-domain';
import {
  ONE_LANGUAGE,
  templateDestination,
  templatesNamingFolder,
  translationSlots,
  type LanguageSource,
  type SiteLanguagesInfo,
  type TranslationSlot,
} from './languages';
import { langName } from "../i18n/lang-names";
import {
  deleteTemplate,
  promptRenameTemplate,
  clearTemplateFolder,
  promptTemplateFolder,
  templateFailed,
  templateRowLabel,
  type TemplateCommands,
  type TemplateMeta,
} from './template-menu';

// ── Pure naming helpers ────────────────────────────────────────────────────

/** First free untitled name among `taken` (case-insensitive — APFS default),
 *  counting `untitled.md`, `untitled 2.md`, `untitled 3.md`, … */
export function untitledBasename(taken: Iterable<string>): string {
  const set = new Set<string>();
  for (const name of taken) set.add(name.toLowerCase());
  for (let i = 1; ; i++) {
    const candidate = i === 1 ? 'untitled.md' : `untitled ${i}.md`;
    if (!set.has(candidate)) return candidate;
  }
}

/** Date-prefix convention: ≥2 sibling `.md` files named `YYYY-MM-DD-…`. */
export function hasDatePrefixConvention(siblingNames: Iterable<string>): boolean {
  let count = 0;
  for (const name of siblingNames) {
    if (/^\d{4}-\d{2}-\d{2}-/.test(name) && name.toLowerCase().endsWith('.md')) {
      count++;
      if (count >= 2) return true;
    }
  }
  return false;
}

/** Filename-safe slug of a typed title: lowercase, Unicode letters/numbers
 *  kept, every other run collapsed to a single `-`. Mirrors the prototype. */
export function slugifyTitle(title: string): string {
  return title
    .normalize('NFC')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '');
}

/**
 * The basename a create-session title commit renames to, or null when the
 * title is empty (keep the current untitled name — never an error dialog).
 *
 * Without the date convention the title is used verbatim (exactly what the
 * heading's Filename-mode rename does today, so the H1 round-trips). With it,
 * the name follows the siblings' `YYYY-MM-DD-slug` shape — and because that
 * slug no longer round-trips to the typed title, the caller must also write
 * `title:` frontmatter so the H1 keeps showing the author's words.
 */
export function generatedBasename(
  title: string,
  opts: { datePrefix: boolean; today: string; ext?: string },
): string | null {
  const trimmed = title.trim();
  if (!trimmed) return null;
  const ext = opts.ext ?? '.md';
  if (!opts.datePrefix) return `${trimmed}${ext}`;
  return `${opts.today}-${slugifyTitle(trimmed) || 'untitled'}${ext}`;
}

/** Today in the date-prefix convention's `YYYY-MM-DD`, local time. */
export function todayIso(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, '0');
  const d = String(now.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

// ── Session state machine ──────────────────────────────────────────────────

interface CreateSession {
  /** Identity of the created file — survives every rename (EntryId, not path). */
  entryId: EntryId;
  /** Destination dir, project-root-relative ('' = root). */
  dir: string;
  /** Display name of the destination folder. */
  dirName: string;
  /** File open before the create; Undo returns here. Null = none. */
  prevPath: string | null;
  datePrefix: boolean;
  today: string;
  /** Basename the file currently has (kept for the status line). */
  currentName: string;
  /**
   * 'pending'    — file created, its loadFile not yet completed.
   * 'title-only' — open in the editor, no body edit yet: the heading rename
   *                follows the title and the status line is visible.
   * (The session object is dropped entirely once a body edit ends it.)
   */
  phase: 'pending' | 'title-only';
  /** A page made as another language's version: its title is not a rename. */
  translation: boolean;
}

/** The subset of FileTree the create flow touches (structural — keeps this
 *  module import-light and the seam obvious). */
export interface CreateFlowTree {
  createAnchor: { path: string; isDir: boolean } | null;
  refreshAndRender(): Promise<void>;
  revealNewFile(filePath: string): void;
  startCreate(folderPath: string, type: 'file' | 'folder', opts?: { templateId?: string }): Promise<void>;
}

/**
 * The one owner of the reveal-then-open sequence every "a new entry now exists
 * at `rel`" flow needs: rebuild tree rows from disk, re-anchor the breadcrumb +
 * select the new row (revealNewFile, NOT highlight — see its doc comment), then
 * await the same open action the file-tree inline create enters. Completion
 * means the editor has opened the file, rather than an event merely dispatched. Shared by `newPage`, the create-home
 * flow in editor-main, and template instantiation — previously independent
 * copies of this exact three-call sequence.
 */
export async function revealAndOpenNewEntry(
  tree: Pick<CreateFlowTree, 'refreshAndRender' | 'revealNewFile'> | null | undefined,
  rel: string,
  open: (relativePath: string) => Promise<void>,
): Promise<void> {
  await tree?.refreshAndRender();
  tree?.revealNewFile(rel);
  await open(rel);
}

/** Minimal command-result shape; editor-main injects the engine commands
 *  through these. */
type CommandResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

export interface CreateFlowDeps {
  projectRoot(): string;
  fileTree(): CreateFlowTree | null;
  /** Registry-domain (relative) path of the open file, or null. */
  currentFilePath(): string | null;
  /** Await the same open action the file tree enters, with new-file focus. */
  openNewEntry(relativePath: string): Promise<void>;
  /** Registry upsert — the EntryId mint (identity, never path comparison). */
  upsert(relPath: string): EntryId | null;
  /** Live relative path for an EntryId (rename-safe derivation). */
  pathOf(id: EntryId): string | null;
  /** Same watcher-echo suppression the tree rename uses. */
  /** Announce a rename moss is about to issue (both sides, relative), so the
   *  watcher echo is swallowed rather than reloading a path mid-move. */
  registerPendingRename(from: string, to: string): void;
  /** Rename through the tree (renameEntryWithRefs + lifecycle). Absolute paths. */
  renameEntry(oldAbs: string, newAbs: string): Promise<{ status: 'ok' } | { status: 'error'; error: string }>;
  /** Write `title:` frontmatter via the chip-bar store (Title-mode precedent). */
  setTitleField(value: string): void;
  /** Push text into the heading controller's saved state (clears to placeholder). */
  setHeadingSaved(text: string): void;
  // Engine commands, injected by editor-main:
  listDirectory(absDir: string): Promise<CommandResult<Array<{ name: string }>>>;
  createFile(absDir: string, name: string): Promise<CommandResult<string>>;
  /** Like createFile, but seeded from a saved page template. */
  instantiateTemplate(templateId: string, absDir: string, name: string): Promise<CommandResult<string>>;
  deleteEntry(absPath: string): Promise<CommandResult<null>>;
  /** The template list the ＋▾ menu reads before opening, and the rename /
   *  delete its Manage Templates… levels reach (one seam type, not three
   *  re-declared signatures). Null in a plain folder: templates are a site's,
   *  so a loose document's menu has no template rows rather than an error. */
  templates: Omit<TemplateCommands, 'createTemplate'> | null;
  /** The site's language trees, and existence probes for a page's other-language places. */
  languages: LanguageSource;
  /** Make the page's version in `lang` at the mirrored path; returns the new page's absolute path. */
  createTranslation(sourceAbs: string, lang: string): Promise<CommandResult<string>>;
}

const NO_TEMPLATES: TemplateMeta[] = [];

export class CreateFlow {
  private session: CreateSession | null = null;
  private creating = false;
  private menu: CtxMenuHandle | null = null;
  private abort = new AbortController();

  constructor(private deps: CreateFlowDeps) {}

  // ── Wiring ───────────────────────────────────────────────────────────────

  wire(): void {
    const { signal } = this.abort;
    const plusBtn = document.getElementById('editor-create-btn');
    const caretBtn = document.getElementById('editor-create-caret');

    if (plusBtn) {
      plusBtn.setAttribute('aria-label', t('tree.new_page'));
      this.refreshTooltip(plusBtn);
      // Recompute on hover/focus: the tooltip system reads `data-tooltip` at
      // mouseover and observes attribute mutations while shown, so writing the
      // fresh destination here is enough to keep it live as the anchor moves.
      plusBtn.addEventListener('mouseover', () => this.refreshTooltip(plusBtn), { signal });
      plusBtn.addEventListener('focusin', () => this.refreshTooltip(plusBtn), { signal });
      plusBtn.addEventListener('click', () => { void this.newPage(); }, { signal });
    }
    if (caretBtn) {
      caretBtn.setAttribute('aria-label', t('create.caret_aria'));
      caretBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        // detail === 0 → keyboard activation (Enter/Space on the button):
        // open with roving focus on the first item, per the menu pattern.
        void this.toggleMenu(caretBtn, { focusFirst: e.detail === 0 });
      }, { signal });
    }

    // ⌘N / ⇧⌘N — bound here because keydown never crosses webviews: while the
    // user types in the editor panel, the shell's handler can't fire (same
    // precedent as the ⌘⇧E twin in editor-layout-btn.ts).
    document.addEventListener('keydown', (e) => {
      // A handler closer to the target already claimed the key (e.g. CM6's
      // macOS Ctrl-N = cursorLineDown, which preventDefaults) — not a create.
      if (e.defaultPrevented) return;
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      if (e.shiftKey && (e.key === 'N' || e.key === 'n')) {
        e.preventDefault();
        this.newFolder();
      } else if (!e.shiftKey && e.key === 'n') {
        e.preventDefault();
        void this.newPage();
      }
    }, { signal });

    // (The shell webview binds the same shortcuts and forwards them over the
    // event bus; editor-main registers those listeners — the interim seam
    // keeps @tauri-apps imports out of this module.)

    // Live filename preview while the title is typed (before commit).
    const headingEl = document.getElementById('editor-heading');
    headingEl?.addEventListener('input', () => {
      const s = this.session;
      if (!s || s.phase !== 'title-only') return;
      const title = (headingEl.textContent ?? '').trim();
      const preview = generatedBasename(title, { datePrefix: s.datePrefix, today: s.today });
      this.renderStatus(preview ?? s.currentName);
    }, { signal });
  }

  destroy(): void {
    this.closeMenu();
    this.abort.abort();
    this.session = null;
  }

  // ── Destination ──────────────────────────────────────────────────────────

  /** The create destination derived from the tree's breadcrumb anchor.
   *  `activePath` is frozen display state — read via the anchor, never written. */
  private currentTarget(): { dir: string; dirName: string } {
    const anchor = this.deps.fileTree()?.createAnchor ?? null;
    const dir = anchor ? resolveCreateTarget(anchor.path, anchor.isDir) : PROJECT_ROOT_REL;
    return { dir, dirName: this.dirNameOf(dir) };
  }

  private dirNameOf(dir: string): string {
    if (dir === PROJECT_ROOT_REL) {
      const root = this.deps.projectRoot();
      return root.split('/').filter(Boolean).pop() ?? root;
    }
    return dir.split('/').pop() ?? dir;
  }

  private refreshTooltip(btn: HTMLElement): void {
    btn.setAttribute('data-tooltip', t('create.new_page_tooltip', { folder: this.currentTarget().dirName }));
  }

  // ── Actions ──────────────────────────────────────────────────────────────

  /**
   * ＋ / ⌘N: create `untitled.md` in the destination and open it. A page
   * template rides the same session — its title is already cleared, so the
   * empty H1 names the file on commit exactly as a blank page's does —
   * seeded by `instantiateTemplate` instead of `createFile`, into the folder
   * the menu was opened on (`dir`) rather than the breadcrumb anchor.
   */
  async newPage(opts?: { dir?: string; templateId?: string; blank?: boolean }): Promise<void> {
    if (this.creating) return;
    const root = this.deps.projectRoot();
    if (!root) return;
    this.creating = true;
    try {
      const { dir, dirName } = opts?.dir !== undefined
        ? { dir: opts.dir, dirName: this.dirNameOf(opts.dir) }
        : this.currentTarget();
      // In a folder that exactly one page template names, a plain new page is
      // made from it; `blank` (the menu's Blank page row) opts out.
      const templateId = opts?.templateId ?? (opts?.blank ? undefined : await this.soleFolderTemplate(dir));
      const absDir = dir === PROJECT_ROOT_REL ? root : toAbsolute(dir, root);

      // Sibling names drive both the untitled suffix and date-prefix
      // inference. A failed listing degrades to no inference — the create
      // itself still collision-retries below.
      let siblings: string[] = [];
      try {
        const listed = await this.deps.listDirectory(absDir);
        if (listed.status === 'ok') siblings = listed.data.map((e) => e.name);
      } catch { /* degrade */ }

      const datePrefix = hasDatePrefixConvention(siblings);
      const taken = new Set(siblings.map((n) => n.toLowerCase()));

      // Auto-suffix on collision — NEVER a blocking dialog. The on-disk truth
      // can differ from the listing (external writes), so a create that fails
      // with "already exists" marks the name taken and tries the next.
      let createdAbs: string | null = null;
      for (let attempts = 0; attempts < 200 && !createdAbs; attempts++) {
        const name = untitledBasename(taken);
        const result = templateId !== undefined
          ? await this.deps.instantiateTemplate(templateId, absDir, name)
          : await this.deps.createFile(absDir, name);
        if (result.status === 'ok') {
          createdAbs = result.data;
        } else if (result.error.includes('already exists')) {
          taken.add(name.toLowerCase());
        } else {
          showToast({ what: t('tree.create_failed').replace('{error}', result.error), severity: 'Blocking' });
          return;
        }
      }
      if (!createdAbs) return;

      await this.openCreated(toRelative(createdAbs, root), { dir, dirName, datePrefix, translation: false });
    } finally {
      this.creating = false;
    }
  }

  /** The one owner of "a new page now exists": its session, then the reveal-and-open. */
  private async openCreated(rel: string, s: { dir: string; dirName: string; datePrefix: boolean; translation: boolean }): Promise<void> {
    const entryId = this.deps.upsert(rel);
    if (entryId === null) return;
    this.session = {
      entryId,
      dir: s.dir,
      dirName: s.dirName,
      prevPath: this.deps.currentFilePath(),
      datePrefix: s.datePrefix,
      today: todayIso(),
      currentName: rel.split('/').pop() ?? rel,
      phase: 'pending',
      translation: s.translation,
    };
    await revealAndOpenNewEntry(this.deps.fileTree(), rel, this.deps.openNewEntry);
  }

  /** The one page template that names `dir`'s folder, when exactly one does. */
  private async soleFolderTemplate(dir: string): Promise<string | undefined> {
    const [templates, langs] = await Promise.all([this.readTemplates(), this.readLanguages()]);
    const pages = templatesNamingFolder(dir, templates, langs).filter((m) => m.kind === 'page');
    return pages.length === 1 ? pages[0].id : undefined;
  }

  private async readTemplates(): Promise<TemplateMeta[]> {
    if (!this.deps.templates) return NO_TEMPLATES;
    try {
      const listed = await this.deps.templates.listTemplates();
      if (listed.status === 'ok') return listed.data;
      templateFailed(listed.error);
    } catch (err) {
      templateFailed(String(err));
    }
    return NO_TEMPLATES;
  }

  private async readLanguages(): Promise<SiteLanguagesInfo> {
    try {
      const langs = await this.deps.languages.siteLanguages();
      if (langs.status === 'ok') return langs.data;
    } catch { /* a failed read is a one-language site */ }
    return ONE_LANGUAGE;
  }

  /**
   * Make the current page's version in `lang`: the Rust side places it at the
   * mirrored path and links the two. The new page opens untitled-first, with
   * its title written, not renamed from it.
   */
  private async createTranslationPage(lang: string): Promise<void> {
    const root = this.deps.projectRoot();
    const source = this.deps.currentFilePath();
    if (this.creating || !root || !source) return;
    this.creating = true;
    try {
      const made = await this.deps.createTranslation(toAbsolute(source, root), lang);
      if (made.status !== 'ok') {
        showToast({ what: t('tree.create_failed').replace('{error}', made.error), severity: 'Blocking' });
        return;
      }
      const rel = toRelative(made.data, root);
      const dir = parentDirOf(rel);
      await this.openCreated(rel, { dir, dirName: this.dirNameOf(dir), datePrefix: false, translation: true });
    } finally {
      this.creating = false;
    }
  }

  /** ⇧⌘N: folders keep the inline-name input — the name IS the content. */
  newFolder(): void {
    const { dir } = this.currentTarget();
    void this.deps.fileTree()?.startCreate(dir, 'folder');
  }

  // ── Menu (▾) ─────────────────────────────────────────────────────────────

  /**
   * The ＋▾ menu is the home of templates: create-from-template rows at the
   * root, and a Manage Templates… drill-in reaching Rename / Delete per template.
   * The list is read BEFORE the menu opens — a local directory read — so the
   * primitive stays synchronous; a failed read opens the empty shape and
   * toasts. A second caret press inside that read sees no open menu and opens
   * a second one, which the primitive resolves by closing the first: accepted
   * rather than guarded with a flag.
   */
  private async toggleMenu(anchor: HTMLElement, opts?: { focusFirst?: boolean }): Promise<void> {
    if (this.menu) { this.closeMenu(); return; }
    const { dir, dirName } = this.currentTarget();

    const [templates, langs] = await Promise.all([this.readTemplates(), this.readLanguages()]);
    const slots = await this.missingTranslations(langs);
    const matching = templatesNamingFolder(dir, templates, langs);
    const pageMatches = matching.filter((m) => m.kind === 'page');
    const sole = pageMatches.length === 1 ? pageMatches[0] : null;

    // Shared context-menu primitive; anchored under the caret, right-aligned
    // to it (the panel's right edge). The trailing note names the create
    // destination.
    this.menu = openCtxMenu({
      entries: joinCtxGroups([
        this.createRows(dir, langs, matching, sole),
        this.templateRows(dir, langs, templates.filter((m) => !matching.includes(m))),
        this.translationRows(slots),
        this.manageTemplatesLevel(templates),
        [destinationNote(dirName)],
      ]),
      position: { kind: 'below', anchor, align: 'right' },
      // The caret button toggles: clicking it while open must not race the
      // outside-click dismissal into a close-then-reopen.
      outsideAnchor: anchor,
      focusFirst: opts?.focusFirst,
      onClose: () => { this.menu = null; },
      logSurface: 'create-menu',
      logTarget: `dir:${dirName}`,
    });
  }

  private closeMenu(): void {
    this.menu?.close();
    this.menu = null;
  }

  /** The create rows. In a folder one page template names, that template is the
   *  first row (⌘N) and the plain page is "Blank page"; with two or more, they
   *  are listed first and the plain page stays "New Page". */
  private createRows(dir: string, langs: SiteLanguagesInfo, matching: TemplateMeta[], sole: TemplateMeta | null): CtxMenuEntry[] {
    const newFolder: CtxMenuEntry = { label: t('tree.new_folder'), id: 'new-folder', kbd: '⇧⌘N', action: () => { this.newFolder(); } };
    const others = matching.filter((m) => m !== sole);
    if (sole) {
      return [
        this.templateRow(sole, dir, langs, '⌘N'),
        ...others.map((m) => this.templateRow(m, dir, langs)),
        { label: t('create.blank_page'), id: 'blank-page', action: () => { void this.newPage({ blank: true }); } },
        newFolder,
      ];
    }
    return [
      ...matching.map((m) => this.templateRow(m, dir, langs)),
      { label: t('tree.new_page'), id: 'new-page', kbd: '⌘N', action: () => { void this.newPage({ blank: true }); } },
      newFolder,
    ];
  }

  /** The "From template" group: a note, then one row per template. A template
   *  with a folder lands there, not in the menu's folder. An empty store
   *  contributes nothing — the group and its separator simply do not appear
   *  and no "no templates yet" line is shown. */
  private templateRows(dir: string, langs: SiteLanguagesInfo, templates: TemplateMeta[]): CtxMenuEntry[] {
    if (templates.length === 0) return [];
    return [
      { kind: 'note', render: (host) => { host.textContent = t('create.from_template_label'); } },
      ...templates.map((meta): CtxMenuEntry => this.templateRow(meta, dir, langs)),
    ];
  }

  private templateRow(meta: TemplateMeta, dir: string, langs: SiteLanguagesInfo, kbd?: string): CtxMenuEntry {
    return {
      label: templateRowLabel(meta),
      id: `template:${meta.id}`,
      ...(kbd ? { kbd } : {}),
      action: () => { void this.createFromTemplate(meta, dir, langs); },
    };
  }

  /** A template's page or folder, in the folder it names. A folder that no
   *  longer exists is an error that creates nothing. */
  private async createFromTemplate(meta: TemplateMeta, dir: string, langs: SiteLanguagesInfo): Promise<void> {
    const root = this.deps.projectRoot();
    const dest = templateDestination(dir, meta.folder, langs);
    if (meta.folder !== null) {
      const absDest = dest === PROJECT_ROOT_REL ? root : toAbsolute(dest, root);
      const listed = await this.deps.listDirectory(absDest).catch(() => null);
      if (listed?.status !== 'ok') {
        showToast({ what: t('create.template_folder_gone', { folder: meta.folder }), severity: 'Blocking' });
        return;
      }
    }
    if (meta.kind === 'folder') void this.deps.fileTree()?.startCreate(dest, 'folder', { templateId: meta.id });
    else void this.newPage({ dir: dest, templateId: meta.id });
  }

  /** The translation rows for the open page: one per language it lacks. */
  private translationRows(slots: TranslationSlot[]): CtxMenuEntry[] {
    return slots.map((s): CtxMenuEntry => ({
      label: s.folderMissing
        ? t('create.translation_folder_row', { language: langName(s.lang) })
        : t('create.translation_row', { language: langName(s.lang) }),
      id: `translation:${s.lang}`,
      action: () => { void this.createTranslationPage(s.lang); },
    }));
  }

  /** The languages the open page lacks; none without a page or a second tree. */
  private async missingTranslations(langs: SiteLanguagesInfo): Promise<TranslationSlot[]> {
    const root = this.deps.projectRoot();
    const rel = this.deps.currentFilePath();
    if (!root || !rel) return [];
    const slots = await translationSlots(rel, langs, this.deps.languages, root);
    return slots.filter((s) => !s.exists);
  }

  /** Manage Templates… → one submenu per template → Rename… / Folder… / Delete.
   *  Depth is capped here at two below the root; nothing to manage means no row. */
  private manageTemplatesLevel(templates: TemplateMeta[]): CtxMenuEntry[] {
    const commands = this.deps.templates;
    if (!commands || templates.length === 0) return [];
    return [{
      kind: 'submenu',
      label: t('tree.manage_templates'),
      id: 'manage-templates',
      entries: templates.map((meta): CtxMenuEntry => ({
        kind: 'submenu',
        label: templateRowLabel(meta),
        id: `manage-template:${meta.id}`,
        entries: [
          { label: t('tree.rename'), id: 'rename-template', action: (ev) => { promptRenameTemplate(commands, meta, ev); } },
          { label: t('tree.template_folder'), id: 'template-folder', action: (ev) => { promptTemplateFolder(commands, meta, ev); } },
          ...(meta.folder === null ? [] : [{ label: t('tree.template_folder_clear'), id: 'template-folder-clear', action: () => { clearTemplateFolder(commands, meta); } }]),
          { label: t('tree.delete'), id: 'delete-template', danger: true, action: () => { void deleteTemplate(commands, meta); } },
        ],
      })),
    }];
  }

  // ── Session lifecycle (notifications from editor-main) ───────────────────

  /** Call at the end of every successful loadFile. Activates a pending
   *  session when its file finished opening; ends the session when a
   *  DIFFERENT file was opened. EntryId comparison, never path strings. */
  onFileLoaded(relPath: string): void {
    const s = this.session;
    if (!s) return;
    const id = this.deps.upsert(relPath);
    if (id === s.entryId) {
      if (s.phase === 'pending') {
        s.phase = 'title-only';
        this.activateUi();
      }
    } else {
      this.endSession();
    }
  }

  /** From the isNewFile focus site in the editor:file-selected handler: when
   *  this open belongs to the session, the caret goes to the (empty) heading
   *  instead of the CM6 body. Returns true when the heading took it. */
  consumeHeadingFocus(relPath: string): boolean {
    const s = this.session;
    if (!s || s.phase !== 'title-only') return false;
    if (this.deps.upsert(relPath) !== s.entryId) return false;
    const headingEl = document.getElementById('editor-heading');
    if (!headingEl || headingEl.dataset.empty === 'true') return false;
    headingEl.focus();
    return true;
  }

  /** First user body edit ends the session: the status line hides and the
   *  filename stops following the title (spec: after the first body edit the
   *  filename is the author's). */
  onBodyEdited(): void {
    const s = this.session;
    if (!s || s.phase !== 'title-only') return;
    const current = this.deps.currentFilePath();
    if (current !== null && this.deps.upsert(current) === s.entryId) {
      this.endSession();
    }
  }

  /** Whether a heading commit on the CURRENT file should route through the
   *  session (bypassing routeHeadingCommit's source-based routing). */
  isActiveForCurrent(): boolean {
    const s = this.session;
    if (!s || s.phase !== 'title-only') return false;
    const current = this.deps.currentFilePath();
    return current !== null && this.deps.upsert(current) === s.entryId;
  }

  /**
   * Session heading commit: rename the still-title-only file to the generated
   * name via the tree's rename machinery (renameEntryWithRefs + lifecycle).
   * Empty title keeps the current untitled name — never an error. With the
   * date convention the slugged filename no longer carries the title text, so
   * `title:` frontmatter is written to keep the H1 the author's words.
   */
  async commitTitle(newText: string): Promise<{ ok: true } | { ok: false; error: string }> {
    const s = this.session;
    if (!s || s.phase !== 'title-only') return { ok: true };
    const currentPath = this.deps.pathOf(s.entryId);
    if (!currentPath) return { ok: true };
    // A translation keeps its original's file name; its title is written, not renamed from.
    if (s.translation) {
      const title = newText.trim();
      if (title) this.deps.setTitleField(title);
      return { ok: true };
    }

    const gen = generatedBasename(newText, { datePrefix: s.datePrefix, today: s.today });
    if (gen === null) {
      // Cleared title: keep the file at its current name; status shows it.
      this.renderStatus(s.currentName);
      return { ok: true };
    }
    if (gen !== s.currentName) {
      const root = this.deps.projectRoot();
      const newPath = siblingPath(currentPath, gen);
      this.deps.registerPendingRename(currentPath, newPath);
      const result = await this.deps.renameEntry(
        toAbsolute(currentPath, root),
        toAbsolute(newPath, root),
      );
      if (result.status !== 'ok') return { ok: false, error: result.error };
      s.currentName = gen;
    }
    if (s.datePrefix) this.deps.setTitleField(newText.trim());
    this.renderStatus(s.currentName);
    return { ok: true };
  }

  /** Undo: return to the previously open file and delete the created page
   *  (OS trash). The only deleter — abandonment never garbage-collects. */
  async undo(): Promise<void> {
    const s = this.session;
    if (!s) return;
    this.endSession();
    const root = this.deps.projectRoot();
    const currentPath = this.deps.pathOf(s.entryId);
    if (s.prevPath) {
      document.dispatchEvent(new CustomEvent('editor:file-selected', {
        detail: { filePath: s.prevPath },
      }));
    }
    if (currentPath) {
      const result = await this.deps.deleteEntry(toAbsolute(currentPath, root));
      if (result.status !== 'ok') {
        showToast({ what: t('tree.create_failed').replace('{error}', result.error), severity: 'Blocking' });
      }
    }
    await this.deps.fileTree()?.refreshAndRender();
    if (!s.prevPath) {
      // Nothing to return to — land on the destination folder's surface.
      const absDir = s.dir === PROJECT_ROOT_REL ? root : toAbsolute(s.dir, root);
      document.dispatchEvent(new CustomEvent('editor:folder-navigate', {
        detail: { folderPath: absDir, folderName: s.dirName },
      }));
    }
  }

  // ── UI ───────────────────────────────────────────────────────────────────

  private activateUi(): void {
    const s = this.session;
    if (!s) return;
    const headingEl = document.getElementById('editor-heading');
    if (headingEl) {
      // Empty heading + placeholder: the file is untitled, so the H1 starts
      // blank ("Untitled" as ghost text) rather than echoing the stem.
      headingEl.dataset.createPlaceholder = t('create.untitled_placeholder');
      this.deps.setHeadingSaved('');
    }
    this.renderStatus(s.currentName);
  }

  private endSession(): void {
    this.session = null;
    const headingEl = document.getElementById('editor-heading');
    // Keep the 'Untitled' ghost while the heading is still empty (the file is
    // still untitled — a blank H1 strip would say nothing); the attribute is
    // inert once the heading has text (the ::before only applies to :empty).
    if (headingEl && (headingEl.textContent ?? '').trim() !== '') {
      delete headingEl.dataset.createPlaceholder;
    }
    const statusEl = document.getElementById('moss-create-status');
    if (statusEl) {
      statusEl.hidden = true;
      statusEl.textContent = '';
    }
  }

  /** "New page in {folder} · {filename} · Undo" under the editor — the spec's
   *  visibility guarantee for date-prefix inference. */
  private renderStatus(filename: string): void {
    const s = this.session;
    const statusEl = document.getElementById('moss-create-status');
    if (!s || !statusEl) return;
    statusEl.textContent = '';
    const text = document.createElement('span');
    appendInterpolatedBold(text, t('create.status_new_page_in'), s.dirName);
    const fname = document.createElement('code');
    fname.className = 'create-status-fname';
    fname.textContent = filename;
    const undoBtn = document.createElement('button');
    undoBtn.type = 'button';
    undoBtn.className = 'moss-link';
    undoBtn.textContent = t('create.undo');
    undoBtn.addEventListener('click', () => { void this.undo(); });
    statusEl.append(text, sepDot(), fname, sepDot(), undoBtn);
    statusEl.hidden = false;
  }
}

/** The "Creates in <b>folder</b>" note — the one owner for the ＋▾ menu and
 *  the tree's empty-area menu (whitespace shows no target row). */
export function destinationNote(dirName: string): CtxMenuNote {
  return { kind: 'note', render: (host) => { appendInterpolatedBold(host, t('create.menu_hint'), dirName); } };
}

/** Fill `{folder}` in a localized template with a BOLD element (safe DOM
 *  construction — folder names are user data, never innerHTML). */
function appendInterpolatedBold(host: HTMLElement, template: string, value: string): void {
  const [pre, post] = template.split('{folder}');
  if (pre) host.append(pre);
  const b = document.createElement('b');
  b.textContent = value;
  host.append(b);
  if (post) host.append(post);
}

function sepDot(): HTMLElement {
  const el = document.createElement('span');
  el.className = 'create-status-sep';
  el.textContent = '·';
  el.setAttribute('aria-hidden', 'true');
  return el;
}
