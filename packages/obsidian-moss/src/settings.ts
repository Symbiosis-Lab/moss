import { App, PluginSettingTab, Setting } from "obsidian";
import type MossPlugin from "./main";

export interface MossSettings {
  /** Explicit path to the moss binary; wins over PATH detection when set. */
  binaryPath: string;
  /**
   * Port to try when the CLI output did not announce a URL (it normally
   * does — this is the backstop, validated against /__moss_health/).
   */
  fallbackPort: number;
  /** Extra CLI flags appended to every build (e.g. "--no-plugins"). */
  extraFlags: string;
}

export const DEFAULT_SETTINGS: MossSettings = {
  binaryPath: "",
  fallbackPort: 8080,
  extraFlags: "",
};

/** Parse the port box; returns null when it is not a valid TCP port. */
export function parsePortInput(raw: string): number | null {
  if (!/^\d{1,5}$/.test(raw.trim())) return null;
  const n = Number(raw.trim());
  return n >= 1 && n <= 65535 ? n : null;
}

export class MossSettingTab extends PluginSettingTab {
  constructor(
    app: App,
    private plugin: MossPlugin,
  ) {
    super(app, plugin);
  }

  display(): void {
    const { containerEl } = this;
    containerEl.empty();

    new Setting(containerEl)
      .setName("moss binary path")
      .setDesc(
        "Leave empty to find moss on PATH (installed via `npm i -g @symbiosis-lab/moss` or Homebrew). Set an absolute path to override.",
      )
      .addText((text) =>
        text
          .setPlaceholder("/opt/homebrew/bin/moss")
          .setValue(this.plugin.settings.binaryPath)
          .onChange(async (value) => {
            this.plugin.settings.binaryPath = value;
            await this.plugin.saveSettings();
          }),
      );

    new Setting(containerEl)
      .setName("Fallback preview port")
      .setDesc(
        "Only used if the CLI does not announce its server URL. moss scans upward from 8080 by default.",
      )
      .addText((text) =>
        text
          .setPlaceholder("8080")
          .setValue(String(this.plugin.settings.fallbackPort))
          .onChange(async (value) => {
            const port = parsePortInput(value);
            if (port !== null) {
              this.plugin.settings.fallbackPort = port;
              await this.plugin.saveSettings();
            }
          }),
      );

    new Setting(containerEl)
      .setName("Extra CLI flags")
      .setDesc("Appended to every moss build (e.g. --no-plugins --strict).")
      .addText((text) =>
        text
          .setPlaceholder("--no-plugins")
          .setValue(this.plugin.settings.extraFlags)
          .onChange(async (value) => {
            this.plugin.settings.extraFlags = value;
            await this.plugin.saveSettings();
          }),
      );
  }
}
