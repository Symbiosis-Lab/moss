// E2E against a REAL Obsidian (wdio-obsidian-service). Proves what unit
// tests cannot: the plugin loads, leaves a vault that is not a moss folder
// untouched, shows frontmatter diagnostics in real editor pixels once the
// vault becomes one, and the preview command spawns the CLI (the shim, or a real moss), parses
// its output, and shows the served page in the pane.
import { browser, expect } from "@wdio/globals";
import * as path from "node:path";
import { obsidianPage } from "wdio-obsidian-service";

const realMoss = Boolean(process.env.MOSS_E2E_MOSS_DIR);

/** Starts the preview and answers the first-build question (the shim never creates .moss/). */
async function startPreview(): Promise<void> {
  await browser.executeObsidianCommand("moss:preview");
  const proceed = browser.$(".modal button.mod-cta");
  if (await proceed.waitForExist({ timeout: 3000 }).catch(() => false)) await proceed.click();
}

/** Opens a note in source mode, so the raw frontmatter text and its decorations render. */
async function openSource(file: string): Promise<void> {
  await obsidianPage.openFile(file);
  await browser.executeObsidian(async ({ app }) => {
    const leaf = app.workspace.getMostRecentLeaf();
    if (!leaf) return;
    const state = leaf.getViewState();
    state.state = { ...state.state, mode: "source", source: true };
    await leaf.setViewState(state);
  });
}

const stat = (file: string) => browser.executeObsidian(({ app }, file) => app.vault.adapter.stat(file), file);
const activeDiagnostics = () => browser.$$(".workspace-leaf.mod-active [class*='moss-diag-']").length;

async function screenshot(name: string): Promise<void> {
  if (process.env.MOSS_E2E_SHOTS) await browser.saveScreenshot(path.join(process.env.MOSS_E2E_SHOTS, `${name}.png`));
}

describe("moss plugin", () => {
  it("loads in the vault", async () => {
    const loaded = await browser.executeObsidian(({ app }) => {
      type WithPlugins = { plugins: { plugins: Record<string, unknown> } };
      return Boolean((app as unknown as WithPlugins).plugins.plugins["moss"]);
    });
    expect(loaded).toBe(true);
  });

  it("checks, marks and writes nothing in a vault that is not a moss folder", async () => {
    const notes = ["bad-frontmatter.md", "2026-11-17.md"];
    const before = await Promise.all(notes.map(stat));
    for (const note of notes) {
      await openSource(note);
      // Past layout-ready and the plugin's 250 ms context rebuild.
      await browser.pause(2000);
      expect(await browser.$(".workspace-leaf.mod-active .cm-content").getText()).not.toBe("");
      expect(await activeDiagnostics()).toBe(0);
    }
    await screenshot("not-a-moss-folder");
    expect(await Promise.all(notes.map(stat))).toEqual(before);
    expect(await stat(".moss")).toBeNull();
  });

  it("asks before a first preview, and Cancel starts nothing", async () => {
    await browser.executeObsidianCommand("moss:preview");
    const modal = browser.$(".modal");
    await modal.waitForExist({ timeout: 10000 });
    const text = await modal.getText();
    expect(text).toContain("adds a .moss folder");
    expect(text).not.toContain("uid");
    // While the question is open, a second click asks nothing new and Stop is not offered: nothing runs yet.
    await browser.executeObsidianCommand("moss:preview");
    await browser.pause(1000);
    expect(await browser.$$(".modal").length).toBe(1);
    const stopOffered = await browser.executeObsidian(({ app }) =>
      (app as unknown as { commands: { findCommand(id: string): { checkCallback?(checking: boolean): boolean } | undefined } })
        .commands.findCommand("moss:stop-preview")?.checkCallback?.(true));
    expect(stopOffered).toBe(false);
    await browser.$(".modal button:not(.mod-cta)").click();
    await browser.pause(1000);
    expect(await browser.$("iframe.moss-preview-iframe").isExisting()).toBe(false);
    expect(await browser.$(".status-bar-item.plugin-moss").isDisplayed()).toBe(false);
  });

  it("runs Preview this folder: spawns the CLI, parses the URL, shows the page", async () => {
    // A real moss shows the open note's page.
    if (realMoss) await obsidianPage.openFile("welcome.md");
    await startPreview();

    const iframe = browser.$("iframe.moss-preview-iframe");
    await iframe.waitForExist({ timeout: 30000 });
    await browser.waitUntil(
      async () => /^http:\/\/localhost:\d+/.test((await iframe.getAttribute("src")) ?? ""),
      { timeout: 30000, timeoutMsg: "iframe src never pointed at the preview server" },
    );
    const src = await iframe.getAttribute("src");
    expect(src).toMatch(/^http:\/\/localhost:\d+/);

    // Prove real bytes rendered, not just an attribute: enter the frame and
    // read the served page.
    await browser.switchFrame(iframe);
    try {
      // The shim serves one fixed page.
      const expected = realMoss ? "Welcome" : "Hello from moss shim";
      await browser.waitUntil(async () => (await browser.$("body").getText()).includes(expected), {
        timeout: 30000, timeoutMsg: `the served page never showed "${expected}"`,
      });
    } finally {
      await browser.switchFrame(null);
    }

    // Status bar reflects the running server (the plugin's own item).
    const status = browser.$(".status-bar-item.plugin-moss");
    await status.waitForExist({ timeout: 10000 });
    expect(await status.getText()).toContain("Previewing");

    await browser.executeObsidianCommand("moss:stop-preview");
    await browser.waitUntil(async () => !(await status.isDisplayed()), {
      timeout: 15000,
      timeoutMsg: "the status bar still shows a preview after Stop preview",
    });
    // The pane says the preview stopped instead of showing the last page.
    expect(await browser.$("iframe.moss-preview-iframe").isExisting()).toBe(false);
    expect(await browser.$(".moss-preview-message").getText()).toContain("Not previewing");
  });

  it("stops the server when the preview pane is closed", async () => {
    await startPreview();
    const status = browser.$(".status-bar-item.plugin-moss");
    await browser.waitUntil(async () => (await status.getText()).includes("Previewing"), {
      timeout: 30000,
      timeoutMsg: "preview never started",
    });
    await browser.executeObsidian(({ app }) => app.workspace.detachLeavesOfType("moss-preview"));
    await browser.waitUntil(async () => !(await status.isDisplayed()), {
      timeout: 15000,
      timeoutMsg: "closing the pane left the server running",
    });
  });

  it("shows frontmatter diagnostics once the vault becomes a moss folder", async () => {
    await obsidianPage.write(".moss/config.toml", "");
    await openSource("bad-frontmatter.md");
    const unknown = browser.$(".workspace-leaf.mod-active .moss-diag-hint");
    await unknown.waitForExist({ timeout: 10000 });
    expect(await unknown.getText()).toBe("titel");
    const wrongType = browser.$(".workspace-leaf.mod-active span.moss-diag-error");
    await wrongType.waitForExist({ timeout: 10000 });
    expect(await wrongType.getText()).toBe("draft");
    await screenshot("moss-folder");

    // A note without frontmatter takes its title from the file name. Scope to
    // the ACTIVE leaf: Obsidian keeps the previous tab's DOM alive.
    for (const note of ["2026-11-17.md", "welcome.md"]) {
      await openSource(note);
      await browser.pause(1000);
      expect(await browser.$(".workspace-leaf.mod-active .cm-content").getText()).not.toBe("");
      expect(await activeDiagnostics()).toBe(0);
    }
  });
});
