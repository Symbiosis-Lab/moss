import { App, PluginSettingTab, Setting } from "obsidian";
import type { DesktopCommands } from "./desktop";
import type MossPlugin from "./main";

export interface MossSettings {
  /** Where the moss program is; wins over the usual install locations when set. */
  binaryPath: string;
}

export const DEFAULT_SETTINGS: MossSettings = { binaryPath: "" };

export class MossSettingTab extends PluginSettingTab {
  constructor(
    app: App,
    private plugin: MossPlugin,
    private desktop: DesktopCommands,
  ) {
    super(app, plugin);
  }

  display(): void {
    const { containerEl } = this;
    containerEl.empty();
    const setting = new Setting(containerEl).setName("moss program");
    const describe = async () => {
      const found = this.desktop.findBinary();
      if (!found) return void setting.setDesc("Not found. Get moss at mosspub.com, or enter where it is.");
      setting.setDesc(`Found at ${found}.`);
      const version = await this.desktop.version(found);
      if (version && this.desktop.findBinary() === found) setting.setDesc(`Previews run with ${version}, at ${found}.`);
    };
    // Asking a half-typed path for its version on every keystroke would run whatever it names so far.
    let pending: ReturnType<typeof setTimeout> | undefined;
    setting.addText((text) =>
      text
        .setPlaceholder("Found automatically")
        .setValue(this.plugin.settings.binaryPath)
        .onChange(async (value) => {
          this.plugin.settings.binaryPath = value.trim();
          await this.plugin.saveSettings();
          clearTimeout(pending);
          pending = setTimeout(() => void describe(), 600);
        }),
    );
    void describe();
  }
}
