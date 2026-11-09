/**
 * The tree's context menus — empty area, batch (N>1), folder, file — and the
 * URL-import popover the folder menu opens. Templates are created and managed
 * from the titlebar ＋▾ menu (create-flow.ts); the tree only offers "Save as
 * Template…" on a markdown file.
 *
 * Built on the shared primitive (shared/ui/ctx-menu.ts): positioning,
 * viewport clamping, outside-click AND Escape dismissal, menu ARIA roles,
 * roving focus, `.ctx-kbd` chips. This module only decides what each menu
 * CONTAINS; how a menu behaves is the primitive's business. Items don't
 * close the menu themselves — the primitive closes before running an action.
 *
 * Constructed
 * once per mount; the controller's AbortSignal closes whatever is open.
 */

import { t } from "../../i18n/index";
import { host as uiHost } from "../../host";
import {
  openCtxMenu,
  joinCtxGroups,
  type CtxMenuEntry,
  type CtxMenuHandle,
} from "../../shared/ui/ctx-menu";
import { openInputPopover } from "../../shared/ui/input-popover";
import { toAbsolute, PROJECT_ROOT_REL, resolveCreateTarget, displayName } from '../path-domain';
import { destinationNote } from '../create-flow';
import { promptSaveAsTemplate, type TemplateCommands } from '../template-menu';
import type { LanguageSource } from '../languages';
import { toggleInternalState, getInternalToggleState } from '../internal-toggle';
import type { EntryRecord } from '../entry-registry';
import type { TreeOps } from './ops';
import type { FolderDerivation, FolderDerivationChoice } from '@symbiosis-lab/moss-ui';
import { problemsOf } from './derivations';

type Point = { clientX: number; clientY: number };

/** The controller interface used by the menus; it exposes only their coordination needs. */
export interface MenuHost {
  readonly ops: TreeOps;
  /** "Save as Template…" is the tree's one template action; the rest live on the ＋▾ menu. */
  readonly commands: Pick<TemplateCommands, 'createTemplate'> & LanguageSource;
  /** False for a loose document's folder (document mode). Templates,
   *  publish history, URL import and home files are a site's, so its menus
   *  leave those rows out rather than offer what can only fail. */
  readonly site: boolean;
  projectRoot(): string;
  /** The tree's root element — the events the menus dispatch bubble from it. Null while unmounted. */
  container(): HTMLElement | null;
  /** The breadcrumb leaf, the create flow's destination. */
  createAnchor(): { path: string; isDir: boolean } | null;
  /** The registry record at `path`, in either Unicode spelling. */
  recordAt(path: string): EntryRecord | null;
  /** The folder's elected home file, or null. */
  homeEntry(folderPath: string): EntryRecord | null;
  /** How many entries are selected — one picks the single menu, more the batch menu. */
  selectionSize(): number;
  /** The selected entries' registry paths. */
  selectionPaths(): string[];
  startCreate(dir: string, type: 'file' | 'folder'): Promise<void>;
  renameEntry(entry: EntryRecord): void;
  /** The folder's derivation (tree/derivations.ts), or null when none applies. */
  derivationAt(folderPath: string): FolderDerivation | null;
  /** Write the author's choice for the folder. */
  setDerivation(folderPath: string, choice: FolderDerivationChoice): void;
  /** "Versions…" — opens the Versions panel. One label for every scope
   *  Scope is
   *  already fully determined by what was clicked, so the menu label
   *  repeating it back ("Folder versions…" / "Site versions…") added
   *  nothing `openVersions`'s own dispatch doesn't already carry. */
  openVersions(scope: 'page' | 'folder' | 'site', path: string): void;
}

export class TreeMenu {
  // The shared primitive owns listeners/dismissal; the tree only tracks the
  // handle so teardown can close it.
  private active: CtxMenuHandle | null = null;

  constructor(private readonly host: MenuHost, signal: AbortSignal) {
    signal.addEventListener('abort', () => this.close(), { once: true });
  }

  close(): void {
    this.active?.close();
    this.active = null;
  }

