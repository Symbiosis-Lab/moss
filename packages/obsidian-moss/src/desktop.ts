import { FileSystemAdapter, Notice, WorkspaceLeaf } from "obsidian";
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
  parseRootJson,
  publishInAppUrl,
  publishNotice,
  splitFlags,
  type DetectEnv,
  type SiteAddress,
} from "./desktop/cli";
import { firstBuildMessage, isInICloud } from "./desktop/first-build";
import { FirstBuildModal } from "./desktop/first-build-modal";
import { MossPreviewView, MOSS_PREVIEW_VIEW_TYPE, PREVIEW_STOPPED } from "./desktop/preview-view";
import { MossProcess, type SpawnFn } from "./desktop/process";
import { MossSettingTab } from "./settings";
import type MossPlugin from "./main";

export class DesktopCommands {
  constructor(private plugin: MossPlugin) {}

  private get app() { return this.plugin.app; }
  private get settings() { return this.plugin.settings; }
  private mossProcess = new MossProcess(spawn as unknown as SpawnFn);
  private statusEl: HTMLElement | null = null;
  /** Set while the running process is a preview server, not a one-off build; `url` once it answers. */
  private preview: { root: string; url: string | null } | null = null;
  /** Bumped per preview run, so a stopped run's late exit cannot clear a newer one. */
  private previewRun = 0;

  load(): void {
    this.plugin.addSettingTab(new MossSettingTab(this.app, this.plugin));
    this.plugin.registerView(MOSS_PREVIEW_VIEW_TYPE, (leaf) => new MossPreviewView(leaf, () => this.stopPreview(false)));
    // CLI bridge.
    this.plugin.addCommand({
      id: "build",
      name: "Build site",
      callback: () => void this.runBuild(),
    });
    this.plugin.addCommand({
      id: "preview",
      name: "Preview site",
      callback: () => this.runPreview(),
    });
    this.plugin.addCommand({
      id: "stop-preview",
      name: "Stop preview server",
      callback: () => this.stopPreview(true),
    });
    this.plugin.addCommand({
      id: "publish-in-app",
      name: "Publish this folder",
      callback: () => void this.publishInApp(),
    });

    this.statusEl = this.plugin.addStatusBarItem();
    this.setStatus("idle");
  }

  unload(): void {
    this.mossProcess.stop();
  }

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

  /**
   * The site the open note belongs to, as `moss root` finds it (the nearest
   * folder with a `.moss` folder); the vault itself when no site holds the
   * note or the CLI cannot say. The frontmatter checks and page preview still
   * read the vault root's site settings.
   */
  private async currentSite(binary = this.findBinary()): Promise<{ root: string; address: SiteAddress } | null> {
    const vault = this.vaultPath();
    if (!vault) return null;
    const note = this.app.workspace.getActiveFile();
    if (!binary) return { root: vault, address: undefined };
    const found = parseRootJson(await runCli(binary, ["root", note ? path.join(vault, note.path) : vault, "--json"]).catch(() => ""));
    // No site holds the note yet: a first build makes the vault one.
    return { root: found.root || vault, address: found.address };
  }

  /**
   * True once the owner agrees to a first build, or at once when the folder is
   * already a moss site. A first build creates `.moss/`, which a sync client
   * copies everywhere, so it is never started without saying so.
   */
  private async confirmFirstBuild(root: string): Promise<boolean> {
    if (fs.existsSync(path.join(root, ".moss"))) return true;
    const folder = path.basename(root);
    const lines = firstBuildMessage({ folder, inICloud: isInICloud(root) });
    return new Promise((resolve) => new FirstBuildModal(this.app, `Make ${folder} a moss site?`, lines, resolve).open());
  }

  private async runBuild(): Promise<void> {
    const binary = this.requireBinary();
    const site = binary ? await this.currentSite(binary) : null;
    if (!binary || !site) return;
    const root = site.root;
    if (this.mossProcess.running) {
      new Notice("moss is already running. Stop the preview server first.");
      return;
    }
    if (!(await this.confirmFirstBuild(root))) return;
    this.setStatus("building…");
    new Notice("moss: building site…");
    this.mossProcess.start(binary, buildArgs(root, { extraFlags: splitFlags(this.settings.extraFlags) }), root, {
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
    const site = binary ? await this.currentSite(binary) : null;
    if (!binary || !site) return;
    const root = site.root;
    // Another site's preview gives way; this site's is surfaced again.
    if (this.preview && this.preview.root !== root) this.stopPreview(false);
    if (this.mossProcess.running) {
      const view = await this.openPreviewLeaf();
      if (this.preview?.url) view?.setUrl(this.preview.url);
      return;
    }
    if (!(await this.confirmFirstBuild(root))) return;

    const view = await this.openPreviewLeaf();
    view?.showMessage("Building site…");
    this.setStatus("building…");

    this.preview = { root, url: null };
    const run = ++this.previewRun;
    let sawUrl = false;
    this.mossProcess.start(
      binary,
      buildArgs(root, { serve: true, watch: true, extraFlags: splitFlags(this.settings.extraFlags) }),
      root,
      {
        onServerReady: (addr) => {
          if (run !== this.previewRun) return;
          sawUrl = true;
          if (this.preview) this.preview.url = addr.url;
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
          this.preview = null;
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
      if (sawUrl || run !== this.previewRun || !this.mossProcess.running) return;
      void this.tryFallbackPort(view);
    }, 30000);
  }

  private async tryFallbackPort(view: MossPreviewView | null): Promise<void> {
    const port = this.settings.fallbackPort;
    const healthy = await checkMossHealth(port);
    if (healthy && this.mossProcess.running) {
      const url = `http://localhost:${port}`;
      if (this.preview) this.preview.url = url;
      this.setStatus(`serving :${port}`);
      view?.setUrl(url);
    }
  }

  /** `announce` is false when the pane closed itself and there is nothing to say. */
  private stopPreview(announce: boolean): void {
    if (!this.preview) {
      if (announce) new Notice("moss: no preview server running.");
      return;
    }
    this.mossProcess.stop();
    this.preview = null;
    this.setStatus("idle");
    this.openPreviewView()?.showMessage(PREVIEW_STOPPED);
    if (announce) new Notice("moss: preview server stopped.");
  }

  private async publishInApp(): Promise<void> {
    const site = await this.currentSite();
    if (!site) return;
    const folder = path.basename(site.root);
    new Notice(publishNotice(folder, site.address), 10000);
    shell.openExternal(publishInAppUrl(site.root)).catch(() => {
      new Notice(`Get moss at mosspub.com to publish ${folder}.`, 10000);
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

function runCli(binary: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(binary, args, { maxBuffer: 4 * 1024 * 1024 }, (err, out) => (err ? reject(err) : resolve(out)));
  });
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
