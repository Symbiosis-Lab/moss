import { App, Modal, Setting } from "obsidian";

/** Asks before a first build changes a folder. Closing it any other way than Continue cancels. */
export class FirstBuildModal extends Modal {
  private decided = false;

  constructor(
    app: App,
    private title: string,
    private lines: string[],
    private done: (ok: boolean) => void,
  ) {
    super(app);
  }

  onOpen(): void {
    this.titleEl.setText(this.title);
    for (const line of this.lines) this.contentEl.createEl("p", { text: line });
    new Setting(this.contentEl)
      .addButton((b) => b.setButtonText("Cancel").onClick(() => this.finish(false)))
      .addButton((b) => b.setButtonText("Continue").setCta().onClick(() => this.finish(true)));
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
