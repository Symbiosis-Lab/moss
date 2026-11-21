import { EventEmitter } from "node:events";
import { expect, it, vi } from "vitest";
import { DesktopCommands } from "../src/desktop";
import { MossPreviewView } from "../src/desktop/preview-view";
import { MossProcess, type SpawnFn } from "../src/desktop/process";

const shell = vi.hoisted(() => ({ openExternal: vi.fn(async (_url: string) => {}) }));
vi.mock("electron", () => ({ shell }));

it("stops a running preview when the Obsidian window unloads", () => {
  const handlers: Record<string, () => void> = {};
  const statusEl = { addClass() {}, toggle() {}, setText() {} };
  const plugin = {
    app: { workspace: { on: () => ({}), onLayoutReady() {} }, metadataCache: { on: () => ({}) } },
    addSettingTab() {}, registerView() {}, addCommand() {}, addRibbonIcon() {}, registerEvent() {}, registerEditorExtension() {},
    addStatusBarItem: () => statusEl,
    registerDomEvent(_target: unknown, type: string, handler: () => void) { if (type !== "click") handlers[type] = handler; },
  };
  const child = Object.assign(new EventEmitter(), { stderr: new EventEmitter(), stdout: new EventEmitter(), kill: vi.fn() });
  vi.stubGlobal("window", {});
  const commands = new DesktopCommands(plugin as never);
  const preview = new MossProcess((() => child) as unknown as SpawnFn);
  (commands as unknown as { mossProcess: MossProcess }).mossProcess = preview;
  commands.load();
  preview.start("/bin/moss", [], "/v", {});

  handlers.beforeunload();

  expect(child.kill).toHaveBeenCalledWith("SIGTERM");
  vi.unstubAllGlobals();
});

it("opens in the browser the page showing, unless the page names an address off the preview's server", async () => {
  const address = vi.fn();
  const view = Object.assign(Object.create(MossPreviewView.prototype), { address });
  const plugin = { app: { workspace: { getLeavesOfType: () => [{ view }] } } };
  const commands = new DesktopCommands(plugin as never) as unknown as { session: unknown; openInBrowser(): Promise<void> };
  commands.session = { root: "/v/site", folder: "site", url: "http://localhost:8080" };
  const set = "http://localhost:8080/posts/a/";

  address.mockResolvedValue({ set, showing: "http://localhost:8080/posts/b/" });
  await commands.openInBrowser();
  address.mockResolvedValue({ set, showing: "file:///etc/passwd" });
  await commands.openInBrowser();
  address.mockResolvedValue({ set, showing: "http://localhost:80809/posts/b/" });
  await commands.openInBrowser();
  address.mockResolvedValue({ set });
  await commands.openInBrowser();

  expect(shell.openExternal.mock.calls.map(([url]) => url)).toEqual(["http://localhost:8080/posts/b/", set, set, set]);
});