  /** Open `entries` as the tree's context menu at the given viewport point.
   *  `pos` is a MouseEvent or a synthesized point (Shift+F10 at the focused
   *  row); `focusFirst` puts roving focus on the first item for keyboard
   *  invocations. */
  private mountCtx(
    entries: CtxMenuEntry[],
    pos: Point,
    opts?: { focusFirst?: boolean },
    log?: { surface: string; target?: string },
  ): void {
    this.close();
    this.active = openCtxMenu({
      entries,
      position: { kind: 'point', x: pos.clientX, y: pos.clientY },
      focusFirst: opts?.focusFirst,
      logSurface: log?.surface,
      logTarget: log?.target,
      onClose: () => { this.active = null; },
    });
  }

  /**
   * Empty-area (panel whitespace) menu: the same Add-content actions the ＋▾
   * menu offers, targeting the browsed folder. Creation itself is OWNED by
   * create-flow.ts (untitled-first newPage, inline-named newFolder) — the
   * tree only dispatches the same request events the shell's ⌘N/⇧⌘N
   * forwarding uses; editor-main routes them to the one CreateFlow instance.
   * A trailing note names the destination, since whitespace shows no target row.
   */
  showEmptyAreaCtx(e: Point, opts?: { focusFirst?: boolean }): void {
    const anchor = this.host.createAnchor();
    const dir = anchor ? resolveCreateTarget(anchor.path, anchor.isDir) : PROJECT_ROOT_REL;
    const dirName = displayName(dir, this.host.projectRoot());
    this.mountCtx(joinCtxGroups([
      [
        {
          label: t('tree.new_page'),
          id: 'new-page',
          kbd: '⌘N',
          action: () => { document.dispatchEvent(new CustomEvent('editor:request-new-page', { bubbles: true })); },
        },
        {
          label: t('tree.new_folder'),
          id: 'new-folder',
          kbd: '⇧⌘N',
          action: () => { document.dispatchEvent(new CustomEvent('editor:request-new-folder', { bubbles: true })); },
        },
      ],
      [destinationNote(dirName)],
    ]), e, opts, { surface: 'tree-empty-area' });
  }

  /**
   * The ONE N>1 batch menu — file, folder and mixed selections all get the
   * same items (single/multi symmetry).
   * "Open all" counts only the FILES in the selection (folders don't open
   * in the editor) and is omitted when the selection holds none.
   */
  private showMultiCtx(e: Point, opts: { focusFirst?: boolean } | undefined, surface: string): void {
    const { host } = this;
    const { ops } = host;
    const N = host.selectionSize();
    const selectedPaths = host.selectionPaths();
    const filePaths = selectedPaths.filter(p => host.recordAt(p)?.isDir === false);
    this.mountCtx(joinCtxGroups([
      filePaths.length > 0 ? [
        {
          label: t('tree.open_all_n', { n: filePaths.length }),
          id: 'open-all',
          action: () => {
            for (const p of filePaths) {
              host.container()?.dispatchEvent(new CustomEvent('editor:file-selected', {
                bubbles: true,
                detail: { filePath: p },
              }));
            }
          },
        },
      ] : [],
      [
        // "Move to…" only where the host has a folder picker.
        ...(uiHost.pickFolder ? [{ label: t('tree.move_n_items', { n: N }), id: 'move-to', action: () => { void ops.promptMoveEntries(selectedPaths); } }] : []),
        { label: t('tree.delete_n_items', { n: N }), id: 'delete', action: () => { void ops.deleteEntries(selectedPaths); } },
      ],
      // Reveal first in Finder, where the host has a file manager.
      uiHost.reveal ? [
        {
          label: t('tree.reveal_first', { label: t('tree.reveal') }),
          id: 'reveal',
          action: () => { if (selectedPaths.length > 0) void ops.revealEntry(selectedPaths[0]); },
        },
      ] : [],
      [
        // Copy paths (N) — OS-absolute, one per line.
        {
          label: t('tree.copy_paths', { n: N }),
          id: 'copy-path',
          action: () => {
            void ops.copyToClipboard(
              selectedPaths.map(p => toAbsolute(p, host.projectRoot())).join('\n'),
            );
          },
        },
      ],
    ]), e, opts, { surface, target: `selection:${N} items` });
  }

