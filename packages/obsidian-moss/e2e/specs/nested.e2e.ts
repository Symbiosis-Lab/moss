// A moss folder nested inside a vault, in a REAL Obsidian: the mark on its row
// in the file tree, the preview pane's place, and the pane following the note
// being written. The following needs a real preview server, so those tests run
// only when MOSS_E2E_MOSS_DIR names a real moss (see wdio.conf.mts).
import { browser, expect } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import { obsidianPage } from "wdio-obsidian-service";

const FRAME = "iframe.moss-preview-iframe";
const realMoss = Boolean(process.env.MOSS_E2E_MOSS_DIR);

async function screenshot(name: string): Promise<void> {
  if (process.env.MOSS_E2E_SHOTS) await browser.saveScreenshot(path.join(process.env.MOSS_E2E_SHOTS, `${name}.png`));
}

/** The `content` of the mark's pseudo-element on a folder's row: `none` without the mark. */
const mark = (folder: string) => browser.execute((folder) => {
  const name = document.querySelector(`.nav-folder-title[data-path="${folder}"] > .nav-folder-title-content`);
  return name ? getComputedStyle(name, "::before").content : "no such row";
}, folder);

/** Runs `read` inside the previewed page. */
async function inPage<T>(read: () => T): Promise<T> {
  await browser.switchFrame(browser.$(FRAME));
  try {
    return await browser.execute(read) as T;
  } finally {
    await browser.switchFrame(null);
  }
}

const pagePath = () => inPage(() => location.pathname);
const shows = (page: string) => browser.waitUntil(async () => (await pagePath()).includes(page), {
  timeout: 30000, timeoutMsg: `the pane never showed ${page}`,
});

/** The pane's own header buttons that show, left to right. */
const headerButtons = () => browser.execute(() =>
  [...document.querySelectorAll('.workspace-leaf-content[data-type="moss-preview"] .view-header .clickable-icon')]
    .filter((button) => (button as HTMLElement).offsetParent !== null)
    .map((button) => button.getAttribute("aria-label")));

/** Presses a header button and reads what the press left on it in the same turn, then how it looks 150ms on. */
const press = (label: string) => browser.execute(async (label) => {
  const button = document.querySelector<HTMLElement>(`.workspace-leaf-content[data-type="moss-preview"] .view-action[aria-label="${label}"]`);
  if (!button) throw new Error(`no ${label} button`);
  button.click();
  const atOnce = button.ariaBusy;
  await new Promise((resolve) => setTimeout(resolve, 150));
  return { atOnce, ring: getComputedStyle(button, "::after").opacity, pressable: getComputedStyle(button).pointerEvents };
}, label);

type Opens = { urls: string[]; release: (() => void)[] };

/** Stands in for the system's opener, which would leave Obsidian; each address waits until `release`. */
const holdOpens = () => browser.execute(() => {
  const opens: Opens = { urls: [], release: [] };
  (window as unknown as { mossOpens: Opens }).mossOpens = opens;
  const { shell } = (window as unknown as { require(name: string): { shell: { openExternal(url: string): Promise<void> } } }).require("electron");
  shell.openExternal = (url) => {
    opens.urls.push(url);
    return new Promise((done) => opens.release.push(done));
  };
});
const opened = () => browser.execute(() => (window as unknown as { mossOpens: Opens }).mossOpens.urls);
const release = () => browser.execute(() => (window as unknown as { mossOpens: Opens }).mossOpens.release.splice(0).forEach((done) => done()));
const busy = () => browser.execute(() => [...document.querySelectorAll('.view-action[aria-busy="true"]')].length);

async function openSource(file: string, source = true): Promise<void> {
  await obsidianPage.openFile(file);
  await browser.executeObsidian(async ({ app }, source) => {
    const leaf = app.workspace.getMostRecentLeaf();
    if (!leaf) return;
    const state = leaf.getViewState();
    state.state = { ...state.state, mode: "source", source };
    await leaf.setViewState(state);
  }, source);
}

