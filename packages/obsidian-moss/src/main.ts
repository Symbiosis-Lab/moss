// moss for Obsidian — v1.
//
// Knowledge first: the frontmatter schema from `moss describe --json`
// (bundled snapshot, refreshed from the CLI when present) drives inline
// diagnostics, hover explanations, and field-name completion. On top of that,
// the CLI bridge: Build/Preview commands that run `moss build` against the
// vault and show the preview server in a right-leaf iframe pane.
import { FileSystemAdapter, Notice, Plugin, WorkspaceLeaf } from "obsidian";
import { shell } from "electron";
import { execFile, spawn } from "node:child_process";
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import {
  buildArgs,
  findMossBinary,
  MOSS_HEALTH_MARKER,
  MOSS_HEALTH_PATH,
  publishInAppUrl,
  splitFlags,
  type DetectEnv,
} from "./cli";
import { frontmatterExtension } from "./cm-frontmatter";
import { agentFoldersFor, firstBuildMessage, isInICloud } from "./first-build";
import { FirstBuildModal } from "./first-build-modal";
import { MossFieldSuggest } from "./field-suggest";
import { MossPreviewView, MOSS_PREVIEW_VIEW_TYPE, PREVIEW_STOPPED } from "./preview-view";
import { MossProcess, type SpawnFn } from "./process";
import { parseDescribeJson, type MossSchema } from "./schema";
import { DEFAULT_SETTINGS, MossSettingTab, type MossSettings } from "./settings";
import { shortcodeExtension } from "./syntax/cm-shortcode";
import describeSnapshot from "./describe-snapshot.json";

export default class MossPlugin extends Plugin {
  settings: MossSettings = { ...DEFAULT_SETTINGS };
  schema: MossSchema = parseDescribeJson(JSON.stringify(describeSnapshot), "bundled snapshot");
  private mossProcess = new MossProcess(spawn as unknown as SpawnFn);
  private statusEl: HTMLElement | null = null;
  private previewUrl: string | null = null;
  /** True while the running process is a preview server, not a one-off build. */
  private previewing = false;
  /** Bumped per preview run, so a stopped run's late exit cannot clear a newer one. */
  private previewRun = 0;

  async onload(): Promise<void> {
    await this.loadSettings();
    this.addSettingTab(new MossSettingTab(this.app, this));

    // Knowledge layer.
    this.registerEditorExtension([frontmatterExtension(() => this.schema), shortcodeExtension()]);
    this.registerEditorSuggest(new MossFieldSuggest(this, () => this.schema));

    // Preview pane.
    // Closing the pane stops its server: nothing else shows it is running.
    this.registerView(MOSS_PREVIEW_VIEW_TYPE, (leaf) => new MossPreviewView(leaf, () => this.stopPreview(false)));

    // CLI bridge.
    this.addCommand({
      id: "build",
      name: "Build site",
      callback: () => void this.runBuild(),
    });
    this.addCommand({
      id: "preview",
      name: "Preview site",
      callback: () => this.runPreview(),
    });
    this.addCommand({
      id: "stop-preview",
      name: "Stop preview server",
      callback: () => this.stopPreview(true),
    });
    this.addCommand({
      id: "publish-in-app",
      name: "Publish with the moss app",
      callback: () => this.publishInApp(),
    });

    this.statusEl = this.addStatusBarItem();
    this.setStatus("idle");

    // Refresh the schema from the real CLI in the background; the bundled
    // snapshot already covers the knowledge features until (and unless) a
    // binary is found.
    void this.refreshSchema();
  }

  onunload(): void {
    this.mossProcess.stop();
  }

  async loadSettings(): Promise<void> {
    this.settings = { ...DEFAULT_SETTINGS, ...((await this.loadData()) ?? {}) };
  }

  async saveSettings(): Promise<void> {
    await this.saveData(this.settings);
    void this.refreshSchema();
  }

  // ------------------------------------------------------------------
  // CLI detection

