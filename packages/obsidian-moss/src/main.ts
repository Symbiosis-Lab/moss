import { Events, MarkdownView, Notice, Platform, Plugin } from "obsidian";
import { contextChanged, frontmatterExtension } from "./cm-frontmatter";
import { initializeCore, MossSite } from "./core";
import { MossFieldSuggest } from "./field-suggest";
import { PagePreviewView, PAGE_PREVIEW_VIEW_TYPE } from "./page-preview-view";
import { composedSchema, type MossSchema } from "./schema";
import { DEFAULT_SETTINGS, type MossSettings } from "./settings";
import { readSiteContext } from "./site-context";
import { shortcodeExtension } from "./syntax/cm-shortcode";
import type { DesktopCommands } from "./desktop";

export default class MossPlugin extends Plugin {
  settings: MossSettings = { ...DEFAULT_SETTINGS };
  schema: MossSchema = { fields: new Map() };
  site!: MossSite;
  private desktop?: DesktopCommands;
  private rebuildTimer?: ReturnType<typeof setTimeout>;
  private generation = 0;
  private disposed = false;

  async onload(): Promise<void> {
    this.settings = { ...DEFAULT_SETTINGS, ...((await this.loadData()) ?? {}) };
    initializeCore();
    this.site = new MossSite(JSON.stringify({ files: [], lang: "en" }));
    this.schema = composedSchema();
    await this.rebuildContext();
    if (this.disposed) return;
    this.registerEditorExtension([
      frontmatterExtension(() => this.schema, () => this.site), shortcodeExtension(),
    ]);
    this.registerEditorSuggest(new MossFieldSuggest(this, () => this.schema));
    this.registerView(PAGE_PREVIEW_VIEW_TYPE, (leaf) => new PagePreviewView(leaf, this));
    this.addCommand({ id: "preview-page", name: "Preview this page", callback: () => this.openPagePreview() });
    for (const event of ["create", "delete", "rename"] as const) {
      this.registerEvent((this.app.vault as Events).on(event, () => this.scheduleRebuild()));
    }
    this.registerEvent(this.app.vault.on("modify", (file) => {
      if (file.path.startsWith(".moss/")) this.scheduleRebuild();
    }));
    // Hidden folders bypass Vault's file index, but still emit raw adapter events.
    this.registerEvent((this.app.vault as Events).on("raw", (path: unknown) => {
      if (typeof path !== "string") return;
      if (path === ".moss" || path.startsWith(".moss/")) this.scheduleRebuild();
    }));
    this.app.workspace.onLayoutReady(() => { if (!this.disposed) this.scheduleRebuild(); });
    if (Platform.isDesktopApp) {
      const { DesktopCommands } = await import("./desktop");
      if (this.disposed) return;
      this.desktop = new DesktopCommands(this);
      this.desktop.load();
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
      const context = await readSiteContext(this.app.vault);
      if (this.disposed || generation !== this.generation) return;
      const site = new MossSite(JSON.stringify(context));
      this.site.free();
      this.site = site;
      this.schema = composedSchema(context.plugins, context.listing_styles);
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

  private async openPagePreview(): Promise<void> {
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
    this.app.workspace.detachLeavesOfType(PAGE_PREVIEW_VIEW_TYPE);
    this.site?.free();
  }
}