async function startPreview(): Promise<void> {
  await browser.executeObsidianCommand("moss:preview");
  const proceed = browser.$(".modal button.mod-cta");
  if (await proceed.waitForExist({ timeout: 3000 }).catch(() => false)) await proceed.click();
  await browser.$(FRAME).waitForExist({ timeout: 60000 });
}

describe("a moss folder inside a vault", () => {
  before(async () => {
    await browser.execute(() => window.resizeTo(1440, 900));
    await browser.executeObsidian(({ app }) => {
      const explorer = app.workspace.getLeavesOfType("file-explorer")[0];
      (explorer?.view as unknown as { tree?: { setCollapseAll?(collapse: boolean): void } })?.tree?.setCollapseAll?.(false);
    });
  });

  it("marks the folder in the file tree once it is a moss folder, and no other", async () => {
    await obsidianPage.openFile("site/posts/first-frost.md");
    await browser.pause(1500);
    expect(await mark("site")).toBe("none");
    await obsidianPage.write("site/.moss/config.toml", "");
    await browser.waitUntil(async () => (await mark("site")) !== "none", { timeout: 10000, timeoutMsg: "the moss folder never got its mark" });
    expect(await mark("Daily notes")).toBe("none");
    expect(await mark("site/posts")).toBe("none");
    await screenshot("mark-on-the-moss-folder");
  });

  it("marks a folder whatever its name, and exactly the folders moss itself calls sites", async () => {
    const odd = `花园 [it's "here"]`;
    await obsidianPage.write(`${odd}/leaf.md`, "A leaf.\n");
    await obsidianPage.write(`${odd}/.moss/config.toml`, "");
    const marked = () => browser.execute(() =>
      [...document.querySelectorAll(".nav-folder-title.moss-folder")].map((row) => row.getAttribute("data-path")).sort());
    await browser.waitUntil(async () => (await marked()).length === 2, { timeout: 10000, timeoutMsg: "the second moss folder never got its mark" });
    expect(await marked()).toEqual(["site", odd].sort());
    if (!realMoss) return;
    const vault = fs.realpathSync(await browser.executeObsidian(({ app }) => (app.vault.adapter as unknown as { getBasePath(): string }).getBasePath()));
    const folders = await browser.execute(() => [...document.querySelectorAll(".nav-folder-title[data-path]")].map((row) => row.getAttribute("data-path") ?? ""));
    const sites = folders.filter((folder) => {
      const at = path.join(vault, folder);
      const { root } = JSON.parse(execFileSync(path.join(process.env.MOSS_E2E_MOSS_DIR ?? "", "moss"), ["root", at, "--json"], { encoding: "utf8" }));
      return root !== null && fs.realpathSync(root) === at;
    });
    expect(await marked()).toEqual(sites.sort());
  });

  it("keeps the mark on a moss folder through a rename, and off a renamed sibling", async () => {
    const rename = (from: string, to: string) => browser.executeObsidian(async ({ app }, from, to) => {
      const folder = app.vault.getAbstractFileByPath(from);
      if (!folder) throw new Error(`no ${from}`);
      await app.fileManager.renameFile(folder, to);
    }, from, to);
    const marked = () => browser.execute(() =>
      [...document.querySelectorAll(".nav-folder-title.moss-folder")].map((row) => row.getAttribute("data-path")).sort());
    await rename(`花园 [it's "here"]`, "moss garden");
    await rename("Daily notes", "Day notes");
    await browser.waitUntil(async () => (await marked()).includes("moss garden"), { timeout: 10000, timeoutMsg: "the mark did not follow the renamed moss folder" });
    expect(await marked()).toEqual(["moss garden", "site"]);
    await rename("Day notes", "Daily notes");
    await browser.pause(1000);
    expect(await marked()).toEqual(["moss garden", "site"]);
  });

  it("checks a note in the moss folder against that folder, and leaves notes outside it alone", async () => {
    const note = "---\ntitel: Frost\ndraft: \"yes\"\n---\n\nSee [[posts/first-frost]] and [[nowhere]].\n";
    await obsidianPage.write("site/posts/checked.md", note);
    await obsidianPage.write("Daily notes/checked.md", note);
    const diagnostics = () => browser.execute(() => [...document.querySelectorAll(".workspace-leaf.mod-active [class*='moss-diag-']")]
      .map((element) => element.classList.contains("cm-line") ? "line" : element.textContent));
    await openSource("site/posts/checked.md");
    // The link to a page of the folder resolves by its path there; the one to no page does not.
    await browser.waitUntil(async () => (await diagnostics()).length === 3, { timeout: 10000, timeoutMsg: `the note was not checked: ${await diagnostics()}` });
    expect((await diagnostics()).sort()).toEqual(["draft", "line", "titel"]);
    await openSource("Daily notes/checked.md");
    await browser.pause(1500);
    expect(await diagnostics()).toEqual([]);
    // The preview below starts from the open note, so leave the folder's note open, as written.
    await openSource("site/posts/first-frost.md", false);
  });

  it("opens the preview beside the editor, with no other tool's tab or button over it", async () => {
    await startPreview();
    const pane = await browser.execute((frame) => {
      const leaf = document.querySelector(frame)?.closest(".workspace-leaf");
      const tabs = leaf?.closest(".workspace-tabs");
      return {
        inSidebar: Boolean(leaf?.closest(".mod-left-split, .mod-right-split")),
        tabTypes: [...(tabs?.querySelectorAll(".workspace-tab-header") ?? [])].map((tab) => tab.getAttribute("data-type")),
        tabTitle: tabs?.querySelector(".workspace-tab-header.is-active .workspace-tab-header-inner-title")?.textContent,
        headerTitle: leaf?.querySelector(".view-header-title")?.textContent,
      };
    }, FRAME);
    expect(pane.inSidebar).toBe(false);
    expect(pane.tabTypes).toEqual(["moss-preview"]);
    expect(await headerButtons()).toEqual(["Open in browser", "Publish this folder", "More options"]);
    expect(pane.headerTitle).toBe("Preview: site");
    expect(pane.tabTitle).toBe("Preview: site");
    // The person goes on writing: the note keeps the focus.
    expect(await browser.execute(() => document.querySelector(".workspace-leaf.mod-active")?.querySelector(".cm-editor") !== null)).toBe(true);
    await screenshot("pane-header");
  });

  (realMoss ? describe : describe.skip)("with the moss program", () => {
    it("shows the page of the note that is open, and follows to the next note", async () => {
      await shows("first-frost");
      await screenshot("follows-first-note");
      await obsidianPage.openFile("site/posts/tide-tables.md");
      await shows("tide-tables");
      await screenshot("follows-next-note");
    });

    it("shows an edit after Obsidian saves it, in place", async () => {
      await inPage(() => {
        (window as unknown as { keptAcrossTheEdit?: boolean }).keptAcrossTheEdit = true;
        window.scrollTo(0, 300);
      });
      await browser.executeObsidian(({ app }) => {
        const editor = app.workspace.activeEditor?.editor;
        if (!editor) throw new Error("no editor");
        editor.replaceRange("\n\nSpring tides fall on the ninth.\n", { line: editor.lastLine(), ch: 0 });
      });
      await browser.waitUntil(() => inPage(() => document.body.innerText.includes("Spring tides fall on the ninth.")), {
        timeout: 30000, timeoutMsg: "the edit never reached the pane",
      });
      // Updated in place: the same window, where the reader had scrolled to.
      const after = await inPage(() => ({
        kept: (window as unknown as { keptAcrossTheEdit?: boolean }).keptAcrossTheEdit === true,
        scrollY: window.scrollY,
      }));
      expect(after.kept).toBe(true);
      expect(after.scrollY).toBeGreaterThan(200);
      await screenshot("edit-arrives");
    });

    it("brings the passage being edited into view", async () => {
      const visible = () => inPage(() => {
        const heading = [...document.querySelectorAll("h2")].find((h) => h.textContent?.includes("Part 8"));
        const top = heading?.getBoundingClientRect().top ?? -1;
        return top >= 0 && top < window.innerHeight;
      });
      await inPage(() => window.scrollTo(0, 0));
      expect(await visible()).toBe(false);
      await browser.executeObsidian(({ app }) => {
        const editor = app.workspace.activeEditor?.editor;
        if (!editor) throw new Error("no editor");
        const line = editor.getValue().split("\n").findIndex((text) => text.startsWith("## Part 8"));
        editor.setCursor({ line, ch: 0 });
      });
      await browser.waitUntil(visible, { timeout: 10000, timeoutMsg: "the pane never scrolled to the passage under the cursor" });
      await screenshot("passage-in-view");
    });

    it("stays on its page for a note outside the folder, and while pinned", async () => {
      await obsidianPage.openFile("Daily notes/2026-11-19.md");
      await browser.pause(3000);
      expect(await pagePath()).toContain("tide-tables");
      await screenshot("note-outside-the-folder");

      const pin = (pinned: boolean) => browser.executeObsidian(({ app }, pinned) => {
        app.workspace.getLeavesOfType("moss-preview")[0]?.setPinned(pinned);
      }, pinned);
      await pin(true);
      await obsidianPage.openFile("site/posts/first-frost.md");
      await browser.pause(3000);
      expect(await pagePath()).toContain("tide-tables");
      await pin(false);
      await obsidianPage.openFile("site/index.md");
      await browser.waitUntil(async () => (await pagePath()) === "/", { timeout: 30000, timeoutMsg: "the unpinned pane never followed" });
    });

    it("opens the page that is showing in the browser, and answers the press at once", async () => {
      await holdOpens();
      await obsidianPage.openFile("site/posts/first-frost.md");
      await shows("first-frost");
      const origin = await inPage(() => location.origin);
      expect(await press("Open in browser")).toEqual({ atOnce: "true", ring: "1", pressable: "none" });
      await screenshot("button-waiting");
      expect((await opened()).map((url) => [new URL(url).origin, new URL(url).pathname])).toEqual([[origin, await pagePath()]]);
      await release();
      await browser.waitUntil(async () => (await busy()) === 0, { timeoutMsg: "the button stayed busy after its answer" });

      // A link followed inside the page: the pane was never pointed there.
      await inPage(() => location.assign(location.pathname.replace("first-frost", "tide-tables")));
      await shows("tide-tables");
      await press("Open in browser");
      expect(new URL((await opened())[1]).pathname).toContain("tide-tables");
      await release();
    });

    it("publishes the pane's folder, whatever note is open", async () => {
      await holdOpens();
      await obsidianPage.openFile("Daily notes/2026-11-19.md");
      expect((await press("Publish this folder")).atOnce).toBe("true");
      await browser.waitUntil(async () => (await opened()).length === 1, { timeout: 15000, timeoutMsg: "Publish this folder opened nothing" });
      const vault = await browser.executeObsidian(({ app }) => (app.vault.adapter as unknown as { getBasePath(): string }).getBasePath());
      const handed = new URL((await opened())[0]);
      expect(handed.protocol + handed.host).toBe("moss:deploy");
      expect(fs.realpathSync(handed.searchParams.get("path") ?? "")).toBe(fs.realpathSync(path.join(vault, "site")));
      await release();
      await browser.waitUntil(async () => (await busy()) === 0, { timeoutMsg: "the button stayed busy after its answer" });
      await screenshot("header-buttons");
    });

    it("carries no moss button once the preview stops", async () => {
      await browser.executeObsidianCommand("moss:stop-preview");
      await browser.$(".moss-preview-message").waitForExist({ timeout: 15000 });
      expect(await headerButtons()).toEqual(["More options"]);
    });
  });
});
