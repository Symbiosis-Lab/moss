import type { MarkdownFileInfo, WorkspaceLeaf } from "obsidian";
import { afterEach, expect, it, vi } from "vitest";
import { TFile } from "obsidian";
import { initializeCore, MossSite, type PagePreview } from "../src/core";
import type MossPlugin from "../src/main";
import { vaultBaseHref } from "../src/site-context";
import { PagePreviewView } from "../src/page-preview-view";

initializeCore();
afterEach(() => vi.useRealTimers());

function fixture() {
  const note = Object.assign(new TFile(), { path: "embed.md", extension: "md" });
  const file = Object.assign(new TFile(), { path: "note.md", extension: "md" });
  const site = new MossSite(JSON.stringify({ files: [file.path, note.path], lang: "en" }));
  const editor = { getValue: () => "# Unsaved page\n![[embed]]" };
  const events: Record<string, () => void> = {};
  const cachedRead = vi.fn(async () => "Embedded words");
  const plugin = { site, app: {
    vault: { cachedRead, getAbstractFileByPath: () => note, on: () => ({}),
      adapter: { getResourcePath: () => "capacitor://localhost/vault/note.md?t=1" } },
    workspace: { activeEditor: { file, editor }, getActiveFile: () => file,
      on: (event: string, callback: () => void) => { events[event] = callback; return {}; } },
  } } as unknown as MossPlugin;
  const attributes: Record<string, string> = {};
  const iframe = { srcdoc: "", setAttribute: (key: string, value: string) => { attributes[key] = value; } };
  const view = new PagePreviewView({} as WorkspaceLeaf, plugin);
  Object.assign(view, { contentEl: { empty() {}, addClass() {}, createEl: () => iframe } });
  return { view, site, iframe, attributes, editor, events, cachedRead, plugin };
}

it("previews current editor text and embedded notes after 250ms in a script-free sandbox", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.view.onOpen();
  await vi.advanceTimersByTimeAsync(249);
  expect(f.iframe.srcdoc).toBe("");
  await vi.advanceTimersByTimeAsync(1);
  expect(f.iframe.srcdoc).toContain("Unsaved page");
  expect(f.iframe.srcdoc).toContain("Embedded words");
  expect(f.iframe.srcdoc).toContain('href="capacitor://localhost/vault/"');
  expect(f.attributes.sandbox).toBe("");
  f.editor.getValue = () => "# Edited page";
  f.events["editor-change"]();
  await vi.advanceTimersByTimeAsync(250);
  expect(f.iframe.srcdoc).toContain("Edited page");
  await f.view.onClose();
  f.site.free();
});

it("drops a stale revision after a newer render and after close", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.view.onOpen();
  const old: PagePreview = JSON.parse(f.site.preview("note.md", "# Old", ""));
  const latest: PagePreview = JSON.parse(f.site.preview("note.md", "# New", ""));
  f.view.showPreview(latest, 3);
  f.view.showPreview(old, 2);
  expect(f.iframe.srcdoc).toBe(latest.html);
  await f.view.onClose();
  f.view.showPreview(old, 4);
  expect(f.iframe.srcdoc).toBe(latest.html);
  f.site.free();
});

it("does not let a slow embedded-note read replace newer text", async () => {
  vi.useFakeTimers();
  const f = fixture();
  let finish!: (text: string) => void;
  f.cachedRead.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  await f.view.onOpen();
  await vi.advanceTimersByTimeAsync(250);
  f.editor.getValue = () => "# Newer text";
  f.events["editor-change"]();
  await vi.advanceTimersByTimeAsync(250);
  finish("Old embed");
  await vi.advanceTimersByTimeAsync(0);
  expect(f.iframe.srcdoc).toContain("Newer text");
  expect(f.iframe.srcdoc).not.toContain("Old embed");
  await f.view.onClose();
  f.site.free();
});

it("keeps the editor source when the pane takes focus and follows file changes", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.view.onOpen();
  f.plugin.app.workspace.activeEditor = null;
  await vi.advanceTimersByTimeAsync(250);
  expect(f.iframe.srcdoc).toContain("Unsaved page");
  f.plugin.app.workspace.activeEditor = {
    file: Object.assign(new TFile(), { path: "next.md", extension: "md" }),
    editor: { getValue: () => "# Another file" },
  } as MarkdownFileInfo;
  f.events["file-open"]();
  await vi.advanceTimersByTimeAsync(250);
  expect(f.iframe.srcdoc).toContain("Another file");
  await f.view.onClose();
  f.site.free();
});

it.each([
  "capacitor://localhost/_capacitor_file_/var/mobile/Example/Vault/",
  "app://abc123/Users/x/Vault/",
])("resolves nested preview images against the vault resource base %s", (root) => {
  const adapter = { getResourcePath: (path: string) => `${root}${path}?1700000000` };
  const base = vaultBaseHref(adapter, "notes/a.md");
  const site = new MossSite(JSON.stringify({ files: ["notes/a.md", "images/pic.png"], lang: "en" }));
  const preview: PagePreview = JSON.parse(site.preview("notes/a.md", "![pic](../images/pic.png) ![[images/pic.png]]", base));
  const documentBase = /<base href="([^"]+)"/.exec(preview.html)![1];
  const sources = [...preview.html.matchAll(/<img\b[^>]*\bsrc="([^"]+)"/g)].map((match) => match[1]);
  expect(sources).toHaveLength(2);
  for (const src of sources) {
    expect(new URL(src, documentBase).href).toBe(adapter.getResourcePath("images/pic.png").split("?")[0]);
  }
  site.free();
});