  detectEnv(): DetectEnv {
    return {
      pathVar: process.env.PATH ?? "",
      pathDelimiter: path.delimiter,
      platform: process.platform,
      homeDir: os.homedir(),
      fileExists: (p) => {
        try {
          fs.accessSync(p, fs.constants.X_OK);
          return true;
        } catch {
          return false;
        }
      },
    };
  }

  findBinary(): string | null {
    return findMossBinary(this.settings.binaryPath, this.detectEnv());
  }

  private requireBinary(): string | null {
    const binary = this.findBinary();
    if (!binary) {
      new Notice(
        "moss CLI not found. Install it with `npm i -g @symbiosis-lab/moss` or Homebrew, or set the binary path in settings.",
        8000,
      );
      this.setStatus("not found");
    }
    return binary;
  }

  vaultPath(): string | null {
    const adapter = this.app.vault.adapter;
    return adapter instanceof FileSystemAdapter ? adapter.getBasePath() : null;
  }

  // ------------------------------------------------------------------
  // Schema refresh

  async refreshSchema(): Promise<void> {
    const binary = this.findBinary();
    if (!binary) return;
    try {
      const stdout = await new Promise<string>((resolve, reject) => {
        execFile(binary, ["describe", "--json"], { maxBuffer: 4 * 1024 * 1024 }, (err, out) =>
          err ? reject(err) : resolve(out),
        );
      });
      this.schema = parseDescribeJson(stdout);
    } catch (err) {
      // Keep the bundled snapshot; the knowledge layer still works.
      console.warn("[moss] describe --json failed; using bundled schema", err);
    }
  }

  // ------------------------------------------------------------------
  // Build / preview

  /**
   * True once the owner agrees to a first build, or at once when the vault is
   * already a moss site. A first build edits notes (it adds `uid:` lines) and
   * creates folders, so it is never started without saying so.
   */
  private async confirmFirstBuild(vault: string): Promise<boolean> {
    if (await this.app.vault.adapter.exists(".moss")) return true;
    const notesGainingUid = this.app.vault.getMarkdownFiles().filter((file) => {
      const fm = this.app.metadataCache.getFileCache(file)?.frontmatter;
      return fm !== undefined && !("uid" in fm);
    }).length;
    const lines = firstBuildMessage({
      notesGainingUid,
      agentFolders: agentFoldersFor((p) => fs.existsSync(p), vault, os.homedir(), path.sep),
      inICloud: isInICloud(vault),
    });
    return new Promise((resolve) => new FirstBuildModal(this.app, lines, resolve).open());
  }

  private async runBuild(): Promise<void> {
    const binary = this.requireBinary();
    const vault = this.vaultPath();
    if (!binary || !vault) return;
    if (this.mossProcess.running) {
      new Notice("moss is already running. Stop the preview server first.");
      return;
    }
    if (!(await this.confirmFirstBuild(vault))) return;
    this.setStatus("building…");
    new Notice("moss: building site…");
    this.mossProcess.start(binary, buildArgs(vault, { extraFlags: splitFlags(this.settings.extraFlags) }), vault, {
      onError: (message) => {
        new Notice(`moss build failed: ${message}`, 10000);
        this.setStatus("error");
      },
      onProblems: (count) => {
        new Notice(`moss: ${count} problem${count === 1 ? "" : "s"} reported — site still generated.`, 8000);
      },
      onExit: (code) => {
        if (code === 0) {
          new Notice("moss: site built.");
          this.setStatus("idle");
        } else if (code !== null) {
          this.setStatus("error");
        }
      },
    });
  }