  /**
   * Add-content pair shared by the folder and file context menus: New Page,
   * New Folder — both create into `dir`. The folder menu appends Import from
   * URL itself. The empty-area menu does NOT use this: its New Page / New
   * Folder route through the anchor-resolving `editor:request-new-*` events
   * rather than a caller-supplied `dir`, so it isn't the same action shape.
   */
  private buildCreateActionsGroup(dir: string): CtxMenuEntry[] {
    return [
      { label: t('tree.new_page'), id: 'new-page', kbd: '⌘N', action: () => { void this.host.startCreate(dir, 'file'); } },
      { label: t('tree.new_folder'), id: 'new-folder', kbd: '⇧⌘N', action: () => { void this.host.startCreate(dir, 'folder'); } },
    ];
  }

  /**
   * Filesystem group shared by the folder and file menus: Reveal, and "Copy
   * path" as the OS-absolute filesystem path — what a user pastes into a
   * terminal, a file dialog, or another app. The registry stores relative;
   * wrap here.
   */
  private filesystemGroup(path: string): CtxMenuEntry[] {
    const { ops } = this.host;
    return [
      // Where the host has a file manager.
      ...(uiHost.reveal ? [{ label: t('tree.reveal'), id: 'reveal', action: () => { void ops.revealEntry(path); } }] : []),
      { label: t('tree.copy_path'), id: 'copy-path', action: () => { void ops.copyToClipboard(toAbsolute(path, this.host.projectRoot())); } },
    ];
  }

