import { App, Modal, Setting } from "obsidian";

/** Asked once, before moss adds `.moss/` to a folder. Closing it any other way than Preview cancels. */
export class FirstPreviewModal extends Modal {
  private decided = false;

  constructor(
    app: App,
    private folder: string,
    private done: (ok: boolean) => void,
  ) {
    super(app);
  }

  onOpen(): void {
    this.setTitle(`Preview ${this.folder} with moss?`);
    this.contentEl.createEl("p", { text: `moss adds a .moss folder to ${this.folder}.` });
    new Setting(this.contentEl)
      .addButton((b) => b.setButtonText("Cancel").onClick(() => this.finish(false)))
      .addButton((b) => b.setButtonText("Preview").setCta().onClick(() => this.finish(true)));
  }

  onClose(): void {
    if (!this.decided) this.done(false);
    this.contentEl.empty();
  }

  private finish(ok: boolean): void {
    this.decided = true;
    this.done(ok);
    this.close();
  }
}
