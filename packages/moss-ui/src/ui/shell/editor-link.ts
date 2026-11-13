/**
 * What the preview and the editor say to each other: page switches, reveals,
 * scroll sync in both directions, and the preview's click and context-menu
 * messages. The host says only what this cannot see for itself: whether the
 * editor is open, whether it is full width, and how to open a target.
 */
import type { Takeover } from '@symbiosis-lab/moss-ui';
import { debug, warn } from "../log";
import { host as uiHost } from "../host";
import { engine } from '../engine';
import { onMossEvent } from "../utils/moss-events";
import { pageSource } from '../preview/page-source';
import type { PreviewUrlChangedEvent } from '../context/event-bus';
import { isRootUrlPath } from '../utils/url-path';
import type { ResolvedNode } from '../editor/session/node-class';
import { normalizePath, toRelative } from '../editor/path-domain';
import { StaleSourceRefresh } from '../editor/session/stale-source-refresh';
import { SOURCE_PENDING_EVENT, type SourcePendingPayload } from '../editor/session/no-content-pane';
import { cloudSync } from '../preview/cloud/cloud-sync-store';
import { extractDisplayPath } from '../preview/path-utils';
import { sameUrlPathname } from '../preview/navigation-manager';
import { isPreviewCtxMessage, type PreviewCtxPayload, type PreviewSourceTarget } from '../preview/preview-context-menu';
import type { PreviewManager } from '../preview/preview-manager';

/**
 * How long each direction of the link holds off after it acts, in ms.
 * `pageSwitch`: scroll sync and source targets wait while the frame settles on
 * a new page. `fieldReveal`: a preview click reveals the chip, and the chip
 * opening would reveal the preview element back; the direction the user did
 * not ask for yields for the round trip. `scrollSync`: one side's scroll does
 * not echo back from the other.
 */
const SETTLE_MS = { pageSwitch: 300, fieldReveal: 600, scrollSync: 400 } as const;
type Hold = keyof typeof SETTLE_MS;

/** A field name lands in a CSS selector; moss annotates only plain identifiers. */
const FIELD_NAME = /^[A-Za-z0-9_-]+$/;

export interface EditorLinkHost {
  preview(): PreviewManager | null;
  /** The editor is the open view. */
  editorIsOpen(): boolean;
  editorIsFullWidth(): boolean;
  folderPath(): string | null;
  /** Open the editor at a source target; another file's target names its `path`. */
  openTarget(target: PreviewSourceTarget): void;
  openContextMenu(ctx: PreviewCtxPayload): void;
}

export class EditorLink {
  /** The previewed page's answer as an editor node, or null while it has none. */
  get node(): ResolvedNode | null {
    const page = pageSource.current;
    return page ? pageSource.node(page) : null;
  }

  /** Asks the page's lookup again when a file it waits on finishes downloading. */
  readonly staleSource: StaleSourceRefresh;

  /**
   * The page whose answer the editor waits on: it has had none for it yet, or
   * a pending one. Only then may a re-ask of the page on screen reach it.
   */
  private waitingOn: string | null = null;
  /** The file the editor last loaded, relative to the folder; null until it says. */
  private editorFile: string | null = null;
  private unsubscribePage: (() => void) | null = null;

  disconnect(): void { this.unsubscribePage?.(); this.unsubscribePage = null; }

  /** When each hold ends (`Date.now()` ms). */
  private readonly holdUntil: Record<Hold, number> = { pageSwitch: 0, fieldReveal: 0, scrollSync: 0 };
  private flashedFieldSelector: string | null = null;

  constructor(private readonly host: EditorLinkHost) {
    this.staleSource = new StaleSourceRefresh({
      refreshPage: (urlPath) => pageSource.refresh(urlPath),
      liveUrlPath: () => this.liveUrlPath(),
      editorIsActive: () => host.editorIsOpen(),
      applyNode: (node) => uiHost.bus.emit('editor:load-source', node),
    });
  }

