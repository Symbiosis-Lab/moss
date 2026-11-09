// Minimal Obsidian API stub for unit tests (aliased in vitest.config.ts).
// Only what the modules under test import at module scope. Behavioural
// coverage of the real API happens in the wdio e2e, not here.
export class App {}
export class Plugin {
  app: any;
  commands: any[] = [];
  constructor(app: unknown) { this.app = app; }
  async loadData() { return {}; }
  registerEditorExtension() {}
  registerEditorSuggest() {}
  registerView() {}
  registerEvent() {}
  addCommand(command: unknown) { this.commands.push(command); }
  addRibbonIcon() { return {}; }
}
export class PluginSettingTab {
  containerEl: unknown = null;
  constructor(_app: unknown, _plugin: unknown) {}
}
export class Setting {
  setName(): this {
    return this;
  }
  setDesc(): this {
    return this;
  }
  addText(): this {
    return this;
  }
}
export class ItemView {
  contentEl: unknown = null;
  constructor(_leaf: unknown) {}
  registerEvent() {}
}
export class WorkspaceLeaf {}
export class EditorSuggest {
  constructor(_app: unknown) {}
  close(): void {}
}
export class Editor {}
export class TFile {}
export class TFolder {}
export class Notice {
  constructor(_message: string, _timeout?: number) {}
}
export class FileSystemAdapter {}
export class Modal {
  constructor(_app: unknown) {}
}

export const Platform = { isDesktopApp: false };
export class Events {}
export class MarkdownView {}
export { editorInfoField } from "./editor-info-stub";
