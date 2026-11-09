import { ItemView, ViewStateResult, WorkspaceLeaf } from "obsidian";

export const MOSS_PREVIEW_VIEW_TYPE = "moss-preview";

export const PREVIEW_STOPPED = "Not previewing. Choose “Preview this folder” to start.";

/**
 * Right-leaf pane holding the site preview in an iframe pointed at the local
 * server the moss CLI starts, as HTML Preview and Custom Frames do. The view
 * owns no process: the plugin sets and clears the URL.
 */
export class MossPreviewView extends ItemView {
  private iframe: HTMLIFrameElement | null = null;
  private messageEl: HTMLElement | null = null;
  private folder: string | null = null;

  /** `onClosed` lets the plugin stop the server this pane was showing. */
  constructor(
    leaf: WorkspaceLeaf,
    private onClosed: () => void = () => {},
  ) {
    super(leaf);
  }

  getViewType(): string {
    return MOSS_PREVIEW_VIEW_TYPE;
  }

  getDisplayText(): string {
    return `Preview: ${this.folder ?? this.app.vault.getName()}`;
  }

  /** The folder the pane previews names its tab; Obsidian redraws the tab after this. */
  async setState(state: unknown, result: ViewStateResult): Promise<void> {
    const folder = (state as { folder?: unknown } | null)?.folder;
    if (typeof folder === "string") this.folder = folder;
    await super.setState(state, result);
  }

  getState(): Record<string, unknown> {
    return { ...super.getState(), folder: this.folder };
  }

  getIcon(): string {
    return "globe";
  }

  async onOpen(): Promise<void> {
    const container = this.contentEl;
    container.empty();
    container.addClass("moss-preview-container");
    // A pane Obsidian restores at startup has no server behind it.
    this.messageEl = container.createDiv({ cls: "moss-preview-message", text: PREVIEW_STOPPED });
  }

  async onClose(): Promise<void> {
    this.iframe = null;
    this.messageEl = null;
    this.contentEl.empty();
    this.onClosed();
  }

  /** Point the pane at the running preview server. */
  setUrl(url: string): void {
    if (this.messageEl) {
      this.messageEl.remove();
      this.messageEl = null;
    }
    if (!this.iframe) {
      this.iframe = this.contentEl.createEl("iframe", {
        cls: "moss-preview-iframe",
      });
      this.iframe.setAttribute("title", "moss site preview");
    }
    if (this.iframe.src !== url) {
      this.iframe.src = url;
    }
  }

  /** The current iframe URL, or null before the server is up. */
  get url(): string | null {
    return this.iframe?.src ?? null;
  }

  /** Replace the pane content with a status/error message. */
  showMessage(text: string): void {
    if (this.iframe) {
      this.iframe.remove();
      this.iframe = null;
    }
    if (!this.messageEl) {
      this.messageEl = this.contentEl.createDiv({ cls: "moss-preview-message" });
    }
    this.messageEl.setText(text);
  }
}