  /** Starts listening. Call once, where the shell wires the editor. */
  connect(): void {
    this.unsubscribePage = pageSource.subscribe((page) => this.onPreviewPage(page));
    // The editor asks for the page it opened on; a cold boot can miss an answer published before it listened.
    uiHost.bus.on('editor:request-page', () => {
      if (!this.host.editorIsOpen()) return;
      const urlPath = this.liveUrlPath();
      this.waitingOn = urlPath;
      void pageSource.ensure(urlPath);
      const page = pageSource.current;
      if (page?.urlPath === urlPath) this.forward(page);
    });

    uiHost.bus.on<{ path: string }>('editor:file-ready', (payload) => { this.editorFile = payload.path; });

    uiHost.bus.on<{ urlPath: string; awaitingBuild?: boolean }>('editor:navigate-preview', (payload) => {
      const preview = this.host.preview();
      this.hold('pageSwitch');
      // A page created moments ago has no build yet: cover the 404 before navigating, or it paints first.
      if (payload.awaitingBuild) preview?.armNewPageWait(payload.urlPath);
      // Registered before navigating, so the bridge's bounce-back reads as an acknowledgment.
      preview?.markPendingEcho(payload.urlPath);
      preview?.navigateToPage(payload.urlPath);
    });

    // The shell owns the one cloud listener, so the editor says when its "still
    // downloading" pane is up (arm the arrival retry) and when it is gone (stop).
    // The pane's identity is the live URL when it came from resolving a page and
    // the path when the editor opened the file directly; both are passed.
    uiHost.bus.on<SourcePendingPayload>(SOURCE_PENDING_EVENT, (payload) => {
      // Any other surface means the editor moved on from the page's answer.
      if (!payload.pending || payload.sourcePath !== null) this.waitingOn = null;
      if (!payload.pending) {
        this.staleSource.disarm();
        return;
      }
      this.staleSource.armForArrival(this.liveUrlPath(), payload.sourcePath ?? null);
    });
    cloudSync.subscribe((change) => {
      if (change.filesArrived) void this.staleSource.onFilesArrived();
    });
    // A folder's listing can settle with no file arriving; a previous folder has no say here.
    void onMossEvent('SourceStructureSettled', (event) => {
      if (event.payload.outcome === 'Ready' && event.payload.folder_path === this.host.folderPath()) {
        void this.staleSource.onFilesArrived();
      }
    });

    uiHost.bus.on<{ language: string | null }>('editor:reveal-slot', (payload) => void this.revealSlot(payload.language));

    // Suppressed while a preview click is what opened the chip.
    uiHost.bus.on<{ field: string }>('editor:reveal-field-in-preview', (payload) => {
      if (this.held('fieldReveal') || !FIELD_NAME.test(payload.field)) return;
      void this.host.preview()?.revealSource(`[data-source-fm="${payload.field}"]`);
    });

    // Hovering a chip flashes what it renders as, without scrolling; `null` clears the wash.
    uiHost.bus.on<{ field: string | null }>('editor:flash-field-in-preview', (payload) => {
      const preview = this.host.preview();
      const field = payload.field;
      if (this.flashedFieldSelector) {
        void preview?.clearFlashSource(this.flashedFieldSelector);
        this.flashedFieldSelector = null;
      }
      if (field === null || !FIELD_NAME.test(field)) return;
      this.flashedFieldSelector = `[data-source-fm="${field}"]`;
      void preview?.flashSource(this.flashedFieldSelector);
    });

    uiHost.bus.on<{ type: string; line: number; anchorId?: string; sourceFile?: string; atTop?: boolean; atBottom?: boolean; onlyIfOffscreen?: boolean }>(
      'scroll-sync:editor-to-preview',
      (payload) => this.scrollPreviewToEditor(payload),
    );

    // An editor that has just opened, loaded or resized asks where the preview
    // is; `answering` lets it drop a reply the user beat.
    uiHost.bus.on<{ sourceFile: string }>('scroll-sync:request-preview-position', async (payload) => {
      const preview = this.host.preview();
      const active = preview?.getActivePage();
      if (this.host.editorIsFullWidth() || !active?.sourceFile || active.sourceFile !== payload.sourceFile) return;
      const position = await preview?.readScrollPosition();
      if (position) uiHost.bus.emit('scroll-sync:preview-to-editor', { type: 'source-line', ...position, answering: payload.sourceFile });
    });

    window.addEventListener('message', (event) => {
      const data = event.data;
      if (data?.type === 'moss-scroll-position') this.scrollEditorToPreview(data);
      else if (data?.type === 'moss-source-target') this.onSourceTarget(data);
      // Not gated on the editor: copying a link needs none.
      else if (isPreviewCtxMessage(data)) this.host.openContextMenu(data);
    });
  }

