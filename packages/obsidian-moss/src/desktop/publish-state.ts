// The open page in the status bar: its address and where it stands against the last publish, as
// `moss root` says; a click offers the preview, the address, its copies and publishing.
import { debounce, Menu, TFile, type Plugin } from "obsidian";
import type { PageStatus, SiteAddress } from "./cli";

export interface PageAnswer { address: SiteAddress; page?: PageStatus }

/** The page's live address: the site's address and the page's path on it. */
export function pageAddress(answer: PageAnswer): string | undefined {
  if (!answer.page || !answer.address) return undefined;
  return answer.address + answer.page.path;
}

/** The page's channels, then any channel it no longer names that still holds a copy. */
function channelsWithCopies(page: PageStatus): string[] {
  return [...new Set([...page.channels, ...Object.keys(page.copies)])];
}

/** Unknown says nothing: a guess either way would be wrong half the time. */
export function publishLabel(page: PageStatus): string {
  if (page.draft) return "Draft";
  if (page.published === false) return "Not published";
  return page.published && page.changed ? "Changed since publish" : "";
}

/** What the menu's items run. */
export interface PublishActions { preview(): void; publish(): void }

export class PublishState {
  private el: HTMLElement;
  private answer: PageAnswer | null = null;
  private asked = 0;

  constructor(private plugin: Plugin, private ask: (file: TFile) => Promise<PageAnswer | null>, private actions: PublishActions) {
    this.el = plugin.addStatusBarItem();
    this.el.addClass("mod-clickable");
    plugin.registerDomEvent(this.el, "click", (evt) => this.menu(evt));
    const { workspace, metadataCache } = plugin.app;
    // Opening a note in a new leaf fires both in one turn; a later open re-asks, as a publish may have happened since.
    let sameTurn = false;
    const opened = () => {
      if (sameTurn) return;
      sameTurn = true;
      setTimeout(() => { sameTurn = false; }, 0);
      void this.update();
    };
    plugin.registerEvent(workspace.on("file-open", opened));
    plugin.registerEvent(workspace.on("active-leaf-change", opened));
    // Each keystroke's save fires "changed"; ask moss once typing pauses.
    const settled = debounce(() => void this.update(), 800, true);
    plugin.registerEvent(metadataCache.on("changed", (file) => {
      if (file === workspace.getActiveFile()) settled();
    }));
  }

  async update(): Promise<void> {
    const file = this.plugin.app.workspace.getActiveFile();
    const turn = ++this.asked;
    const answer = file instanceof TFile && file.extension === "md" ? await this.ask(file) : null;
    // A later question has already been asked for another file.
    if (turn !== this.asked) return;
    this.answer = answer?.page ? answer : null;
    const page = this.answer?.page;
    this.el.setText(page ? [pageAddress(this.answer!), publishLabel(page)].filter(Boolean).join(" · ") : "");
  }

  private menu(evt: MouseEvent): void {
    const answer = this.answer;
    if (!answer?.page) return;
    const menu = new Menu();
    menu.addItem((i) => i.setTitle("Open in preview").setIcon("globe").onClick(() => this.actions.preview()));
    const address = pageAddress(answer);
    if (address) menu.addItem((i) => i.setTitle(`Copy ${address}`).setIcon("copy").onClick(() => void navigator.clipboard.writeText(`https://${address}`)));
    for (const channel of channelsWithCopies(answer.page)) {
      const copy = answer.page.copies[channel];
      menu.addItem((i) => {
        i.setTitle(`On ${channel}`).setIcon("send");
        if (copy) i.onClick(() => window.open(copy));
        else i.setDisabled(true);
      });
    }
    menu.addSeparator();
    menu.addItem((i) => i.setTitle("Publish this folder").setIcon("upload-cloud").onClick(() => this.actions.publish()));
    menu.showAtMouseEvent(evt);
  }
}
