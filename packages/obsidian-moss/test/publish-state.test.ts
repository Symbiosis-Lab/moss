import { beforeEach, expect, it, vi } from "vitest";
import { TFile } from "obsidian";
import { Menu } from "./obsidian-stub";
import { PublishState } from "../src/desktop/publish-state";

beforeEach(() => { Menu.shown = null; });

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

it("keeps a copy in the menu after the page stops naming its channel", async () => {
  const note = Object.assign(new TFile(), { path: "a.md", extension: "md" });
  let click: (evt: unknown) => void = () => {};
  const plugin = {
    app: { workspace: { on: () => ({}), getActiveFile: () => note }, metadataCache: { on: () => ({}) } },
    addStatusBarItem: () => ({ addClass() {}, setText() {} }),
    registerDomEvent: (_el: unknown, _name: string, handler: (evt: unknown) => void) => { click = handler; },
    registerEvent() {},
  };
  const page = { path: "/a/", draft: false, published: true, changed: false, channels: ["substack"], copies: { mastodon: "https://social.example/@her/1" } };
  const state = new PublishState(plugin as never, async () => ({ address: "garden.example", page }), { preview() {}, publish() {} });
  await state.update();
  click({});
  const channels = Menu.shown!.items.filter((i) => i.title.startsWith("On "));
  expect(channels).toEqual([{ title: "On substack", disabled: true }, { title: "On mastodon", disabled: false }]);
});
