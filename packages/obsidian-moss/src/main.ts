import type { EditorState, Extension } from "@codemirror/state";
import { editorInfoField, Events, MarkdownView, Notice, Platform, Plugin } from "obsidian";
import { BlockSuggest } from "./syntax/block-suggest";
import { blockExtension } from "./syntax/cm-blocks";
import { renderBlock } from "./syntax/block-render";
import { registerEditorActions } from "./editor-actions";
import { showMossFields } from "./property-types";
import { frontmatterExtension } from "./cm-frontmatter";
import { initializeCore, MossSite } from "./core";
import { MossFieldSuggest } from "./field-suggest";
import { markRows, mossFolders } from "./folder-marks";
import { PagePreviewView, PAGE_PREVIEW_VIEW_TYPE } from "./page-preview-view";
import { siteSchema } from "./schema";
import { DEFAULT_SETTINGS, type MossSettings } from "./settings";
import { contextChanged, readSiteContext, siteOf, type OpenSite, type SiteNote } from "./site-context";
import { shortcodeExtension } from "./syntax/cm-shortcode";
import type { DesktopCommands } from "./desktop";

const NO_SITE = { files: [], lang: "en" };

export default class MossPlugin extends Plugin {
  settings: MossSettings = { ...DEFAULT_SETTINGS };
  /** One per moss folder in the vault; a note belongs to the nearest above it. */
  sites: OpenSite[] = [];
  /** What a note outside every moss folder previews with. */
  private plain!: OpenSite;
  private desktop?: DesktopCommands;
  private rebuildTimer?: ReturnType<typeof setTimeout>;
  private generation = 0;
  private disposed = false;
  private readonly aids = [
    frontmatterExtension((state) => this.noteOf(state)), shortcodeExtension((state) => this.noteOf(state)),
    blockExtension((markdown, doc, note) => renderBlock(note, this.app.vault.adapter, markdown, doc), (state) => this.noteOf(state)),
  ];
  /** The registered editor extensions: the aids in a moss folder, none elsewhere. */
  private editorAids: Extension[] = [];
  private marked: ReadonlySet<string> = new Set();
  /** The file tree draws rows as they scroll into view and reuses them; each row is marked as it arrives or changes path. */
  private readonly tree = new MutationObserver(() => markRows(document, this.marked));

  async onload(): Promise<void> {
    const saved = ((await this.loadData()) ?? {}) as Partial<MossSettings>;
    // Only the keys this version knows; a saved key from an older one is dropped.
    this.settings = { binaryPath: saved.binaryPath ?? DEFAULT_SETTINGS.binaryPath };
    initializeCore();
    const plain = new MossSite(JSON.stringify(NO_SITE));
    this.plain = { root: "/", site: plain, schema: siteSchema(plain) };
    this.registerEvent(this.app.workspace.on("layout-change", () => this.watchTree()));
    this.register(() => {
      this.tree.disconnect();
      markRows(document, new Set());
    });
    await this.rebuildContext();
    if (this.disposed) return;
    this.registerEditorExtension(this.editorAids);
    this.registerEditorSuggest(new MossFieldSuggest(this, (path) => this.siteOf(path)));
    this.registerEditorSuggest(new BlockSuggest(this.app));
    registerEditorActions(this);
    this.addCommand({
      id: "show-fields",
      name: "Show moss fields in Properties",
      callback: async () => {
        const file = this.app.workspace.getActiveFile();
        const schema = file && this.siteOf(file.path)?.schema;
        if (!schema) return void new Notice("No moss folder in this vault.");
        const added = await showMossFields(this.app, schema);
        if (added) new Notice(added.length ? `Typed ${added.length} fields.` : "Fields already typed.");
      },
    });
    this.registerView(PAGE_PREVIEW_VIEW_TYPE, (leaf) => new PagePreviewView(leaf, this));
    for (const event of ["create", "delete", "rename"] as const) {
      this.registerEvent((this.app.vault as Events).on(event, () => this.scheduleRebuild()));
    }
    // Hidden folders bypass Vault's file index, but still emit raw adapter events.
    this.registerEvent((this.app.vault as Events).on("raw", (path: unknown) => {
      if (typeof path !== "string") return;
      // A folder that gains or loses `.moss` gains or loses its site and mark.
      if (/(^|\/)\.moss(\/|$)/.test(path)) this.scheduleRebuild();
    }));
    this.app.workspace.onLayoutReady(() => { if (!this.disposed) this.scheduleRebuild(); });
    // The mobile UI (a phone, or desktop Obsidian emulating one) gets the page door, as no program runs on a phone.
    if (Platform.isDesktopApp && !Platform.isMobile) {
      const { DesktopCommands } = await import("./desktop");
      if (this.disposed) return;
      this.desktop = new DesktopCommands(this);
      this.desktop.load();
    } else {
      this.addCommand({
        id: "preview-page",
        name: "Preview this page",
        checkCallback: (checking) => {
          if (!this.canPreviewPage()) return false;
          if (!checking) void this.openPagePreview();
          return true;
        },
      });
      this.addRibbonIcon("globe", "Preview this page", () => {
        if (this.canPreviewPage()) void this.openPagePreview();
        else new Notice("Open a note to preview it.");
      });
    }
  }