  showFolderCtx(e: Point, folderPath: string, opts?: { focusFirst?: boolean }, surface = 'tree-row'): void {
    const { host } = this;
    const { ops } = host;

    if (host.selectionSize() > 1) {
      this.showMultiCtx(e, opts, surface);
      return;
    }

    // ── Single-folder context menu ──────────────────────────────────────
    //
    // Items are organized into named groups. A group
    // that has nothing to show (e.g. "Organize" on the project root) stays
    // empty and joinCtxGroups omits it — a single separator lands between
    // any two groups that BOTH rendered something, so the double-separator
    // bug (an unconditional separator next to another once Rename/Duplicate/
    // Move-to were skipped for isRoot) cannot recur.

    // Rename / Duplicate / Move-to / Delete are all omitted on the project
    // root: it's the user's workspace folder on disk, owned by the OS, not
    // by moss (there's also no parent to move the root into).
    const isRoot = folderPath === PROJECT_ROOT_REL;

    // Group — Add content: New Page, New Folder, Import from URL. All three
    // ultimately create a new page in this folder; they differ only in the
    // content source (blank, blank folder, or scraped from a URL). The kbd
    // chips teach the create-flow bindings (⌘N/⇧⌘N target the breadcrumb
    // anchor's destination — same folder when it's the one browsed).
    const absFolder = isRoot ? host.projectRoot() : toAbsolute(folderPath, host.projectRoot());
    const addContentGroup: CtxMenuEntry[] = [
      ...this.buildCreateActionsGroup(folderPath),
      ...(host.site && uiHost.importSite ? [{ label: t('tree.import_from_url'), id: 'import-from-url', action: (ev: MouseEvent) => { this.showUrlImportOverlay(absFolder, ev); } }] : []),
    ];

    // Create home file — ONLY for a folder that has no home page yet (root
    // included; the backend's is_home election uses the real root basename).
    // Note a folder whose only doc is e.g. about.md HAS a home (the election's
    // alphabetical fallback — the build serves it at the folder URL), so this
    // item correctly hides there. An explicit entry
    // point that mirrors the preview-nav "Create folder page" CTA; the actual
    // create-home action lives in editor-main (editor:create-home handler → the
    // shared takeOver), so this only dispatches through the tree's event
    // channel. folderName via displayName() so the root → `<rootBasename>.md`
    // and a subfolder → `<folderName>.md`.
    // Show/Stop showing the folder's files as pages: the same decision the hint
    // row offers, kept reachable (and reversible) after the row is gone. Only
    // for a folder with files some plugin can show.
    const derivation = host.derivationAt(folderPath);
    if (derivation !== null && derivation.count > 0) {
      const showing = derivation.decision === 'show';
      addContentGroup.push({
        label: t(showing ? 'tree.derive_menu_stop' : 'tree.derive_menu_show', { noun: derivation.noun_other }),
        id: 'derive-pages',
        action: () => { host.setDerivation(folderPath, showing ? 'keep' : { show: derivation.derivation_id }); },
      });
    }

    if (host.site && host.homeEntry(folderPath) === null) {
      addContentGroup.push({
        label: t('tree.create_home_file'),
        id: 'create-home-file',
        action: () => {
          host.container()?.dispatchEvent(new CustomEvent('editor:create-home', {
            bubbles: true,
            detail: {
              folderPath: toAbsolute(folderPath, host.projectRoot()),
              folderName: displayName(folderPath, host.projectRoot()),
            },
          }));
        },
      });
    }

    // Group — Organize: Rename, Duplicate, Move to… (WCAG 2.5.7 keyboard
    // equivalent of drag-to-tree). Omitted on the project root.
    const organizeGroup: CtxMenuEntry[] = [];
    if (!isRoot) {
      organizeGroup.push(
        {
          label: t('tree.rename'),
          id: 'rename',
          kbd: '↩',
          action: () => {
            const entry = host.recordAt(folderPath);
            if (entry) host.renameEntry(entry);
          },
        },
        { label: t('tree.duplicate'), id: 'duplicate', action: () => { void ops.duplicateEntry(folderPath); } },
        ...(uiHost.pickFolder ? [{ label: t('tree.move_to'), id: 'move-to', action: () => { void ops.promptMoveEntries([folderPath]); } }] : []),
        ...(host.site ? [{ label: t('versions.menu'), id: 'versions', action: () => { host.openVersions('folder', folderPath); } }] : []),
      );
    }

    // Group — Filesystem: Reveal in Finder + Copy path.
    const filesystemGroup = this.filesystemGroup(folderPath);

    // Group — Delete: kept alone as its own trailing group, a deliberate,
    // common convention that isolates a destructive action from anything a
    // stray click could land on right after doing something else. Omitted
    // on the project root.
    const deleteGroup: CtxMenuEntry[] = [];
    if (!isRoot) {
      deleteGroup.push({ label: t('tree.delete'), id: 'delete', kbd: '⌘⌫', action: () => { void ops.deleteEntries([folderPath]); } });
    }

    // Group — View: Versions… (root only, since the root has no Organize
    // group of its own), then Show/Hide internal files.
    const viewGroup: CtxMenuEntry[] = [];
    if (isRoot) {
      viewGroup.push(
        ...(host.site ? [{ label: t('versions.menu'), id: 'versions', action: () => { host.openVersions('site', folderPath); } }] : []),
        {
          label: getInternalToggleState() ? t('tree.hide_internal') : t('tree.show_internal'),
          id: 'toggle-internal',
          action: () => { toggleInternalState(); },
        },
      );
    }

    this.mountCtx(
      joinCtxGroups([addContentGroup, organizeGroup, filesystemGroup, deleteGroup, viewGroup]),
      e,
      opts,
      { surface, target: `${isRoot ? 'root' : 'folder'}:${displayName(folderPath, host.projectRoot())}` },
    );
  }

