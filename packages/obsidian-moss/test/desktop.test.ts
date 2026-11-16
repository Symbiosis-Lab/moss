import { EventEmitter } from "node:events";
import { expect, it, vi } from "vitest";
import { DesktopCommands } from "../src/desktop";
import { MossProcess, type SpawnFn } from "../src/desktop/process";

vi.mock("electron", () => ({ shell: {} }));

it("stops a running preview when the Obsidian window unloads", () => {
  const handlers: Record<string, () => void> = {};
  const statusEl = { addClass() {}, toggle() {}, setText() {} };
  const plugin = {
    app: { workspace: { on: () => ({}) } },
    addSettingTab() {}, registerView() {}, addCommand() {}, addRibbonIcon() {}, registerEvent() {},
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