  private liveUrlPath(): string {
    const liveUrl = this.host.preview()?.getCurrentUrl() ?? null;
    return liveUrl ? extractDisplayPath(liveUrl) : '/';
  }

  /** The open folder changed: a pane armed for the old folder's file must not reopen it in the new one. */
  folderChanged(): void {
    this.staleSource.disarm();
    this.waitingOn = null;
    this.editorFile = null;
  }

  /**
   * The node to open the editor on for the previewed page, as the native open
   * carries it, and the start of that page's lookup. A page with no answer yet
   * opens bare; the editor then asks for the answer.
   */
  openNode(): { sourcePath: string | null; isRoot: boolean; takeover: Takeover | null } {
    const urlPath = this.liveUrlPath();
    void pageSource.ensure(urlPath);
    const page = pageSource.current?.urlPath === urlPath && !pageSource.current.pending ? pageSource.current : null;
    return { sourcePath: page?.sourcePath ?? null, isRoot: isRootUrlPath(urlPath), takeover: page?.takeover ?? null };
  }

  /**
   * A target with no `path` is a place in the previewed page, and `null` is the
   * page itself. While the open editor shows another file, name the page's
   * file, so the target opens it instead of landing in the file on screen.
   */
  located(target: PreviewSourceTarget | null): PreviewSourceTarget | null {
    const page = this.node?.sourcePath;
    if (target?.path || target?.kind === 'none' || !page || this.editorFile === null || !this.host.editorIsOpen()) return target;
    const path = toRelative(page, this.host.folderPath() ?? '');
    if (normalizePath(path) === normalizePath(this.editorFile)) return target;
    return { ...(target ?? { kind: 'page' }), path };
  }

  private hold(kind: Hold): void {
    this.holdUntil[kind] = Date.now() + SETTLE_MS[kind];
  }

  private held(kind: Hold): boolean {
    return Date.now() < this.holdUntil[kind];
  }

  private onPreviewPage(page: PreviewUrlChangedEvent): void {
    if (!this.host.editorIsOpen() || page.urlPath !== this.liveUrlPath()) return;
    this.hold('pageSwitch');
    // The page on screen answered again: that need not be the file the editor
    // has open (a rebuild after the user created and opened a new file).
    if (page.isReload && this.waitingOn !== page.urlPath) return;
    this.forward(page);
  }

  private forward(page: PreviewUrlChangedEvent): void {
    this.waitingOn = page.pending ? page.urlPath : null;
    uiHost.bus.emit('editor:load-source', pageSource.node(page));
  }

  /**
   * The user opened a slot file (`footer.md`), which is no page of its own:
   * show the thing they are editing. Move to another language tree only on
   * positive evidence that the preview is in the wrong one; no active page, an
   * unresolved source or a failed probe count as none, because moving costs the
   * scroll position every time and staying costs at worst the other tree's footer.
   */
  private async revealSlot(wanted: string | null): Promise<void> {
    const preview = this.host.preview();
    if (!preview) return;
    const active = preview.getActivePage();
    let differentTree = false;
    if (active?.sourceFile) {
      try {
        const role = await engine.describeSourceRole(active.sourceFile);
        if (role.status === 'ok') differentTree = role.data.language !== wanted;
      } catch (e) {
        warn(`[app] describeSourceRole failed for the active page: ${e}`);
      }
    }
    if (!differentTree) {
      void preview.revealFooter();
      return;
    }
    const treeHome = wanted ? `/${wanted}/` : '/';
    preview.armFooterReveal(treeHome);
    preview.markPendingEcho(treeHome);
    preview.navigateToPage(treeHome);
  }