  showFileCtx(e: Point, filePath: string, opts?: { focusFirst?: boolean }, surface = 'tree-row'): void {
    const { host } = this;
    const { ops } = host;

    if (host.selectionSize() > 1) {
      this.showMultiCtx(e, opts, surface);
      return;
    }

    // ── Single-entry file context menu ───────────────────────────────────
    //
    // Mirrors the single-folder menu's group anatomy (file/folder symmetry):
    // Add content (targeting the file's
    // PARENT folder — same actions a folder row offers into itself),
    // Organize (Rename/Duplicate/Save as Template…/Move to…), Filesystem,
    // Delete.
    const parentDir = resolveCreateTarget(filePath, false);

    // "Save as Template…" only exists for files that BECOME pages — markdown
    // sources; assets and internal files have no frontmatter to capture.
    const isMarkdown = /\.(md|markdown|mdown|mkd)$/i.test(filePath);
    const commands = host.commands;

    this.mountCtx(joinCtxGroups([
      this.buildCreateActionsGroup(parentDir),
      [
        {
          label: t('tree.rename'),
          id: 'rename',
          kbd: '↩',
          action: () => {
            const entry = host.recordAt(filePath);
            if (entry) host.renameEntry(entry);
          },
        },
        { label: t('tree.duplicate'), id: 'duplicate', action: () => { void ops.duplicateEntry(filePath); } },
        ...(isMarkdown && host.site ? [{
          label: t('tree.save_as_template'),
          id: 'save-as-template',
          action: (ev: MouseEvent) => {
            const isHome = host.homeEntry(parentDir)?.path === filePath;
            promptSaveAsTemplate(commands, { absPath: toAbsolute(filePath, host.projectRoot()), projectRoot: host.projectRoot(), isHome }, ev);
          },
        }] : []),
        // WCAG 2.5.7 keyboard equivalent of drag-to-tree — folder rows have it;
        // files reuse the same machinery.
        ...(uiHost.pickFolder ? [{ label: t('tree.move_to'), id: 'move-to', action: () => { void ops.promptMoveEntries([filePath]); } }] : []),
        ...(host.site ? [{ label: t('versions.menu'), id: 'versions', action: () => { host.openVersions('page', filePath); } }] : []),
      ],
      this.filesystemGroup(filePath),
      [
        { label: t('tree.delete'), id: 'delete', kbd: '⌘⌫', action: () => { void ops.deleteEntries([filePath]); } },
      ],
    ]), e, opts, { surface, target: `file:${displayName(filePath, host.projectRoot())}` });
  }

  /**
   * The hint row's "Why?": each file a deriver could not read and the reason,
   * in a small floating surface under the button. The ctx-menu primitive with
   * note entries, so dismissal (outside press, Escape) and viewport clamping
   * are the app's own, as for the "Creates in …" note.
   */
  showDerivationWhy(anchor: HTMLElement, d: FolderDerivation): void {
    this.close();
    this.active = openCtxMenu({
      entries: problemsOf(d).map(({ file, why }): CtxMenuEntry => ({
        kind: 'note',
        render: (host) => {
          const name = document.createElement('b');
          name.textContent = file;
          host.append(name, ` — ${why}`);
        },
      })),
      position: { kind: 'below', anchor, align: 'left' },
      // Pressing "Why?" again while open closes it rather than racing the
      // outside-press dismissal into a close-then-reopen.
      outsideAnchor: anchor,
      logSurface: 'tree-derivation-why',
      logTarget: `folder:${displayName(d.folder === '/' ? PROJECT_ROOT_REL : d.folder.replace(/\/$/, ''), this.host.projectRoot())}`,
      onClose: () => { this.active = null; },
    });
  }

  // ── URL Import Overlay ─────────────────────────────────────────────

  /**
   * URL-input popover that kicks off `startNativeImportTask` targeting
   * `folderAbsPath`. Progress routes to the action panel via the native PanelTask (same path
   * as the home-card URL card in folder-mode-handlers.ts).
   */
  private showUrlImportOverlay(folderAbsPath: string, e: MouseEvent): void {
    openInputPopover({
      label: t('tree.import_from_url'),
      at: { x: e.clientX, y: e.clientY },
      type: 'url',
      placeholder: 'https://example.com/post',
      validate: (url) => {
        try { new URL(url); return null; } catch { return t('onboard.flow.url_invalid'); }
      },
      onSubmit: (url) => { void this.host.ops.runUrlImport(url, folderAbsPath); },
    });
  }
}