  private async runPreview(): Promise<void> {
    const binary = this.requireBinary();
    const vault = this.vaultPath();
    if (!binary || !vault) return;
    if (this.mossProcess.running) {
      // Already serving — just surface the pane again.
      const view = await this.openPreviewLeaf();
      if (this.previewUrl) view?.setUrl(this.previewUrl);
      return;
    }
    if (!(await this.confirmFirstBuild(vault))) return;

    const view = await this.openPreviewLeaf();
    view?.showMessage("Building site…");
    this.setStatus("building…");

    this.previewing = true;
    const run = ++this.previewRun;
    let sawUrl = false;
    this.mossProcess.start(
      binary,
      buildArgs(vault, { serve: true, watch: true, extraFlags: splitFlags(this.settings.extraFlags) }),
      vault,
      {
        onServerReady: (addr) => {
          sawUrl = true;
          this.previewUrl = addr.url;
          this.setStatus(`serving :${addr.port}`);
          view?.setUrl(addr.url);
        },
        onStandby: () => {
          new Notice("Preview is served by the moss app; this window follows it");
        },
        onError: (message) => {
          new Notice(`moss preview failed: ${message}`, 10000);
          view?.showMessage(`moss failed: ${message}`);
          this.setStatus("error");
        },
        onProblems: (count) => {
          new Notice(`moss: ${count} problem${count === 1 ? "" : "s"} reported — site still generated.`, 8000);
        },
        onExit: (code) => {
          if (run !== this.previewRun) return;
          this.previewing = false;
          this.previewUrl = null;
          if (code !== 0 && code !== null) this.setStatus("error");
          else this.setStatus("idle");
          this.openPreviewView()?.showMessage(PREVIEW_STOPPED);
        },
      },
    );

    // Backstop: the URL line has been on stderr since 0.11.x, but if parsing
    // ever misses it, probe the configured fallback port for a real moss
    // server before giving up.
    window.setTimeout(() => {
      if (sawUrl || !this.mossProcess.running) return;
      void this.tryFallbackPort(view);
    }, 30000);
  }

  private async tryFallbackPort(view: MossPreviewView | null): Promise<void> {
    const port = this.settings.fallbackPort;
    const healthy = await checkMossHealth(port);
    if (healthy && this.mossProcess.running) {
      const url = `http://localhost:${port}`;
      this.previewUrl = url;
      this.setStatus(`serving :${port}`);
      view?.setUrl(url);
    }
  }

  /** `announce` is false when the pane closed itself and there is nothing to say. */
  private stopPreview(announce: boolean): void {
    if (!this.previewing) {
      if (announce) new Notice("moss: no preview server running.");
      return;
    }
    this.mossProcess.stop();
    this.previewing = false;
    this.previewUrl = null;
    this.setStatus("idle");
    this.openPreviewView()?.showMessage(PREVIEW_STOPPED);
    if (announce) new Notice("moss: preview server stopped.");
  }

  private publishInApp(): void {
    const vault = this.vaultPath();
    if (!vault) return;
    new Notice(
      "Opening this vault in the moss app, where you publish it. moss publishes every note in the vault except those marked draft: true.",
      10000,
    );
    shell.openExternal(publishInAppUrl(vault)).catch(() => {
      new Notice("The moss app is not installed. Get it from mosspub.com to publish this vault.", 10000);
    });
  }

  private openPreviewView(): MossPreviewView | null {
    const view = this.app.workspace.getLeavesOfType(MOSS_PREVIEW_VIEW_TYPE)[0]?.view;
    return view instanceof MossPreviewView ? view : null;
  }

  private async openPreviewLeaf(): Promise<MossPreviewView | null> {
    const existing = this.app.workspace.getLeavesOfType(MOSS_PREVIEW_VIEW_TYPE);
    let leaf: WorkspaceLeaf | null = existing[0] ?? null;
    if (!leaf) {
      leaf = this.app.workspace.getRightLeaf(false);
      if (!leaf) return null;
      await leaf.setViewState({ type: MOSS_PREVIEW_VIEW_TYPE, active: true });
    }
    this.app.workspace.revealLeaf(leaf);
    const view = leaf.view;
    return view instanceof MossPreviewView ? view : null;
  }

  private setStatus(text: string): void {
    this.statusEl?.setText(`moss: ${text}`);
  }
}

/** GET the moss health endpoint; true only for a real moss preview server. */
function checkMossHealth(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const req = http.get(
      { host: "127.0.0.1", port, path: MOSS_HEALTH_PATH, timeout: 2000 },
      (res) => {
        let body = "";
        res.on("data", (c) => (body += String(c)));
        res.on("end", () => resolve(body.includes(MOSS_HEALTH_MARKER)));
      },
    );
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
  });
}