  private scheduleRebuild(): void {
    ++this.generation;
    clearTimeout(this.rebuildTimer);
    this.rebuildTimer = setTimeout(() => void this.rebuildContext(), 250);
  }

  private async rebuildContext(): Promise<void> {
    const generation = ++this.generation;
    try {
      const folders = await mossFolders(this.app.vault);
      const roots = [...folders];
      const contexts = await Promise.all(roots.map((root) => readSiteContext(this.app.vault, root, roots)));
      if (this.disposed || generation !== this.generation) return;
      this.marked = folders;
      this.watchTree();
      const sites: OpenSite[] = [];
      try {
        roots.forEach((root, index) => {
          const context = contexts[index];
          if (!context) return;
          // Listed before its schema is read, so the catch frees it too.
          const open: OpenSite = { root, site: new MossSite(JSON.stringify(context)), schema: { fields: new Map() } };
          sites.push(open);
          open.schema = siteSchema(open.site);
        });
      } catch (error) {
        for (const built of sites) built.site.free();
        throw error;
      }
      for (const old of this.sites) old.site.free();
      this.sites = sites;
      this.editorAids.splice(0, Infinity, ...(sites.length ? this.aids : []));
      this.app.workspace.updateOptions();
      this.app.workspace.iterateAllLeaves((leaf) => {
        if (leaf.view instanceof MarkdownView) {
          // Obsidian exposes the CM view on Editor at runtime, outside its public types.
          const editor = leaf.view.editor as typeof leaf.view.editor & { cm?: { dispatch: (tr: unknown) => void } };
          editor.cm?.dispatch({ effects: contextChanged.of() });
        }
        if (leaf.view instanceof PagePreviewView) leaf.view.scheduleRender();
      });
    } catch (error) {
      console.warn("[moss] Cannot refresh site context", error);
      new Notice("moss: could not read the site configuration.");
    }
  }

  /** The site of the note at vault path `path`, or null outside every moss folder. */
  siteOf(path: string): SiteNote | null {
    return siteOf(this.sites, path);
  }

  /** The note's site, or the plain one when no moss folder holds it. */
  noteSite(path: string): SiteNote {
    return this.siteOf(path) ?? { ...this.plain, path };
  }

  private noteOf(state: EditorState): SiteNote | null {
    const path = state.field(editorInfoField, false)?.file?.path;
    return path ? this.siteOf(path) : null;
  }

  private watchTree(): void {
    this.tree.disconnect();
    for (const leaf of this.app.workspace.getLeavesOfType("file-explorer")) {
      // A renamed folder keeps its row and changes the row's path.
      this.tree.observe(leaf.view.containerEl, { childList: true, subtree: true, attributeFilter: ["data-path"] });
    }
    markRows(document, this.marked);
  }

  /** The page render needs an open note. */
  canPreviewPage(): boolean {
    return this.app.workspace.getActiveFile()?.extension === "md";
  }

  async openPagePreview(): Promise<void> {
    const workspace = this.app.workspace;
    const source = workspace.activeEditor;
    let leaf = workspace.getLeavesOfType(PAGE_PREVIEW_VIEW_TYPE)[0];
    if (!leaf) {
      const right = workspace.getRightLeaf(false);
      if (!right) return;
      leaf = right;
      await leaf.setViewState({ type: PAGE_PREVIEW_VIEW_TYPE, active: true });
    }
    if (leaf.view instanceof PagePreviewView) leaf.view.scheduleRender(source);
    await workspace.revealLeaf(leaf);
  }

  async saveSettings(): Promise<void> { await this.saveData(this.settings); }

  onunload(): void {
    this.disposed = true;
    ++this.generation;
    clearTimeout(this.rebuildTimer);
    this.desktop?.unload();
    for (const open of this.sites) open.site.free();
    this.plain?.site.free();
  }
}
