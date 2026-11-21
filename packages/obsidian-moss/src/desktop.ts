import { EditorView } from "@codemirror/view";
import { editorInfoField, FileSystemAdapter, Notice, TFile, TFolder, WorkspaceLeaf } from "obsidian";
import { shell } from "electron";
import { execFile, spawn } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import {
  findMossBinary,
  parseRootJson,
  publishInAppUrl,
  publishNotice,
  siteRoot,
  type DetectEnv,
  type PageStatus,
  type SiteAddress,
} from "./desktop/cli";
import { FirstPreviewModal } from "./desktop/first-build-modal";
import { MossPreviewView, MOSS_PREVIEW_VIEW_TYPE, PREVIEW_STOPPED } from "./desktop/preview-view";
import { MossProcess, type SpawnFn } from "./desktop/process";
import { pageFor, watchRebuilds } from "./desktop/server";
import { PublishState } from "./desktop/publish-state";
import { MossSettingTab } from "./settings";
import type MossPlugin from "./main";

export class DesktopCommands {
  constructor(private plugin: MossPlugin) {}

  private get app() { return this.plugin.app; }
  private mossProcess = new MossProcess(spawn as unknown as SpawnFn);
  private statusEl: HTMLElement | null = null;
  /** The running preview, from the first click until its process ends; a callback acts only for its own session. */
  private session: Session | null = null;
  /** The page-only notice shows once per run of Obsidian, not on every click. */
  private pageOnlyNoticeShown = false;

  load(): void {
    this.plugin.addSettingTab(new MossSettingTab(this.app, this.plugin, this));
    const publish = new PublishState(this.plugin, (file) => this.siteFor(this.findBinary(), file.path), {
      preview: () => void this.runPreview(),
      publish: () => void this.publishInApp(),
    });
    this.app.workspace.onLayoutReady(() => void publish.update());
    this.plugin.registerView(MOSS_PREVIEW_VIEW_TYPE, (leaf) => new MossPreviewView(leaf, () => this.stopPreview(false)));
    this.plugin.addCommand({ id: "preview", name: "Preview this folder", callback: () => void this.runPreview() });
    this.plugin.addCommand({
      id: "stop-preview",
      name: "Stop preview",
      checkCallback: (checking) => {
        // Not while only the first-preview question is open: nothing runs yet.
        if (!this.session || !this.mossProcess.running) return false;
        if (!checking) this.stopPreview(true);
        return true;
      },
    });
    this.plugin.addCommand({ id: "publish-in-app", name: "Publish this folder", callback: () => void this.publishInApp() });
    this.plugin.addRibbonIcon("globe", "Preview this folder", () => void this.runPreview());
    this.plugin.registerEvent(this.app.workspace.on("file-menu", (menu, file) => {
      if (!(file instanceof TFolder)) return;
      menu.addItem((item) => item
        .setTitle("Preview with moss")
        .setIcon("globe")
        .onClick(() => void this.runPreview(file.path)));
    }));

    this.plugin.registerEvent(this.app.workspace.on("file-open", (file) => void this.follow(file)));
    this.plugin.registerEditorExtension(EditorView.updateListener.of((update) => {
      const session = this.session;
      if (!session?.page || !update.selectionSet || update.state.field(editorInfoField).file?.path !== session.page) return;
      const line = update.state.doc.lineAt(update.state.selection.main.head).number;
      if (line === session.line) return;
      session.line = line;
      this.openPreviewView()?.scrollToLine(line);
    }));

    // Obsidian calls no onunload when its window closes or it quits.
    this.plugin.registerDomEvent(window, "beforeunload", () => this.mossProcess.stop());

    this.statusEl = this.plugin.addStatusBarItem();
    this.statusEl.addClass("mod-clickable");
    this.plugin.registerDomEvent(this.statusEl, "click", () => void this.openPreviewLeaf());
    this.setStatus(null);
  }

  unload(): void {
    this.stopPreview(false);
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
    return findMossBinary(this.plugin.settings.binaryPath, this.detectEnv());
  }

