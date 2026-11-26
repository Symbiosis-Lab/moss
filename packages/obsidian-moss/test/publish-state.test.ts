import { expect, it, vi } from "vitest";
import { TFile } from "obsidian";
import { PublishState } from "../src/desktop/publish-state";

it("asks moss once when opening a note fires both file-open and active-leaf-change, and again after an edit", async () => {
  const handlers: Record<string, (file?: unknown) => void> = {};
  const note = Object.assign(new TFile(), { path: "posts/heron.md", extension: "md" });
  const on = (name: string, handler: (file?: unknown) => void) => { handlers[name] = handler; return {}; };
  const plugin = {
    app: { workspace: { on, getActiveFile: () => note }, metadataCache: { on } },
    addStatusBarItem: () => ({ addClass() {}, setText() {} }),
    registerDomEvent() {}, registerEvent() {},
  };
  const ask = vi.fn(async () => null);
  new PublishState(plugin as never, ask, { preview() {}, publish() {} });

  handlers["file-open"]();
  handlers["active-leaf-change"]();
  await Promise.resolve();
  expect(ask).toHaveBeenCalledTimes(1);

  handlers.changed(note);
  expect(ask).toHaveBeenCalledTimes(2);
});

it("asks moss again when the same note is opened on a later turn, since a publish may have happened", async () => {
  const handlers: Record<string, (file?: unknown) => void> = {};
  const note = Object.assign(new TFile(), { path: "posts/heron.md", extension: "md" });
  const on = (name: string, handler: (file?: unknown) => void) => { handlers[name] = handler; return {}; };
  const plugin = {
    app: { workspace: { on, getActiveFile: () => note }, metadataCache: { on } },
    addStatusBarItem: () => ({ addClass() {}, setText() {} }),
    registerDomEvent() {}, registerEvent() {},
  };
  const ask = vi.fn(async () => null);
  new PublishState(plugin as never, ask, { preview() {}, publish() {} });

  handlers["file-open"]();
  await new Promise((resolve) => setTimeout(resolve, 5));
  handlers["file-open"]();
  expect(ask).toHaveBeenCalledTimes(2);
});
