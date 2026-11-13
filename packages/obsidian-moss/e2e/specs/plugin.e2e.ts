// E2E against a REAL Obsidian (wdio-obsidian-service). Proves the three
// things unit tests cannot: the plugin actually loads, the frontmatter
// diagnostics reach real editor pixels, and the preview command spawns the
// CLI (shim), parses its output, and shows the served page in the pane.
import { browser, expect } from "@wdio/globals";
import { obsidianPage } from "wdio-obsidian-service";

/** Starts the preview and answers the first-build question (the shim never creates .moss/). */
async function startPreview(): Promise<void> {
  await browser.executeObsidianCommand("moss:preview");
  const proceed = browser.$(".modal button.mod-cta");
  if (await proceed.waitForExist({ timeout: 3000 }).catch(() => false)) await proceed.click();
}

describe("moss plugin", () => {
  it("loads in the vault", async () => {
    const loaded = await browser.executeObsidian(({ app }) => {
      type WithPlugins = { plugins: { plugins: Record<string, unknown> } };
      return Boolean((app as unknown as WithPlugins).plugins.plugins["moss"]);
    });
    expect(loaded).toBe(true);
  });

  it("shows frontmatter diagnostics in the editor", async () => {
    await obsidianPage.openFile("bad-frontmatter.md");
    // Force source mode so the raw frontmatter text (and its decorations)
    // is what renders.
    await browser.executeObsidian(({ app }) => {
      const leaf = app.workspace.getMostRecentLeaf();
      if (!leaf) return;
      const state = leaf.getViewState();
      state.state = { ...state.state, mode: "source", source: true };
      void leaf.setViewState(state);
    });

    const unknown = browser.$(".moss-diag-hint");
    await unknown.waitForExist({ timeout: 10000 });
    expect(await unknown.getText()).toBe("titel");

    const wrongType = browser.$("span.moss-diag-error");
    await wrongType.waitForExist({ timeout: 10000 });
    expect(await wrongType.getText()).toBe("draft");

    // The clean note gets no decorations. Scope to the ACTIVE leaf —
    // Obsidian keeps the previous tab's DOM alive in the background.
    await obsidianPage.openFile("welcome.md");
    await browser.waitUntil(
      async () =>
        !(await browser.$(".workspace-leaf.mod-active .moss-diag-hint").isExisting()) &&
        (await browser.$(".workspace-leaf.mod-active .cm-content").getText()).includes("Welcome"),
      { timeout: 10000, timeoutMsg: "diagnostics lingered in the active editor" },
    );
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
    await startPreview();

    const iframe = browser.$("iframe.moss-preview-iframe");
    await iframe.waitForExist({ timeout: 30000 });
    await browser.waitUntil(
      async () => /^http:\/\/localhost:\d+/.test((await iframe.getAttribute("src")) ?? ""),
      { timeout: 30000, timeoutMsg: "iframe src never pointed at the shim server" },
    );
    const src = await iframe.getAttribute("src");
    expect(src).toMatch(/^http:\/\/localhost:\d+/);

    // Prove real bytes rendered, not just an attribute: enter the frame and
    // read the served page.
    await browser.switchFrame(iframe);
    const heading = browser.$("h1");
    await heading.waitForExist({ timeout: 15000 });
    expect(await heading.getText()).toBe("Hello from moss shim");
    await browser.switchFrame(null);

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
});