  /** The version the program reports, e.g. "moss 0.15.20", or null when it does not answer. */
  version(binary: string): Promise<string | null> {
    return runCli(binary, ["--version"], 5000).then((out) => out.trim().split("\n")[0] || null, () => null);
  }

  vaultPath(): string | null {
    const adapter = this.app.vault.adapter;
    return adapter instanceof FileSystemAdapter ? adapter.getBasePath() : null;
  }

  /**
   * The moss site that owns `target` (a vault path) or else the open note, as
   * `moss root` finds it. When none does, a clicked folder becomes the site;
   * otherwise the vault. Without the program the vault is all there is to say.
   * For a page the last build made, `page` says what moss knows of it.
   */
  private async siteFor(binary: string | null, target?: string): Promise<{ root: string; address: SiteAddress; page?: PageStatus } | null> {
    const vault = this.vaultPath();
    if (!vault) return null;
    if (!binary) return { root: siteRoot(path, vault, undefined, null), address: undefined };
    const at = path.join(vault, target ?? this.app.workspace.getActiveFile()?.path ?? "");
    // A CLI without `moss root` fails here and reads as no site found.
    const found = parseRootJson(await runCli(binary, ["root", at, "--json"], 5000).catch(() => ""));
    return { root: siteRoot(path, vault, target, found.root), address: found.address, page: found.page };
  }

  /** A folder moss has not previewed gains `.moss/`; ask once before that. */
  private async confirmFirstPreview(session: Session): Promise<boolean> {
    if (fs.existsSync(path.join(session.root, ".moss"))) return true;
    return new Promise((resolve) => {
      session.modal = new FirstPreviewModal(this.app, session.folder, resolve);
      session.modal.open();
    });
  }

  /** `target` is the folder whose menu was used; otherwise the open note decides. */
  private async runPreview(target?: string): Promise<void> {
    const binary = this.findBinary();
    if (!binary) {
      // Without the moss program, the page render from moss core is still a preview of the open note.
      if (target !== undefined || !this.plugin.canPreviewPage()) {
        const folder = target ? path.basename(target) || this.app.vault.getName() : this.app.vault.getName();
        new Notice(`Get moss at mosspub.com to preview ${folder}.`, 8000);
        return;
      }
      await this.plugin.openPagePreview();
      if (!this.pageOnlyNoticeShown) {
        this.pageOnlyNoticeShown = true;
        new Notice(`Showing this page only. Get moss at mosspub.com to preview all of ${this.app.vault.getName()}.`, 8000);
      }
      return;
    }
    const site = await this.siteFor(binary, target);
    if (!site) return;
    if (this.session?.root === site.root) {
      const { url } = this.session;
      const view = await this.openPreviewLeaf();
      if (url) view?.setUrl(url);
      return;
    }
    // Another site's preview gives way.
    this.stopPreview(false);
    const session: Session = { root: site.root, folder: path.basename(site.root), url: null };
    this.session = session;
    if (!(await this.confirmFirstPreview(session))) {
      this.endSession(session);
      return;
    }
    const view = await this.openPreviewLeaf(session.folder);
    if (this.session !== session) return;
    view?.showMessage(`Building ${session.folder}…`);
    this.setStatus(`Previewing ${session.folder}`);
    this.mossProcess.start(binary, ["build", session.root, "--serve", "--watch"], session.root, {
      onServerReady: (addr) => {
        if (this.session !== session) return;
        session.url = addr.url;
        session.stopWatching?.();
        session.stopWatching = watchRebuilds(addr.url, () => void this.openPreviewView()?.update());
        void this.follow(this.app.workspace.getActiveFile()).then((followed) => {
          if (!followed && this.session === session) view?.setUrl(addr.url);
        });
      },
      onError: (message) => {
        session.failure = `moss could not preview ${session.folder}: ${message}`;
        view?.showMessage(session.failure);
      },
      onExit: () => this.endSession(session),
    });
  }

