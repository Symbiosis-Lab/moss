import { ItemView, WorkspaceLeaf } from "obsidian";

export const MOSS_PREVIEW_VIEW_TYPE = "moss-preview";

/**
 * Right-leaf pane holding the site preview in an iframe pointed at the local
 * server the moss CLI starts. Established Obsidian practice (HTML Preview,
 * Custom Frames). The view owns no process — the plugin sets/clears the URL.
 */
export class MossPreviewView extends ItemView {
  private iframe: HTMLIFrameElement | null = null;
  private messageEl: HTMLElement | null = null;

  constructor(leaf: WorkspaceLeaf) {
    super(leaf);
  }

  getViewType(): string {
    return MOSS_PREVIEW_VIEW_TYPE;
  }

  getDisplayText(): string {
    return "moss preview";
  }

  getIcon(): string {
    return "globe";
  }

  async onOpen(): Promise<void> {
    const container = this.contentEl;
    container.empty();
    container.addClass("moss-preview-container");
    this.messageEl = container.createDiv({
      cls: "moss-preview-message",
      text: "Starting moss preview…",
    });
  }

  async onClose(): Promise<void> {
    this.iframe = null;
    this.messageEl = null;
    this.contentEl.empty();
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
      this.iframe.style.width = "100%";
      this.iframe.style.height = "100%";
      this.iframe.style.border = "none";
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
