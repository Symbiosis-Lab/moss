import { IframeRPC } from "@symbiosis-lab/moss-ui/ui/preview/iframe-rpc";
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
  /** Talks to moss's own script in the previewed page. */
  private bridge: IframeRPC | null = null;

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

  /** The folder the pane previews names its tab and its header. */
  async setState(state: unknown, result: ViewStateResult): Promise<void> {
    const folder = (state as { folder?: unknown } | null)?.folder;
    if (typeof folder === "string") this.folder = folder;
    await super.setState(state, result);
    // Obsidian redraws the tab after this but not the header's title, which its own views set through `titleEl` (not in the typings).
    (this as { titleEl?: HTMLElement }).titleEl?.setText(this.getDisplayText());
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
    this.dropFrame();
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
      this.bridge = new IframeRPC(this.iframe);
    }
    if (this.iframe.src !== url) {
      this.iframe.src = url;
    }
  }

  /** Takes a rebuild into the page where it stands; a page that cannot (its scripts changed) loads again. */
  async update(): Promise<void> {
    const { iframe, bridge } = this;
    if (!iframe || !bridge || (await bridge.morph()).applied) return;
    if (this.iframe === iframe) iframe.src = iframe.src;
  }

  /** Brings a line of the note into view in the page, unless it already is. */
  scrollToLine(line: number): void {
    // The last argument leaves the page alone while that line is on screen. A page without the bridge has no answer.
    this.bridge?.call("scrollToSourceLine", line, false, false, true).catch(() => {});
  }

  /** The current iframe URL, or null before the server is up. */
  get url(): string | null {
    return this.iframe?.src ?? null;
  }

  private dropFrame(): void {
    this.bridge?.destroy();
    this.bridge = null;
    this.iframe?.remove();
    this.iframe = null;
  }

  /** Replace the pane content with a status/error message. */
  showMessage(text: string): void {
    this.dropFrame();
    if (!this.messageEl) {
      this.messageEl = this.contentEl.createDiv({ cls: "moss-preview-message" });
    }
    this.messageEl.setText(text);
  }
}