  /**
   * Points the pane at the page of the note in front of the person, as Obsidian's
   * own panes follow the active note. It stays where it is for a note outside the
   * previewed folder, a note with no page yet, and while the person has it pinned.
   */
  private async follow(file: TFile | null): Promise<boolean> {
    const session = this.session;
    const vault = this.vaultPath();
    if (!session?.url || !vault || file?.extension !== "md") return false;
    if (this.app.workspace.getLeavesOfType(MOSS_PREVIEW_VIEW_TYPE)[0]?.getViewState().pinned) return false;
    const inFolder = path.relative(session.root, path.join(vault, file.path));
    if (inFolder.startsWith("..") || path.isAbsolute(inFolder)) return false;
    const page = await pageFor(session.url, inFolder.split(path.sep).join("/"));
    if (!page || this.session !== session || !session.url) return false;
    session.page = file.path;
    session.line = undefined;
    this.openPreviewView()?.setUrl(session.url + page);
    return true;
  }

  /** `announce` is false when the pane closed itself, or another site's preview replaces this one. */
  private stopPreview(announce: boolean): void {
    const session = this.session;
    if (!session) return;
    this.mossProcess.stop();
    this.endSession(session);
    if (announce) new Notice(`Stopped previewing ${session.folder}.`);
  }

  /** The one place a preview ends, whether stopped, closed, cancelled or exited. */
  private endSession(session: Session): void {
    if (this.session !== session) return;
    this.session = null;
    session.stopWatching?.();
    session.modal?.close();
    this.setStatus(null);
    // A failure stays on screen; the exit that follows it is not news.
    if (!session.failure) this.openPreviewView()?.showMessage(PREVIEW_STOPPED);
  }

  private async publishInApp(): Promise<void> {
    const site = await this.siteFor(this.findBinary());
    if (!site) return;
    const folder = path.basename(site.root);
    new Notice(publishNotice(folder, site.address));
    shell.openExternal(publishInAppUrl(site.root)).catch(() => {
      new Notice(`Get moss at mosspub.com to publish ${folder}.`, 10000);
    });
  }

  private openPreviewView(): MossPreviewView | null {
    const view = this.app.workspace.getLeavesOfType(MOSS_PREVIEW_VIEW_TYPE)[0]?.view;
    return view instanceof MossPreviewView ? view : null;
  }

  /** `folder` names the pane's tab when a new preview starts. */
  private async openPreviewLeaf(folder?: string): Promise<MossPreviewView | null> {
    const existing = this.app.workspace.getLeavesOfType(MOSS_PREVIEW_VIEW_TYPE);
    let leaf: WorkspaceLeaf | null = existing[0] ?? null;
    const editing = this.app.workspace.getMostRecentLeaf();
    // Beside the editor: a page needs the width, and a sidebar puts other tools' tabs over it.
    leaf ??= this.app.workspace.getLeaf("split", "vertical");
    if (!existing[0] || folder) await leaf.setViewState({ type: MOSS_PREVIEW_VIEW_TYPE, active: false, state: { folder } });
    await this.app.workspace.revealLeaf(leaf);
    // The person goes on writing; the pane follows.
    if (editing && editing !== leaf) this.app.workspace.setActiveLeaf(editing, { focus: true });
    const view = leaf.view;
    return view instanceof MossPreviewView ? view : null;
  }

  /** Nothing shows at rest; a running preview names its folder. */
  private setStatus(text: string | null): void {
    this.statusEl?.toggle(text !== null);
    this.statusEl?.setText(text ?? "");
  }
}

/** A running preview; `folder` is the name every label uses. */
interface Session {
  root: string;
  folder: string;
  url: string | null;
  /** The vault path of the note whose page the pane follows, and the cursor line last sent to it. */
  page?: string;
  line?: number;
  /** Ends the watch on the server's rebuilds. */
  stopWatching?: () => void;
  /** Why the preview failed, kept on screen after the process exits. */
  failure?: string;
  /** The first-preview question while it is open; ending the session closes it. */
  modal?: FirstPreviewModal;
}

function runCli(binary: string, args: string[], timeout = 0): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(binary, args, { maxBuffer: 4 * 1024 * 1024, timeout }, (err, out) => (err ? reject(err) : resolve(out)));
  });
}
