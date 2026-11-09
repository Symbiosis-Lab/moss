import { ItemView, TFile, type MarkdownFileInfo, type WorkspaceLeaf } from "obsidian";
import type { PagePreview } from "./core";
import type MossPlugin from "./main";
import { vaultBaseHref } from "./site-context";

export const PAGE_PREVIEW_VIEW_TYPE = "moss-page-preview";

export class PagePreviewView extends ItemView {
  private iframe: HTMLIFrameElement | null = null;
  private timer?: ReturnType<typeof setTimeout>;
  private request = 0;
  private source: MarkdownFileInfo | null = null;
  private shown = { request: 0, revision: "", html: "" };
  private closed = false;

  constructor(leaf: WorkspaceLeaf, private plugin: MossPlugin) { super(leaf); }
  getViewType(): string { return PAGE_PREVIEW_VIEW_TYPE; }
  getDisplayText(): string { return "moss page preview"; }
  getIcon(): string { return "file-search"; }

  async onOpen(): Promise<void> {
    this.closed = false;
    this.shown = { request: 0, revision: "", html: "" };
    this.contentEl.empty();
    this.contentEl.addClass("moss-preview-container");
    this.iframe = this.contentEl.createEl("iframe", { cls: "moss-page-preview-iframe moss-preview-iframe" });
    this.iframe.setAttribute("sandbox", "");
    this.iframe.setAttribute("title", "moss page preview");
    const workspace = this.plugin.app.workspace;
    this.registerEvent(workspace.on("editor-change", () => this.scheduleRender()));
    this.registerEvent(workspace.on("file-open", () => this.scheduleRender()));
    this.registerEvent(this.plugin.app.vault.on("modify", () => this.scheduleRender()));
    this.scheduleRender();
  }

  scheduleRender(source = this.plugin.app.workspace.activeEditor): void {
    if (this.closed) return;
    if (source?.file?.extension === "md") this.source = source;
    const request = ++this.request;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.render(request), 250);
  }

  private async render(request: number): Promise<void> {
    const { vault, workspace } = this.plugin.app;
    const editor = this.source;
    const file = editor?.file ?? workspace.getActiveFile();
    if (!file || file.extension !== "md") return;
    try {
      const text = editor?.file?.path === file.path && editor.editor
        ? editor.editor.getValue() : await vault.cachedRead(file);
      if (request !== this.request || this.closed) return;
      const site = this.plugin.site;
      const paths: string[] = JSON.parse(site.embeddedNotes(file.path, text));
      const notes: Record<string, string> = {};
      await Promise.all(paths.map(async (path) => {
        const note = vault.getAbstractFileByPath(path);
        if (note instanceof TFile) notes[path] = await vault.cachedRead(note);
      }));
      if (this.closed || request !== this.request || site !== this.plugin.site) return;
      site.setNotes(JSON.stringify(notes));
      const result: PagePreview = JSON.parse(site.preview(file.path, text, vaultBaseHref(vault.adapter, file.path)));
      this.showPreview(result, request);
    } catch (error) {
      if (!this.closed && request === this.request && this.iframe) {
        this.iframe.srcdoc = "<p>Could not preview this page.</p>";
        console.warn("[moss] Page preview failed", error);
      }
    }
  }

  showPreview(result: PagePreview, request: number): void {
    if (this.closed || !this.iframe || request < this.request || request < this.shown.request) return;
    // Revisions are content hashes; request order supplies their age.
    if (result.revision !== this.shown.revision || result.html !== this.shown.html) {
      this.iframe.srcdoc = result.html;
    }
    this.shown = { request, revision: result.revision, html: result.html };
  }

  async onClose(): Promise<void> {
    this.closed = true;
    ++this.request;
    clearTimeout(this.timer);
    this.iframe = null;
    this.contentEl.empty();
  }
}