  private scrollPreviewToEditor(msg: { line: number; anchorId?: string; sourceFile?: string; atTop?: boolean; atBottom?: boolean; onlyIfOffscreen?: boolean }): void {
    if (this.held('scrollSync') || this.host.editorIsFullWidth()) return;
    const preview = this.host.preview();
    const { line, anchorId, sourceFile, atTop, atBottom, onlyIfOffscreen } = msg;
    // The active page's source resolves after navigation and is null meanwhile;
    // a stamped message is dropped until it can be checked. Unstamped ones apply.
    const active = preview?.getActivePage();
    if (sourceFile) {
      if (!active || active.sourceFile === null) {
        debug(`[app] Dropped editor→preview scroll-sync during resolution window: msg=${sourceFile}`);
        return;
      }
      if (sourceFile !== active.sourceFile) {
        debug(`[app] Dropped editor→preview scroll-sync: msg=${sourceFile} active=${active.sourceFile}`);
        return;
      }
    }
    this.hold('scrollSync');
    preview?.claimScrollForEditor();
    preview?.scrollToSourceLine(line, anchorId ?? null, atTop ?? false, atBottom ?? false, onlyIfOffscreen ?? false);
    preview?.setLastEditorPosition(line, anchorId ?? null, atTop ?? false, atBottom ?? false);
  }

  private scrollEditorToPreview(data: { url?: unknown; line: number; atTop?: unknown; atBottom?: unknown }): void {
    if (this.held('scrollSync') || this.held('pageSwitch') || this.host.editorIsFullWidth()) return;
    if (!this.host.editorIsOpen()) return;
    // Pathnames only: a hash or a cache-bust query must not defeat the match.
    const active = this.host.preview()?.getActivePage();
    const msgUrl = typeof data.url === 'string' ? data.url : undefined;
    if (msgUrl && active?.url && !sameUrlPathname(msgUrl, active.url)) {
      debug(`[app] Dropped preview→editor scroll-sync: msg=${msgUrl} active=${active.url}`);
      return;
    }
    this.hold('scrollSync');
    uiHost.bus.emit('scroll-sync:preview-to-editor', {
      type: 'source-line',
      line: data.line,
      atTop: data.atTop === true,
      atBottom: data.atBottom === true,
    });
  }

  /**
   * A click or selection in the preview:
   * a single click orients the editor, a double click commits to the target.
   */
  private onSourceTarget(data: { target?: PreviewSourceTarget; interaction?: string }): void {
    if (this.held('pageSwitch')) return;
    const interaction = data.interaction;
    const target = this.located(data.target ?? null);
    if (!target) return;
    // A target in another file: only a double click commits to it.
    if (typeof target.path === 'string') {
      if (interaction === 'dblclick') this.host.openTarget(target);
      return;
    }
    if (!this.host.editorIsOpen()) return;
    switch (target.kind) {
      case 'body-line':
      case 'body-range': {
        const startLine = target.kind === 'body-line' ? target.line : target.startLine;
        const endLine = target.kind === 'body-line' ? target.line : target.endLine;
        if (interaction === 'select' && target.text) uiHost.bus.emit('editor:select-text', { text: target.text, startLine, endLine });
        else if (interaction === 'dblclick') this.host.openTarget(target);
        else if (interaction === 'click') uiHost.bus.emit('editor:reveal-line', { startLine, endLine });
        break;
      }
      case 'frontmatter':
        // Clicks only: the bridge also posts while the user drags across the
        // byline to copy it, and answering that would pull focus mid-selection.
        if (interaction === 'click' || interaction === 'dblclick') {
          this.hold('fieldReveal');
          if (interaction === 'dblclick') this.host.openTarget(target);
          else uiHost.bus.emit('editor:reveal-field', { field: target.field });
        }
        break;
      // 'none': template chrome, no source.
    }
  }
}
